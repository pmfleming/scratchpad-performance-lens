import argparse
import json
import os
import subprocess
import sys
import threading
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

from perf_report_shared import (
    load_benchmark_metadata,
    matching_flamegraph_ids,
    metadata_for_benchmark,
)
from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("slowspots.json")
VISIBILITY_OUTPUT = Path("target/analysis/slowspots.json")


@dataclass
class PerfMetrics:
    name: str
    mean_ns: float
    std_dev_ns: float
    median_ns: float
    max_ns: float
    min_ns: float
    dispersion_ns: Optional[float] = None
    dispersion_label: str = "median_abs_dev"
    score: float = 0.0
    signals: str = ""
    benchmark_key: str = ""
    targets: Optional[List[str]] = None
    benchmark_kind: str = "unmapped"
    workload_family: str = "unmapped"
    threshold_ms: float = 50.0
    matching_flamegraphs: Optional[List[str]] = None
    has_profile_coverage: bool = False
    stability: str = "stable"
    suspected_limiting_resource: str = "cpu"
    probe_class: str = "targeted_path"
    measurement_role: str = "change_validation"
    measurement_question: str = "Did this implementation path stay inside its latency budget?"

    @property
    def mean_ms(self) -> float:
        return self.mean_ns / 1_000_000.0

    @property
    def dispersion_ms(self) -> Optional[float]:
        if self.dispersion_ns is None:
            return None
        return self.dispersion_ns / 1_000_000.0


class SlowspotAnalyzer:
    def __init__(self, threshold_ms: float = 50.0):
        self.threshold_ms = threshold_ms
        self.benchmark_metadata = load_benchmark_metadata(self.threshold_ms)

    def run_benchmarks(self, skip_bench: bool = False) -> List[PerfMetrics]:
        if not skip_bench:
            print("Running benchmarks via cargo bench...", file=sys.stderr)
            try:
                self.run_bench_command(
                    ["cargo", "bench", "--bench", "search_speed", "--bench", "frame_budget"]
                )
            except Exception as exc:
                print(f"Benchmarking failed: {exc}", file=sys.stderr)
                return []

        results = self.load_criterion_results(os.path.join("target", "criterion"))
        if not results:
            if not skip_bench:
                print(
                    "Error: No Criterion benchmark results were discovered.",
                    file=sys.stderr,
                )
            return []

        return sorted(results, key=lambda item: -item.score)

    def run_bench_command(self, cmd: List[str]) -> None:
        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )

        status = {"current": "starting cargo bench", "done": False}

        def progress_reporter() -> None:
            start = time.time()
            while not status["done"]:
                elapsed = time.time() - start
                print(
                    f"[progress {elapsed:5.1f}s] {status['current']}",
                    file=sys.stderr,
                    flush=True,
                )
                time.sleep(5)

        reporter = threading.Thread(target=progress_reporter, daemon=True)
        reporter.start()

        captured_output = []
        try:
            assert process.stdout is not None
            for raw_line in process.stdout:
                line = raw_line.rstrip()
                captured_output.append(raw_line)
                benchmark_name = self.parse_benchmark_progress(line)
                if benchmark_name is not None:
                    status["current"] = benchmark_name

            return_code = process.wait()
        finally:
            status["done"] = True
            reporter.join(timeout=1)

        if return_code != 0:
            raise subprocess.CalledProcessError(
                return_code,
                cmd,
                output="".join(captured_output),
            )

    def parse_benchmark_progress(self, line: str) -> Optional[str]:
        prefixes = ["Benchmarking ", "Running ", "Compiling ", "Finished "]
        for prefix in prefixes:
            if line.startswith(prefix):
                return line
        return None

    def load_criterion_results(self, criterion_dir: str) -> List[PerfMetrics]:
        if not os.path.exists(criterion_dir):
            print(
                f"Error: Criterion results directory not found at {criterion_dir}",
                file=sys.stderr,
            )
            return []

        results = []
        for root, _, files in os.walk(criterion_dir):
            if "estimates.json" not in files:
                continue
            estimates_path = os.path.join(root, "estimates.json")
            if os.path.basename(os.path.dirname(estimates_path)) != "new":
                continue

            with open(estimates_path, "r", encoding="utf-8") as f:
                data = json.load(f)

            benchmark_name = self.benchmark_name_from_estimate_path(
                criterion_dir, estimates_path
            )
            benchmark_key = self.benchmark_key(benchmark_name)
            if benchmark_key not in self.benchmark_metadata:
                print(
                    f"Skipping stale or unmapped Criterion result: {benchmark_name}",
                    file=sys.stderr,
                )
                continue
            metadata = self.metadata_for_benchmark(benchmark_name)
            mean = data.get("mean", {}).get("point_estimate", 0.0)
            std_dev = data.get("std_dev", {}).get("point_estimate", 0.0)
            median = data.get("median", {}).get("point_estimate", 0.0)
            dispersion = data.get("median_abs_dev", {}).get("point_estimate")
            flamegraphs = matching_flamegraph_ids(self.benchmark_key(benchmark_name))

            metric = PerfMetrics(
                name=benchmark_name,
                mean_ns=mean,
                std_dev_ns=std_dev,
                median_ns=median,
                max_ns=mean + (2 * std_dev),
                min_ns=max(0, mean - (2 * std_dev)),
                dispersion_ns=dispersion,
                benchmark_key=benchmark_key,
                targets=metadata["targets"],
                benchmark_kind=metadata["kind"],
                workload_family=str(metadata.get("workload_family", "unmapped")),
                threshold_ms=metadata["threshold_ms"],
                matching_flamegraphs=flamegraphs,
                has_profile_coverage=bool(flamegraphs),
                suspected_limiting_resource=str(
                    metadata.get("limiting_resource_hint", "cpu")
                ),
            )
            metric.stability = self.stability_label(metric)
            metric.score = self.calculate_score(metric)
            metric.signals = self.generate_signals(metric)
            results.append(metric)

        return results

    def benchmark_name_from_estimate_path(
        self, criterion_dir: str, estimates_path: str
    ) -> str:
        relative = os.path.relpath(estimates_path, criterion_dir)
        parts = relative.split(os.sep)
        return "/".join(parts[:-2])

    def benchmark_key(self, benchmark_name: str) -> str:
        return benchmark_name.split("/", 1)[0]

    def metadata_for_benchmark(self, benchmark_name: str) -> Dict[str, Any]:
        return metadata_for_benchmark(
            benchmark_name,
            self.benchmark_metadata,
            self.threshold_ms,
        )

    def get_mock_data(self) -> List[PerfMetrics]:
        mock = [
            PerfMetrics(
                "tab_stress_operations",
                45_000_000.0,
                5_000_000.0,
                44_000_000.0,
                55_000_000.0,
                35_000_000.0,
            ),
            PerfMetrics(
                "file_open_latency",
                120_000_000.0,
                20_000_000.0,
                115_000_000.0,
                160_000_000.0,
                80_000_000.0,
            ),
            PerfMetrics(
                "buffer_search_regex",
                8_000_000.0,
                1_000_000.0,
                7_500_000.0,
                10_000_000.0,
                6_000_000.0,
            ),
            PerfMetrics(
                "ui_render_frame",
                12_000_000.0,
                2_000_000.0,
                11_000_000.0,
                16_000_000.0,
                8_000_000.0,
            ),
        ]
        for metric in mock:
            metadata = self.metadata_for_benchmark(metric.name)
            metric.benchmark_key = self.benchmark_key(metric.name)
            metric.targets = metadata["targets"]
            metric.benchmark_kind = metadata["kind"]
            metric.workload_family = str(metadata.get("workload_family", "unmapped"))
            metric.threshold_ms = metadata["threshold_ms"]
            metric.matching_flamegraphs = matching_flamegraph_ids(metric.benchmark_key)
            metric.has_profile_coverage = bool(metric.matching_flamegraphs)
            metric.suspected_limiting_resource = str(
                metadata.get("limiting_resource_hint", "cpu")
            )
            metric.stability = self.stability_label(metric)
            metric.score = self.calculate_score(metric)
            metric.signals = self.generate_signals(metric)
        return sorted(mock, key=lambda item: -item.score)

    def calculate_score(self, metric: PerfMetrics) -> float:
        score = metric.mean_ms
        if metric.mean_ns > 0:
            relative_std_dev = metric.std_dev_ns / metric.mean_ns
            score *= 1.0 + relative_std_dev
        return round(score, 2)

    def generate_signals(self, metric: PerfMetrics) -> str:
        signals = []
        if metric.mean_ms > metric.threshold_ms:
            signals.append(f"slow > {metric.threshold_ms}ms")
        if metric.mean_ns > 1_000_000 and (metric.std_dev_ns / metric.mean_ns) > 0.2:
            signals.append("high variance")
        if not metric.targets:
            signals.append("unmapped benchmark")
        if metric.matching_flamegraphs:
            signals.append("profile coverage")
        return ", ".join(signals) if signals else "nominal"

    def stability_label(self, metric: PerfMetrics) -> str:
        if metric.mean_ns <= 0:
            return "unknown"
        variance_ratio = metric.std_dev_ns / metric.mean_ns
        if variance_ratio >= 0.2:
            return "high-variance"
        if variance_ratio >= 0.1:
            return "watch"
        return "stable"


def render_cli(payload: object) -> str:
    rows = payload if isinstance(payload, list) else []
    lines = ["Slowspots"]
    for index, item in enumerate(rows[:10], start=1):
        lines.append(
            f"{index:>2}. {item['name']} | family={item.get('workload_family', 'unmapped')} | mean={item['mean_ns'] / 1_000_000.0:.2f}ms | score={item['score']:.2f} | {item['signals']}"
        )
    if not rows:
        lines.append("No slowspots found.")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Emit Criterion performance slowspot metrics as JSON"
    )
    parser.add_argument(
        "--threshold", type=float, default=50.0, help="Default latency threshold in ms"
    )
    parser.add_argument(
        "--skip-bench",
        action="store_true",
        help="Skip running benchmarks and load existing Criterion results",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help=f"Optional output JSON path. Example: {DEFAULT_OUTPUT}",
    )
    parser.add_argument(
        "--fail-on-slow",
        action="store_true",
        help="Exit with a non-zero status when any benchmark exceeds its threshold",
    )
    add_mode_argument(parser)

    args = parser.parse_args()
    analyzer = SlowspotAnalyzer(threshold_ms=args.threshold)
    results = analyzer.run_benchmarks(skip_bench=args.skip_bench)
    payload = [asdict(metric) for metric in results]
    emit_report(
        payload,
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=render_cli,
        label="slowspot",
    )
    if args.fail_on_slow and any(metric.mean_ms > metric.threshold_ms for metric in results):
        sys.exit(1)


if __name__ == "__main__":
    main()

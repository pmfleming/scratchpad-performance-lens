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

from perf_report_shared import load_benchmark_metadata, matching_flamegraph_ids
from report_modes import add_mode_argument, emit_report

BENCH_CMD = ["cargo", "bench", "--bench", "search_speed"]
METADATA_PATH = Path("benches/search_benchmark_targets.json")
DEFAULT_OUTPUT = Path("search_speed.json")
VISIBILITY_OUTPUT = Path("target/analysis/search_speed.json")


@dataclass
class SearchSpeedMetric:
    name: str
    benchmark_key: str
    scenario_label: str
    description: str
    mode: str
    latency_kind: str
    scaling_axis: str
    benchmark_kind: str
    workload_family: str
    parameter_value: Optional[int]
    parameter_unit: str
    parameter_label: str
    query: str
    targets: List[str]
    threshold_ms: float
    fixed_item_count: Optional[int]
    item_count: Optional[int]
    bytes_per_item: Optional[int]
    total_bytes: Optional[int]
    response_match_limit: Optional[int]
    mean_ns: float
    std_dev_ns: float
    median_ns: float
    max_ns: float
    min_ns: float
    throughput_mb_s: Optional[float]
    ns_per_kb: Optional[float]
    score: float = 0.0
    signals: str = ""
    matching_flamegraphs: Optional[List[str]] = None
    has_profile_coverage: bool = False
    stability: str = "stable"
    suspected_limiting_resource: str = "cpu"
    probe_class: str = "targeted_path"
    measurement_role: str = "change_validation"
    measurement_question: str = "Did this search path stay inside its latency budget?"

    @property
    def mean_ms(self) -> float:
        return self.mean_ns / 1_000_000.0


class SearchSpeedAnalyzer:
    def __init__(self) -> None:
        self.benchmark_metadata = self.load_benchmark_metadata()

    def load_benchmark_metadata(self) -> Dict[str, Dict[str, Any]]:
        return load_benchmark_metadata(50.0)

    def run_benchmarks(self, skip_bench: bool = False) -> List[SearchSpeedMetric]:
        if not skip_bench:
            print("Running focused search benchmarks via cargo bench...", file=sys.stderr)
            try:
                self.run_bench_command(BENCH_CMD)
            except subprocess.CalledProcessError as exc:
                print(f"Search benchmarking failed: {exc}", file=sys.stderr)
                if exc.output:
                    print(exc.output, file=sys.stderr)
                print("Falling back to existing Criterion search results.", file=sys.stderr)

        return self.load_criterion_results(self.criterion_results_dir())

    def criterion_results_dir(self) -> Path:
        return Path(os.environ.get("CARGO_TARGET_DIR", "target")) / "criterion"

    def run_bench_command(self, cmd: List[str]) -> None:
        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )

        status = {"current": "starting cargo bench --bench search_speed", "done": False}

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

        captured_output: List[str] = []
        try:
            assert process.stdout is not None
            for raw_line in process.stdout:
                line = raw_line.rstrip()
                captured_output.append(raw_line)
                progress = self.parse_benchmark_progress(line)
                if progress is not None:
                    status["current"] = progress

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

    def load_criterion_results(self, criterion_dir: Path) -> List[SearchSpeedMetric]:
        if not criterion_dir.exists():
            print(
                f"Search benchmark results directory not found at {criterion_dir}",
                file=sys.stderr,
            )
            return []

        results: List[SearchSpeedMetric] = []
        for root, _, files in os.walk(criterion_dir):
            if "estimates.json" not in files:
                continue

            estimates_path = Path(root) / "estimates.json"
            if estimates_path.parent.name != "new":
                continue

            benchmark_name = self.benchmark_name_from_estimate_path(
                criterion_dir, estimates_path
            )
            benchmark_key = self.benchmark_key(benchmark_name)
            metadata = self.benchmark_metadata.get(benchmark_key)
            if metadata is None:
                continue

            with estimates_path.open("r", encoding="utf-8") as handle:
                data = json.load(handle)

            parameter_value = self.parameter_value(benchmark_name)
            item_count = self.resolve_item_count(metadata, parameter_value)
            bytes_per_item = self.resolve_bytes_per_item(metadata, parameter_value)
            total_bytes = self.resolve_total_bytes(metadata, parameter_value)
            throughput_mb_s = self.throughput_mb_s(total_bytes, data)
            ns_per_kb = self.ns_per_kb(total_bytes, data)
            metric = SearchSpeedMetric(
                name=benchmark_name,
                benchmark_key=benchmark_key,
                scenario_label=self.scenario_label(benchmark_key),
                description=str(metadata.get("description", benchmark_key)),
                mode=str(metadata.get("mode", "unknown")),
                latency_kind=str(metadata.get("latency_kind", "completion")),
                scaling_axis=str(metadata.get("scaling_axis", "aggregate_size")),
                benchmark_kind=str(metadata.get("kind", "workflow")),
                workload_family=str(metadata.get("workload_family", "search")),
                parameter_value=parameter_value,
                parameter_unit=str(metadata.get("parameter_unit", "value")),
                parameter_label=self.parameter_label(metadata, parameter_value),
                query=str(metadata.get("query", "")),
                targets=list(metadata.get("targets", [])),
                threshold_ms=float(metadata.get("threshold_ms", 50.0)),
                fixed_item_count=self.optional_int(metadata.get("fixed_item_count")),
                item_count=item_count,
                bytes_per_item=bytes_per_item,
                total_bytes=total_bytes,
                response_match_limit=self.optional_int(metadata.get("response_match_limit")),
                mean_ns=float(data.get("mean", {}).get("point_estimate", 0.0)),
                std_dev_ns=float(data.get("std_dev", {}).get("point_estimate", 0.0)),
                median_ns=float(data.get("median", {}).get("point_estimate", 0.0)),
                max_ns=float(data.get("mean", {}).get("point_estimate", 0.0))
                + (2.0 * float(data.get("std_dev", {}).get("point_estimate", 0.0))),
                min_ns=max(
                    0.0,
                    float(data.get("mean", {}).get("point_estimate", 0.0))
                    - (2.0 * float(data.get("std_dev", {}).get("point_estimate", 0.0))),
                ),
                throughput_mb_s=throughput_mb_s,
                ns_per_kb=ns_per_kb,
                matching_flamegraphs=matching_flamegraph_ids(benchmark_key),
                has_profile_coverage=bool(matching_flamegraph_ids(benchmark_key)),
                suspected_limiting_resource=str(
                    metadata.get("limiting_resource_hint", "cpu")
                ),
            )
            metric.stability = self.stability_label(metric)
            metric.score = self.calculate_score(metric)
            metric.signals = self.generate_signals(metric)
            results.append(metric)

        return sorted(results, key=lambda item: (-item.score, item.name))

    def benchmark_name_from_estimate_path(
        self, criterion_dir: Path, estimates_path: Path
    ) -> str:
        relative = estimates_path.relative_to(criterion_dir)
        return "/".join(relative.parts[:-2])

    def benchmark_key(self, benchmark_name: str) -> str:
        return benchmark_name.split("/", 1)[0]

    def parameter_value(self, benchmark_name: str) -> Optional[int]:
        parts = benchmark_name.split("/", 1)
        if len(parts) < 2:
            return None
        try:
            return int(parts[1])
        except ValueError:
            return None

    def resolve_total_bytes(
        self, metadata: Dict[str, Any], parameter_value: Optional[int]
    ) -> Optional[int]:
        parameter_unit = str(metadata.get("parameter_unit", "value"))
        if parameter_unit == "bytes":
            if parameter_value is None:
                return None
            fixed_item_count = self.optional_int(metadata.get("fixed_item_count")) or 1
            return parameter_value * fixed_item_count

        bytes_per_item = metadata.get("bytes_per_item")
        if bytes_per_item is None or parameter_value is None:
            return None
        return int(bytes_per_item) * parameter_value

    def resolve_item_count(
        self, metadata: Dict[str, Any], parameter_value: Optional[int]
    ) -> Optional[int]:
        parameter_unit = str(metadata.get("parameter_unit", "value"))
        if parameter_unit == "bytes":
            return self.optional_int(metadata.get("fixed_item_count")) or 1
        return parameter_value

    def resolve_bytes_per_item(
        self, metadata: Dict[str, Any], parameter_value: Optional[int]
    ) -> Optional[int]:
        parameter_unit = str(metadata.get("parameter_unit", "value"))
        if parameter_unit == "bytes":
            return parameter_value

        bytes_per_item = metadata.get("bytes_per_item")
        if bytes_per_item is None:
            return None
        return int(bytes_per_item)

    def optional_int(self, value: Any) -> Optional[int]:
        if value is None:
            return None
        return int(value)

    def throughput_mb_s(
        self, total_bytes: Optional[int], estimates: Dict[str, Any]
    ) -> Optional[float]:
        mean_ns = float(estimates.get("mean", {}).get("point_estimate", 0.0))
        if total_bytes is None or total_bytes <= 0 or mean_ns <= 0.0:
            return None
        bytes_per_second = total_bytes * 1_000_000_000.0 / mean_ns
        return bytes_per_second / (1024.0 * 1024.0)

    def ns_per_kb(
        self, total_bytes: Optional[int], estimates: Dict[str, Any]
    ) -> Optional[float]:
        mean_ns = float(estimates.get("mean", {}).get("point_estimate", 0.0))
        if total_bytes is None or total_bytes <= 0 or mean_ns <= 0.0:
            return None
        return mean_ns / (total_bytes / 1024.0)

    def scenario_label(self, benchmark_key: str) -> str:
        return benchmark_key.replace("search_", "").replace("_", " ").title()

    def parameter_label(
        self, metadata: Dict[str, Any], parameter_value: Optional[int]
    ) -> str:
        if parameter_value is None:
            return "-"

        parameter_unit = str(metadata.get("parameter_unit", "value"))
        if parameter_unit == "bytes":
            return human_bytes(parameter_value)
        return f"{parameter_value} {parameter_unit}"

    def calculate_score(self, metric: SearchSpeedMetric) -> float:
        if metric.ns_per_kb is None or metric.mean_ns <= 0.0:
            return round(metric.mean_ms, 2)
        relative_std_dev = metric.std_dev_ns / metric.mean_ns
        return round(metric.ns_per_kb * (1.0 + relative_std_dev), 2)

    def generate_signals(self, metric: SearchSpeedMetric) -> str:
        signals = []
        if metric.mean_ms > metric.threshold_ms:
            signals.append(f"over budget > {metric.threshold_ms:.1f}ms")
        if metric.latency_kind == "first_response":
            signals.append("partial-result latency")
        else:
            signals.append("full-scan latency")
        if metric.mean_ns > 0.0 and (metric.std_dev_ns / metric.mean_ns) > 0.2:
            signals.append("high variance")
        if metric.matching_flamegraphs:
            signals.append("profile coverage")
        return ", ".join(signals)

    def stability_label(self, metric: SearchSpeedMetric) -> str:
        if metric.mean_ns <= 0.0:
            return "unknown"
        variance_ratio = metric.std_dev_ns / metric.mean_ns
        if variance_ratio >= 0.2:
            return "high-variance"
        if variance_ratio >= 0.1:
            return "watch"
        return "stable"


def human_bytes(value: int) -> str:
    if value >= 1024 * 1024:
        return f"{value / (1024 * 1024):.1f} MB"
    if value >= 1024:
        return f"{value / 1024:.0f} KB"
    return f"{value} B"


def render_cli(payload: object) -> str:
    rows = payload if isinstance(payload, list) else []
    lines = ["Search Speed"]
    for index, item in enumerate(rows[:10], start=1):
        throughput = item.get("throughput_mb_s")
        throughput_label = (
            f"{throughput:.1f} MB/s" if isinstance(throughput, (float, int)) else "-"
        )
        lines.append(
            f"{index:>2}. {item['mode']}/{item['latency_kind']}/{item['scaling_axis']} | param={item['parameter_label']} | mean={item['mean_ns'] / 1_000_000.0:.2f}ms | throughput={throughput_label} | {item['signals']}"
        )
    if not rows:
        lines.append("No search-speed benchmarks found.")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Emit search performance metrics from the dedicated Criterion bench"
    )
    parser.add_argument(
        "--skip-bench",
        action="store_true",
        help="Skip running cargo bench and load existing Criterion results",
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
        help="Exit with a non-zero status when any search benchmark exceeds its threshold",
    )
    add_mode_argument(parser)

    args = parser.parse_args()
    analyzer = SearchSpeedAnalyzer()
    results = analyzer.run_benchmarks(skip_bench=args.skip_bench)
    payload = [asdict(metric) for metric in results]
    emit_report(
        payload,
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=render_cli,
        label="search-speed",
    )

    if args.fail_on_slow and any(metric.mean_ms > metric.threshold_ms for metric in results):
        sys.exit(1)


if __name__ == "__main__":
    main()

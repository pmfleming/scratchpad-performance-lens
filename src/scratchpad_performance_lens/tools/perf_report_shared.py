import json
import os
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

BENCHMARK_METADATA_PATHS = (
    Path("benches/benchmark_targets.json"),
    Path("benches/search_benchmark_targets.json"),
)

BUILTIN_BENCHMARK_METADATA: Dict[str, Dict[str, Any]] = {
    "file_load": {
        "targets": ["src/app/services/file_service.rs", "src/app/domain/buffer/document.rs"],
        "kind": "workflow",
        "threshold_ms": 160.0,
        "family": "file-load",
        "limiting_resource_hint": "memory",
        "description": "Realistic file load path through file service and document creation.",
    },
    "file_open_latency": {
        "targets": ["src/app/services/file_service.rs", "src/app/services/file_controller"],
        "kind": "workflow",
        "threshold_ms": 160.0,
        "family": "file-load",
        "limiting_resource_hint": "memory",
        "description": "File-open latency, including decode and metadata work.",
    },
    "scroll_stress_latency": {
        "targets": ["src/app/ui/editor_area", "src/app/ui/editor_content/native_editor"],
        "kind": "workflow",
        "threshold_ms": 16.7,
        "family": "scroll",
        "limiting_resource_hint": "cpu",
        "description": "Repeated visible-window layout and redraw cost while scrolling.",
    },
    "ui_render_frame": {
        "targets": ["src/app/ui"],
        "kind": "workflow",
        "threshold_ms": 16.7,
        "family": "scroll",
        "limiting_resource_hint": "cpu",
        "description": "Frame render latency.",
    },
    "ui_render_frame_120hz": {
        "targets": ["src/app/app_state/frame.rs", "src/app/ui"],
        "kind": "workflow",
        "threshold_ms": 8.33,
        "family": "scroll",
        "limiting_resource_hint": "cpu",
        "description": "Headless editor frame latency against the 120 Hz frame budget.",
    },
    "editor_scroll_frame_120hz": {
        "targets": ["src/app/ui/editor_area", "src/app/ui/scrolling", "src/app/ui/editor_content"],
        "kind": "workflow",
        "threshold_ms": 8.33,
        "family": "scroll",
        "limiting_resource_hint": "cpu",
        "description": "Large-buffer editor frame latency against the 120 Hz frame budget.",
    },
    "document_snapshot_creation_latency": {
        "targets": ["src/app/domain/buffer/document.rs"],
        "kind": "workflow",
        "threshold_ms": 40.0,
        "family": "snapshot",
        "limiting_resource_hint": "memory",
        "description": "Revisioned document snapshot creation latency.",
    },
    "viewport_extraction_latency": {
        "targets": ["src/app/domain/view", "src/app/ui/editor_content/native_editor"],
        "kind": "workflow",
        "threshold_ms": 16.7,
        "family": "viewport",
        "limiting_resource_hint": "cpu",
        "description": "Visible-range and overscanned viewport extraction latency.",
    },
    "paste_stress_latency": {
        "targets": ["src/app/domain/buffer/document.rs", "src/app/app_state/workspace"],
        "kind": "workflow",
        "threshold_ms": 120.0,
        "family": "edit-paste",
        "limiting_resource_hint": "memory",
        "description": "Large paste mutation, metadata refresh, and undo-state update latency.",
    },
    "split_stress_latency": {
        "targets": ["src/app/domain/tab", "src/app/ui/editor_area"],
        "kind": "workflow",
        "threshold_ms": 80.0,
        "family": "split-layout",
        "limiting_resource_hint": "cpu",
        "description": "Repeated split, rebalance, close, and tile layout latency.",
    },
    "tile_count_scale": {
        "targets": ["src/app/domain/tab", "src/app/ui/editor_area"],
        "kind": "scale",
        "threshold_ms": 80.0,
        "family": "split-layout",
        "limiting_resource_hint": "cpu",
        "description": "Tile and view-count layout scaling.",
    },
    "tab_stress_operations": {
        "targets": ["src/app/domain/tab", "src/app/ui/tab_strip"],
        "kind": "workflow",
        "threshold_ms": 80.0,
        "family": "tab-management",
        "limiting_resource_hint": "cpu",
        "description": "Tab activation, reorder, and movement latency.",
    },
    "tab_count_scale": {
        "targets": ["src/app/domain/tab", "src/app/ui/tab_strip"],
        "kind": "scale",
        "threshold_ms": 140.0,
        "family": "tab-management",
        "limiting_resource_hint": "memory",
        "description": "Tab-count scaling and live tab-object cost.",
    },
    "control_char_load": {
        "targets": ["src/app/services/file_service.rs", "src/app/color_contrast.rs"],
        "kind": "workflow",
        "threshold_ms": 80.0,
        "family": "control-char-encoding",
        "limiting_resource_hint": "cpu",
        "description": "Load path with control-character and encoding inspection.",
    },
    "control_char_visible": {
        "targets": ["src/app/ui/editor_content/native_editor/painting.rs"],
        "kind": "workflow",
        "threshold_ms": 16.7,
        "family": "control-char-encoding",
        "limiting_resource_hint": "cpu",
        "description": "Visible control-character rendering cost.",
    },
    "control_char_clean": {
        "targets": ["src/app/ui/editor_content/native_editor/painting.rs"],
        "kind": "workflow",
        "threshold_ms": 16.7,
        "family": "control-char-encoding",
        "limiting_resource_hint": "cpu",
        "description": "Clean text rendering baseline.",
    },
    "piece_tree_anchor_remove": {
        "targets": ["src/app/domain/buffer/piece_tree"],
        "kind": "micro",
        "threshold_ms": 10.0,
        "family": "anchor-maintenance",
        "limiting_resource_hint": "cpu",
        "description": "Piece-tree anchor removal and maintenance cost.",
    },
    "buffer_search_regex": {
        "targets": ["src/app/services/search"],
        "kind": "micro",
        "threshold_ms": 50.0,
        "family": "search",
        "limiting_resource_hint": "cpu",
        "description": "Regex search over a single buffer.",
    },
    "search_active_completion_file_size": {
        "targets": ["src/app/services/search", "src/app/app_state/search_state"],
        "kind": "workflow",
        "threshold_ms": 100.0,
        "family": "search",
        "mode": "active",
        "latency_kind": "completion",
        "scaling_axis": "file_size",
        "parameter_unit": "bytes",
        "description": "Active-file search completion while file size grows.",
    },
    "search_active_first_response_file_size": {
        "targets": ["src/app/services/search", "src/app/app_state/search_state"],
        "kind": "workflow",
        "threshold_ms": 25.0,
        "family": "search",
        "mode": "active",
        "latency_kind": "first_response",
        "scaling_axis": "file_size",
        "parameter_unit": "bytes",
        "description": "Active-file first search response while file size grows.",
    },
    "search_current_completion_file_size": {
        "targets": ["src/app/services/search", "src/app/app_state/search_state"],
        "kind": "workflow",
        "threshold_ms": 100.0,
        "family": "search",
        "mode": "current",
        "latency_kind": "completion",
        "scaling_axis": "file_size",
        "parameter_unit": "bytes",
        "description": "Current-tab search completion while file size grows.",
    },
    "search_current_first_response_file_size": {
        "targets": ["src/app/services/search", "src/app/app_state/search_state"],
        "kind": "workflow",
        "threshold_ms": 25.0,
        "family": "search",
        "mode": "current",
        "latency_kind": "first_response",
        "scaling_axis": "file_size",
        "parameter_unit": "bytes",
        "description": "Current-tab first search response while file size grows.",
    },
    "search_all_completion_file_size": {
        "targets": ["src/app/services/search", "src/app/domain/tab"],
        "kind": "workflow",
        "threshold_ms": 140.0,
        "family": "search",
        "mode": "all",
        "latency_kind": "completion",
        "scaling_axis": "file_size",
        "parameter_unit": "bytes",
        "description": "All-open-tabs search completion while file size grows.",
    },
    "search_all_first_response_file_size": {
        "targets": ["src/app/services/search", "src/app/domain/tab"],
        "kind": "workflow",
        "threshold_ms": 35.0,
        "family": "search",
        "mode": "all",
        "latency_kind": "first_response",
        "scaling_axis": "file_size",
        "parameter_unit": "bytes",
        "description": "All-open-tabs first search response while file size grows.",
    },
    "search_current_completion_aggregate_size": {
        "targets": ["src/app/services/search", "src/app/app_state/search_state"],
        "kind": "workflow",
        "threshold_ms": 140.0,
        "family": "search",
        "mode": "current",
        "latency_kind": "completion",
        "scaling_axis": "aggregate_size",
        "parameter_unit": "files",
        "bytes_per_item": 65536,
        "description": "Current-tab search completion while file count and corpus size grow.",
    },
    "search_current_first_response_aggregate_size": {
        "targets": ["src/app/services/search", "src/app/app_state/search_state"],
        "kind": "workflow",
        "threshold_ms": 35.0,
        "family": "search",
        "mode": "current",
        "latency_kind": "first_response",
        "scaling_axis": "aggregate_size",
        "parameter_unit": "files",
        "bytes_per_item": 65536,
        "description": "Current-tab first search response while file count and corpus size grow.",
    },
    "search_current_app_state_completion_aggregate_size": {
        "targets": ["src/app/services/search", "src/app/app_state/search_state"],
        "kind": "workflow",
        "threshold_ms": 140.0,
        "family": "search",
        "mode": "current",
        "latency_kind": "completion",
        "scaling_axis": "aggregate_size",
        "parameter_unit": "files",
        "bytes_per_item": 65536,
        "description": "Current app-state search completion while corpus size grows.",
    },
    "search_all_completion_aggregate_size": {
        "targets": ["src/app/services/search", "src/app/domain/tab"],
        "kind": "workflow",
        "threshold_ms": 180.0,
        "family": "search",
        "mode": "all",
        "latency_kind": "completion",
        "scaling_axis": "aggregate_size",
        "parameter_unit": "files",
        "bytes_per_item": 65536,
        "description": "All-tabs search completion while file count and corpus size grow.",
    },
    "search_all_first_response_aggregate_size": {
        "targets": ["src/app/services/search", "src/app/domain/tab"],
        "kind": "workflow",
        "threshold_ms": 45.0,
        "family": "search",
        "mode": "all",
        "latency_kind": "first_response",
        "scaling_axis": "aggregate_size",
        "parameter_unit": "files",
        "bytes_per_item": 65536,
        "description": "All-tabs first search response while file count and corpus size grow.",
    },
    "search_current_dispatch_aggregate_size": {
        "targets": ["src/app/app_state/search_state", "src/app/services/search"],
        "kind": "workflow",
        "threshold_ms": 50.0,
        "family": "search-dispatch",
        "mode": "current",
        "latency_kind": "dispatch",
        "scaling_axis": "aggregate_size",
        "parameter_unit": "files",
        "bytes_per_item": 65536,
        "description": "Current-scope search dispatch and target collection overhead.",
    },
    "search_all_dispatch_aggregate_size": {
        "targets": ["src/app/app_state/search_state", "src/app/domain/tab"],
        "kind": "workflow",
        "threshold_ms": 50.0,
        "family": "search-dispatch",
        "mode": "all",
        "latency_kind": "dispatch",
        "scaling_axis": "aggregate_size",
        "parameter_unit": "files",
        "bytes_per_item": 65536,
        "description": "All-tabs search dispatch and target collection overhead.",
    },
    "search_capacity": {
        "targets": ["src/app/services/search"],
        "kind": "capacity",
        "threshold_ms": 140.0,
        "family": "search",
        "limiting_resource_hint": "cpu",
        "description": "Standalone search capacity sweep over very large text.",
    },
}

FLAMEGRAPH_CONFIGS = [
    {
        "id": "tab_operations_profile",
        "name": "Tab Operations Profile",
        "cargo_args": ["--bin", "profile_tab_operations"],
        "benchmark_keys": ["tab_stress_operations", "tab_count_scale"],
        "workload_families": ["tab-management"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Tab activation, reorder, and multi-tab movement hot path.",
    },
    {
        "id": "tab_tile_layout_profile",
        "name": "Tab Tile Layout Profile",
        "cargo_args": ["--bin", "profile_tab_tile_layout"],
        "benchmark_keys": ["tile_count_scale"],
        "workload_families": ["split-layout"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Split resize, rebalance, and tile layout hot path.",
    },
    {
        "id": "view_navigation_profile",
        "name": "View Navigation Profile",
        "cargo_args": ["--bin", "profile_view_navigation"],
        "benchmark_keys": [],
        "workload_families": ["exploratory"],
        "coverage_role": "exploratory",
        "resource_focus": "cpu",
        "description": "Exploratory editor-view navigation profile without a dedicated broad benchmark family.",
    },
    {
        "id": "search_current_app_state_profile",
        "name": "Search Current App-State Profile",
        "cargo_args": ["--bin", "profile_search_current_app_state"],
        "benchmark_keys": [
            "search_active_completion_file_size",
            "search_active_first_response_file_size",
            "search_current_completion_file_size",
            "search_current_completion_aggregate_size",
            "search_current_app_state_completion_aggregate_size",
        ],
        "workload_families": ["search"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Active-file and current-tab search hot path through the Scratchpad search pipeline.",
    },
    {
        "id": "search_all_tabs_profile",
        "name": "Search All Tabs Profile",
        "cargo_args": ["--bin", "profile_search_all_tabs"],
        "benchmark_keys": [
            "search_all_completion_file_size",
            "search_all_completion_aggregate_size",
        ],
        "workload_families": ["search"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "All-open-tabs search hot path across the global workspace tab manager.",
    },
    {
        "id": "search_dispatch_profile",
        "name": "Search Dispatch Profile",
        "cargo_args": ["--bin", "profile_search_dispatch"],
        "benchmark_keys": [
            "search_current_dispatch_aggregate_size",
            "search_all_dispatch_aggregate_size",
        ],
        "workload_families": ["search-dispatch"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Search request building and target collection cost before worker-side scanning begins.",
    },
    {
        "id": "document_snapshot_profile",
        "name": "Document Snapshot Profile",
        "cargo_args": ["--bin", "profile_document_snapshot"],
        "benchmark_keys": ["document_snapshot_creation_latency"],
        "workload_families": ["snapshot"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Revisioned snapshot creation cost for large piece-tree-backed documents.",
    },
    {
        "id": "viewport_extraction_profile",
        "name": "Viewport Extraction Profile",
        "cargo_args": ["--bin", "profile_viewport_extraction"],
        "benchmark_keys": ["viewport_extraction_latency"],
        "workload_families": ["viewport"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Piece-tree visible-range and overscanned viewport extraction cost for expanded buffers.",
    },
    {
        "id": "ui_render_frame_profile",
        "name": "UI Render Frame Profile",
        "cargo_args": ["--bin", "profile_ui_render_frame"],
        "benchmark_keys": ["ui_render_frame_120hz", "editor_scroll_frame_120hz", "ui_render_frame"],
        "workload_families": ["scroll"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Headless Scratchpad frame loop profile with phase timing instrumentation.",
    },
    {
        "id": "scroll_stress_profile",
        "name": "Scroll Stress Profile",
        "cargo_args": ["--bin", "profile_scroll_stress"],
        "benchmark_keys": ["scroll_stress_latency"],
        "workload_families": ["scroll"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Headless editor layout and repaint work representative of repeated scroll redraw.",
    },
    {
        "id": "paste_stress_profile",
        "name": "Paste Stress Profile",
        "cargo_args": ["--bin", "profile_paste_stress"],
        "benchmark_keys": ["paste_stress_latency"],
        "workload_families": ["edit-paste"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Large insert into an expanded buffer, including metadata refresh and undo state updates.",
    },
    {
        "id": "split_stress_profile",
        "name": "Split Stress Profile",
        "cargo_args": ["--bin", "profile_split_stress"],
        "benchmark_keys": ["split_stress_latency"],
        "workload_families": ["split-layout"],
        "coverage_role": "report-driven",
        "resource_focus": "cpu",
        "description": "Repeated splitting and rebalance work on expanded document tiles.",
    },
    {
        "id": "search_capacity_profile",
        "name": "Search Capacity Profile",
        "cargo_args": ["--bin", "profile_search_capacity"],
        "benchmark_keys": ["search_capacity"],
        "workload_families": ["search"],
        "coverage_role": "exploratory",
        "resource_focus": "cpu",
        "description": "Standalone large-text search profile for the upper end of file-size scaling.",
    },
]


def benchmark_key_from_name(benchmark_name: str) -> str:
    return benchmark_name.split("/", 1)[0]


def flamegraph_configs() -> List[Dict[str, Any]]:
    return [dict(config) for config in FLAMEGRAPH_CONFIGS]


def matching_flamegraph_ids(benchmark_key: str) -> List[str]:
    matches = []
    for config in FLAMEGRAPH_CONFIGS:
        if benchmark_key in config.get("benchmark_keys", []):
            matches.append(str(config["id"]))
    return matches


def terminate_process_tree(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"],
            capture_output=True,
            text=True,
        )
        return
    process.kill()


MB = 1024 * 1024
GB = 1024 * MB
EMPTY_PROCESS_SAMPLE: Dict[str, Optional[int]] = {
    "working_set_bytes": None,
    "peak_working_set_bytes": None,
    "page_fault_count": None,
    "handle_count": None,
}


def human_bytes(value: Optional[int]) -> str:
    if value is None:
        return "-"
    if value >= GB:
        return f"{value / GB:.1f} GB"
    if value >= MB:
        return f"{value / MB:.1f} MB"
    if value >= 1024:
        return f"{value / 1024:.0f} KB"
    return f"{value} B"


def workload_label(value: int, unit: str) -> str:
    return human_bytes(value) if unit == "bytes" else f"{value} {unit}"


def safe_delta(last: Optional[int], first: Optional[int]) -> Optional[int]:
    if last is None or first is None:
        return None
    return last - first


def run_fallback_workload(value: int, unit: str) -> int:
    if unit == "bytes":
        sample = bytearray(min(value, 4 * MB))
        return sample[0] if sample else 0
    return sum(index & 1 for index in range(min(value, 10_000)))


def sample_process(pid: int) -> Dict[str, Optional[int]]:
    if os.name == "nt":
        return sample_windows_process(pid)
    return sample_posix_process()


def sample_windows_process(pid: int) -> Dict[str, Optional[int]]:
    import ctypes
    from ctypes import wintypes

    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    PROCESS_VM_READ = 0x0010

    class PROCESS_MEMORY_COUNTERS_EX(ctypes.Structure):
        _fields_ = [
            ("cb", wintypes.DWORD),
            ("PageFaultCount", wintypes.DWORD),
            ("PeakWorkingSetSize", ctypes.c_size_t),
            ("WorkingSetSize", ctypes.c_size_t),
            ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPagedPoolUsage", ctypes.c_size_t),
            ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
            ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
            ("PagefileUsage", ctypes.c_size_t),
            ("PeakPagefileUsage", ctypes.c_size_t),
            ("PrivateUsage", ctypes.c_size_t),
        ]

    kernel32 = ctypes.windll.kernel32
    process = kernel32.OpenProcess(
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
        False,
        pid,
    )
    if not process:
        return dict(EMPTY_PROCESS_SAMPLE)

    try:
        counters = PROCESS_MEMORY_COUNTERS_EX()
        counters.cb = ctypes.sizeof(PROCESS_MEMORY_COUNTERS_EX)
        if not ctypes.windll.psapi.GetProcessMemoryInfo(
            process,
            ctypes.byref(counters),
            counters.cb,
        ):
            return dict(EMPTY_PROCESS_SAMPLE)

        handle_count = wintypes.DWORD()
        kernel32.GetProcessHandleCount(process, ctypes.byref(handle_count))
        return {
            "working_set_bytes": int(counters.WorkingSetSize),
            "peak_working_set_bytes": int(counters.PeakWorkingSetSize),
            "page_fault_count": int(counters.PageFaultCount),
            "handle_count": int(handle_count.value),
        }
    finally:
        kernel32.CloseHandle(process)


def sample_posix_process() -> Dict[str, Optional[int]]:
    try:
        import resource

        usage = resource.getrusage(resource.RUSAGE_CHILDREN)
        rss = int(usage.ru_maxrss)
        if sys.platform != "darwin":
            rss *= 1024
        return {
            "working_set_bytes": rss,
            "peak_working_set_bytes": rss,
            "page_fault_count": None,
            "handle_count": None,
        }
    except Exception:
        return dict(EMPTY_PROCESS_SAMPLE)


def run_json_probe(
    *,
    build_cmd: List[str],
    probe_path: Path,
    timeout_seconds: int,
    label: str,
) -> Tuple[List[Dict[str, Any]], str]:
    subprocess.run(build_cmd, check=True, capture_output=True, text=True)
    process = subprocess.Popen(
        [str(probe_path)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    samples: List[Dict[str, Any]] = []

    def read_stdout() -> None:
        assert process.stdout is not None
        for raw_line in process.stdout:
            line = raw_line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            event.update(sample_process(process.pid))
            samples.append(event)

    reader = threading.Thread(target=read_stdout, daemon=True)
    reader.start()
    probe_status = "completed"
    try:
        return_code = process.wait(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        probe_status = "timed_out"
        terminate_process_tree(process)
        return_code = 124
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            pass
    reader.join(timeout=5)

    stderr = process.stderr.read().strip() if process.stderr is not None else ""
    if return_code != 0 and probe_status != "timed_out":
        raise RuntimeError(
            f"{label} failed with exit code {return_code}: {stderr or 'no stderr'}"
        )
    return samples, probe_status


def normalize_metadata_entry(
    value: Dict[str, Any],
    default_threshold: float,
    *,
    default_kind: str,
    default_family: str,
) -> Dict[str, Any]:
    normalized = {
        "targets": list(value.get("targets", [])),
        "kind": value.get("kind", default_kind),
        "threshold_ms": float(value.get("threshold_ms", default_threshold)),
        "workload_family": value.get("family", value.get("workload_family", default_family)),
        "limiting_resource_hint": value.get("limiting_resource_hint", "cpu"),
    }
    for extra_key, extra_value in value.items():
        if extra_key in {"family", "workload_family"}:
            continue
        normalized[extra_key] = extra_value
    return normalized


def load_benchmark_metadata(default_threshold: float = 50.0) -> Dict[str, Dict[str, Any]]:
    metadata: Dict[str, Dict[str, Any]] = {
        key: normalize_metadata_entry(
            value,
            default_threshold,
            default_kind="workflow",
            default_family="unmapped",
        )
        for key, value in BUILTIN_BENCHMARK_METADATA.items()
    }

    for path in BENCHMARK_METADATA_PATHS:
        if not path.exists():
            continue

        with path.open("r", encoding="utf-8") as handle:
            data = json.load(handle)

        is_search_metadata = path.name == "search_benchmark_targets.json"
        for key, value in data.items():
            metadata[key] = normalize_metadata_entry(
                value,
                default_threshold,
                default_kind="workflow" if is_search_metadata else "unmapped",
                default_family="search" if is_search_metadata else "unmapped",
            )

    return metadata


def metadata_for_benchmark(
    benchmark_name: str,
    metadata: Dict[str, Dict[str, Any]],
    default_threshold: float,
) -> Dict[str, Any]:
    benchmark_key = benchmark_key_from_name(benchmark_name)
    return metadata.get(
        benchmark_key,
        {
            "targets": [],
            "kind": "unmapped",
            "threshold_ms": default_threshold,
            "workload_family": "unmapped",
            "limiting_resource_hint": "cpu",
        },
    )

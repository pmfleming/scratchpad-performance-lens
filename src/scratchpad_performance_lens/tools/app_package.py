import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Dict, List

SESSION_DIR_NAME = "scratchpad"
SESSION_MANIFEST_NAME = "session.json"
SESSION_ERROR_LOG_NAME = "error.log"
SESSION_BUFFER_EXTENSION = ".tmp"
SESSION_CLEAR_VERIFY_DELAY_SECONDS = 1.5


def app_session_root() -> Path:
    return Path(tempfile.gettempdir()) / SESSION_DIR_NAME


def file_info(path: Path) -> Dict[str, Any]:
    try:
        stat = path.stat()
    except OSError as exc:
        return {
            "exists": False,
            "path": str(path),
            "name": path.name,
            "error": str(exc),
        }
    return {
        "exists": True,
        "path": str(path),
        "name": path.name,
        "size_bytes": stat.st_size,
        "modified_at": stat.st_mtime,
    }


def load_json_file(path: Path, warnings: List[str]) -> Any | None:
    if not path.exists():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        warnings.append(f"Could not read {path.name}: {exc}")
        return None


def load_ndjson_file(path: Path, warnings: List[str]) -> List[Dict[str, Any]]:
    if not path.exists():
        return []
    diagnostics: List[Dict[str, Any]] = []
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as exc:
        warnings.append(f"Could not read {path.name}: {exc}")
        return []
    for index, line in enumerate(lines, start=1):
        if not line.strip():
            continue
        try:
            payload = json.loads(line)
        except json.JSONDecodeError as exc:
            warnings.append(f"Skipped malformed {path.name} line {index}: {exc}")
            continue
        if isinstance(payload, dict):
            payload.setdefault("line", index)
            diagnostics.append(payload)
        else:
            warnings.append(f"Skipped non-object {path.name} line {index}")
    return diagnostics


def flatten_session_buffers(manifest: Dict[str, Any] | None, root: Path) -> List[Dict[str, Any]]:
    if not isinstance(manifest, dict):
        return []
    buffers: List[Dict[str, Any]] = []
    for tab_index, tab in enumerate(manifest.get("tabs") or []):
        if not isinstance(tab, dict):
            continue
        tab_buffers = tab.get("buffers") or []
        if not tab_buffers and tab.get("temp_id"):
            tab_buffers = [{
                "id": tab.get("buffer_id"),
                "name": tab.get("name"),
                "path": tab.get("path"),
                "is_dirty": tab.get("is_dirty"),
                "temp_id": tab.get("temp_id"),
                "encoding": tab.get("encoding"),
                "has_bom": tab.get("has_bom"),
            }]
        for buffer_index, buffer in enumerate(tab_buffers):
            if isinstance(buffer, dict):
                buffers.append(session_buffer_row(tab, buffer, tab_index, buffer_index, root))
    return buffers


def session_buffer_row(
    tab: Dict[str, Any],
    buffer: Dict[str, Any],
    tab_index: int,
    buffer_index: int,
    root: Path,
) -> Dict[str, Any]:
    temp_id = buffer.get("temp_id")
    snapshot_path = root / f"{temp_id}{SESSION_BUFFER_EXTENSION}" if temp_id else None
    return {
        "tab_index": tab_index,
        "buffer_index": buffer_index,
        "id": buffer.get("id"),
        "name": buffer.get("name") or tab.get("name") or "Untitled",
        "path": buffer.get("path") or tab.get("path"),
        "is_dirty": bool(buffer.get("is_dirty")),
        "is_settings_file": bool(buffer.get("is_settings_file")),
        "temp_id": temp_id,
        "encoding": buffer.get("encoding") or (buffer.get("format") or {}).get("encoding_name"),
        "has_bom": buffer.get("has_bom") or (buffer.get("format") or {}).get("has_bom"),
        "disk_modified_millis": buffer.get("disk_modified_millis"),
        "disk_len": buffer.get("disk_len"),
        "text_history_count": len(buffer.get("text_history") or []),
        "snapshot": file_info(snapshot_path) if snapshot_path else {"exists": False},
    }


def session_topology_summary(manifest: Dict[str, Any] | None) -> List[Dict[str, Any]]:
    if not isinstance(manifest, dict):
        return []
    topology: List[Dict[str, Any]] = []
    for tab_index, tab in enumerate(manifest.get("tabs") or []):
        if not isinstance(tab, dict):
            continue
        views = tab.get("views") or []
        topology.append({
            "tab_index": tab_index,
            "active_view_id": tab.get("active_view_id"),
            "view_count": len(views),
            "view_ids": [view.get("id") for view in views if isinstance(view, dict)],
            "root_pane_kind": root_pane_kind(tab.get("root_pane")),
        })
    return topology


def root_pane_kind(node: Any) -> str:
    if not isinstance(node, dict):
        return "unknown"
    if "Leaf" in node or "leaf" in node or "view_id" in node:
        return "leaf"
    if "Split" in node or "split" in node or "axis" in node:
        return "split"
    return next(iter(node.keys()), "unknown")


def app_process_running() -> bool:
    try:
        if sys.platform == "win32":
            output = subprocess.run(
                ["tasklist", "/FI", "IMAGENAME eq scratchpad.exe", "/FO", "CSV", "/NH"],
                check=False,
                capture_output=True,
                text=True,
                timeout=5,
            ).stdout
            return "scratchpad.exe" in output.lower()
        result = subprocess.run(
            ["pgrep", "-x", "scratchpad"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
        return result.returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


def clear_app_package_buffers() -> Dict[str, Any]:
    root = app_session_root()
    manifest_path = root / SESSION_MANIFEST_NAME
    warnings: List[str] = []
    if app_process_running():
        payload = app_package_payload()
        payload["clear_result"] = {
            "blocked": True,
            "message": "Scratchpad appears to be running. Close the app before clearing persisted buffers.",
            "warnings": [],
        }
        return payload

    manifest = load_json_file(manifest_path, warnings)
    buffers = flatten_session_buffers(manifest, root)
    buffers_removed = len(buffers)
    dirty_buffers_removed = sum(1 for buffer in buffers if buffer.get("is_dirty"))
    tabs_removed = len(manifest.get("tabs") or []) if isinstance(manifest, dict) else 0
    snapshot_files_removed = 0

    if isinstance(manifest, dict):
        manifest["tabs"] = []
        manifest["active_tab_index"] = 0
        try:
            manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        except OSError as exc:
            warnings.append(f"Could not write {manifest_path.name}: {exc}")
    elif root.exists():
        warnings.append("No readable session manifest; removed snapshot files only.")

    if root.exists():
        for path in sorted(root.glob(f"*{SESSION_BUFFER_EXTENSION}")):
            try:
                path.unlink()
                snapshot_files_removed += 1
            except OSError as exc:
                warnings.append(f"Could not remove {path.name}: {exc}")

    time.sleep(SESSION_CLEAR_VERIFY_DELAY_SECONDS)
    payload = app_package_payload()
    current_summary = payload.get("manifest_summary") or {}
    if current_summary.get("tab_count") or current_summary.get("buffer_count"):
        payload["clear_result"] = {
            "blocked": True,
            "message": "Scratchpad rewrote the session after clearing. Close the app and run Clear buffers again.",
            "tabs_removed": tabs_removed,
            "buffers_removed": buffers_removed,
            "dirty_buffers_removed": dirty_buffers_removed,
            "snapshot_files_removed": snapshot_files_removed,
            "warnings": warnings,
        }
        return payload

    payload["clear_result"] = {
        "tabs_removed": tabs_removed,
        "buffers_removed": buffers_removed,
        "dirty_buffers_removed": dirty_buffers_removed,
        "snapshot_files_removed": snapshot_files_removed,
        "warnings": warnings,
    }
    return payload


def app_package_payload() -> Dict[str, Any]:
    root = app_session_root()
    manifest_path = root / SESSION_MANIFEST_NAME
    error_log_path = root / SESSION_ERROR_LOG_NAME
    warnings: List[str] = []
    manifest = load_json_file(manifest_path, warnings)
    diagnostics = load_ndjson_file(error_log_path, warnings)
    buffers = flatten_session_buffers(manifest, root)
    buffer_files = [
        file_info(path) for path in sorted(root.glob(f"*{SESSION_BUFFER_EXTENSION}"))
    ] if root.exists() else []
    tabs = manifest.get("tabs") if isinstance(manifest, dict) else []
    view_count = sum(len(tab.get("views") or []) for tab in tabs if isinstance(tab, dict))
    return {
        "exists": root.exists(),
        "session_root": str(root),
        "manifest_path": str(manifest_path),
        "error_log_path": str(error_log_path),
        "manifest_file": file_info(manifest_path),
        "error_log_file": file_info(error_log_path),
        "manifest": manifest,
        "manifest_summary": {
            "version": manifest.get("version") if isinstance(manifest, dict) else None,
            "active_tab_index": manifest.get("active_tab_index") if isinstance(manifest, dict) else None,
            "tab_count": len(tabs) if isinstance(tabs, list) else 0,
            "buffer_count": len(buffers),
            "view_count": view_count,
            "dirty_buffer_count": sum(1 for buffer in buffers if buffer.get("is_dirty")),
            "snapshot_file_count": len(buffer_files),
            "missing_snapshot_count": sum(
                1 for buffer in buffers if not buffer.get("snapshot", {}).get("exists")
            ),
            "diagnostic_count": len(diagnostics),
        },
        "buffers": buffers,
        "buffer_files": buffer_files,
        "topology": session_topology_summary(manifest),
        "diagnostics": diagnostics[-500:],
        "warnings": warnings,
    }

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any
import tomllib


@dataclass(frozen=True)
class LensConfig:
    project_name: str
    project_root: Path
    output_dir: Path

    @classmethod
    def load(cls, path: Path | None) -> "LensConfig":
        data: dict[str, Any] = {}
        config_path = path.resolve() if path is not None else None
        if config_path is not None and config_path.exists():
            data = tomllib.loads(config_path.read_text(encoding="utf-8"))

        project_root = Path(data.get("project_root") or ".").resolve()
        output_dir = Path(data.get("output_dir") or "target/analysis")
        if not output_dir.is_absolute():
            output_dir = project_root / output_dir

        return cls(
            project_name=str(data.get("project_name") or project_root.name),
            project_root=project_root,
            output_dir=output_dir,
        )

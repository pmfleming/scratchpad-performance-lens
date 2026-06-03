use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LensConfig {
    pub project_name: String,
    pub project_root: PathBuf,
    pub output_dir: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    project_name: Option<String>,
    project_root: Option<PathBuf>,
    output_dir: Option<PathBuf>,
}

impl LensConfig {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let raw = if let Some(path) = path {
            let path = path
                .canonicalize()
                .with_context(|| format!("could not resolve config path {}", path.display()))?;
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("could not read config {}", path.display()))?;
            toml::from_str::<RawConfig>(&text)
                .with_context(|| format!("could not parse config {}", path.display()))?
        } else {
            RawConfig::default()
        };

        let project_root = absolutize_from(raw.project_root.unwrap_or_else(|| PathBuf::from(".")))?;
        let output_dir = raw
            .output_dir
            .unwrap_or_else(|| PathBuf::from("target/analysis"));
        let output_dir = if output_dir.is_absolute() {
            output_dir
        } else {
            project_root.join(output_dir)
        };

        Ok(Self {
            project_name: raw.project_name.unwrap_or_else(|| {
                project_root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            }),
            project_root,
            output_dir,
        })
    }
}

fn absolutize_from(path: PathBuf) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    joined
        .canonicalize()
        .with_context(|| format!("could not resolve project root {}", joined.display()))
}

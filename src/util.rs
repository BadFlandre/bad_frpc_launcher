use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn app_base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|v| v.to_path_buf()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(crate) fn resolve_under_app(app_dir: &Path, p: &str) -> PathBuf {
    let pbuf = PathBuf::from(p);
    if pbuf.is_absolute() {
        return pbuf;
    }

    let mut candidates = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }

    candidates.push(app_dir.to_path_buf());
    if let Some(parent) = app_dir.parent() {
        candidates.push(parent.to_path_buf());
        if let Some(grand_parent) = parent.parent() {
            candidates.push(grand_parent.to_path_buf());
        }
    }

    for base in &candidates {
        let candidate = base.join(&pbuf);
        if candidate.exists() {
            return candidate;
        }
    }

    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| app_dir.to_path_buf())
        .join(pbuf)
}

pub(crate) fn path_to_string(app_dir: &Path, abs: &Path) -> String {
    if let Ok(rel) = abs.strip_prefix(app_dir) {
        let rel_str = rel.to_string_lossy().to_string();
        if rel_str.is_empty() {
            ".".to_string()
        } else {
            format!("./{rel_str}")
        }
    } else {
        abs.to_string_lossy().to_string()
    }
}

pub(crate) fn read_text_file(path: &Path) -> std::io::Result<String> {
    fs::read_to_string(path)
}

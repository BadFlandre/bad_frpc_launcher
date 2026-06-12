use crate::util::{read_text_file, resolve_under_app};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub(crate) struct AppInfo {
    pub(crate) software: SoftwareInfo,
    pub(crate) author: AuthorInfo,
}

impl Default for AppInfo {
    fn default() -> Self {
        Self {
            software: SoftwareInfo::default(),
            author: AuthorInfo::default(),
        }
    }
}

impl AppInfo {
    pub(crate) fn load(app_dir: &Path) -> Self {
        let path = resolve_under_app(app_dir, "./config/app_info.toml");
        let Ok(text) = read_text_file(&path) else {
            return Self::default();
        };

        toml::from_str(&text).unwrap_or_default()
    }

    pub(crate) fn window_title(&self) -> String {
        format!("{} {}", self.software.name, self.software.version)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub(crate) struct SoftwareInfo {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) description: String,
    pub(crate) homepage: String,
}

impl Default for SoftwareInfo {
    fn default() -> Self {
        Self {
            name: "Bad frpc Launcher".to_string(),
            version: "0.1.0".to_string(),
            description: "frpc GUI 启动器".to_string(),
            homepage: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub(crate) struct AuthorInfo {
    pub(crate) name: String,
    pub(crate) bio: String,
    pub(crate) contact: String,
}

impl Default for AuthorInfo {
    fn default() -> Self {
        Self {
            name: "Unknown".to_string(),
            bio: String::new(),
            contact: String::new(),
        }
    }
}

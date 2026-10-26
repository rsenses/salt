use crate::{open::OpenConfig, sort::SortConfig};
use serde::Deserialize;
use std::{env, fs, io, path::PathBuf};

pub struct AppConfig {
    pub open: OpenConfig,
    pub sort: SortConfig,
    pub icons: bool,
}

#[derive(Default, Deserialize)]
struct Root {
    ui: Option<UiConfig>,
}

#[derive(Deserialize)]
struct UiConfig {
    #[serde(default = "icons_default")]
    icons: bool,
}

fn icons_default() -> bool {
    true
}

fn parse_icons_setting(source: &str) -> Result<bool, String> {
    let root: Root = toml::from_str(source).map_err(|error| error.to_string())?;
    Ok(root.ui.is_none_or(|ui| ui.icons))
}

impl AppConfig {
    pub fn load() -> io::Result<Self> {
        let path = config_path()?;
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", path.display()),
                ));
            }
        };
        let parse_error = |message: String| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {message}", path.display()),
            )
        };
        Ok(Self {
            open: OpenConfig::parse(&source).map_err(parse_error)?,
            sort: SortConfig::parse(&source).map_err(parse_error)?,
            icons: parse_icons_setting(&source).map_err(parse_error)?,
        })
    }
}

fn config_path() -> io::Result<PathBuf> {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|value| PathBuf::from(value).join(".config")))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "HOME is unset; cannot locate salt.toml",
            )
        })?;
    Ok(base.join("salt").join("salt.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_enabled_unless_ui_disables_them() {
        assert!(parse_icons_setting("").unwrap());
        assert!(parse_icons_setting("[ui]\nicons = true").unwrap());
        assert!(parse_icons_setting("[ui]\n").unwrap());
        assert!(!parse_icons_setting("[ui]\nicons = false").unwrap());
        assert!(parse_icons_setting("[ui]\nicons = 'never'").is_err());
        let example = include_str!("../salt.toml.example");
        assert!(parse_icons_setting(example).unwrap());
        assert!(OpenConfig::parse(example).is_ok());
        assert!(SortConfig::parse(example).is_ok());
    }
}

use crate::{open::OpenConfig, sort::SortConfig};
use serde::Deserialize;
use std::{env, fs, io, path::PathBuf};

pub struct AppConfig {
    pub open: OpenConfig,
    pub sort: SortConfig,
    pub icons: bool,
    pub footer_position: FooterPosition,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FooterPosition {
    Top,
    #[default]
    Bottom,
}

#[derive(Default, Deserialize)]
struct Root {
    ui: Option<UiConfig>,
}

#[derive(Deserialize)]
struct UiConfig {
    #[serde(default = "icons_default")]
    icons: bool,
    #[serde(default)]
    footer_position: FooterPosition,
}

fn icons_default() -> bool {
    true
}

fn parse_ui_settings(source: &str) -> Result<(bool, FooterPosition), String> {
    let root: Root = toml::from_str(source).map_err(|error| error.to_string())?;
    Ok(root.ui.map_or((true, FooterPosition::Bottom), |ui| {
        (ui.icons, ui.footer_position)
    }))
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
        let (icons, footer_position) = parse_ui_settings(&source).map_err(parse_error)?;
        Ok(Self {
            open: OpenConfig::parse(&source).map_err(parse_error)?,
            sort: SortConfig::parse(&source).map_err(parse_error)?,
            icons,
            footer_position,
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
    fn ui_defaults_and_position_are_validated() {
        assert_eq!(
            parse_ui_settings("").unwrap(),
            (true, FooterPosition::Bottom)
        );
        assert_eq!(
            parse_ui_settings("[ui]\nicons = true").unwrap(),
            (true, FooterPosition::Bottom)
        );
        assert_eq!(
            parse_ui_settings("[ui]\nicons = false\nfooter_position = 'top'").unwrap(),
            (false, FooterPosition::Top)
        );
        assert!(parse_ui_settings("[ui]\nicons = 'never'").is_err());
        assert!(parse_ui_settings("[ui]\nfooter_position = 'left'").is_err());
        let example = include_str!("../salt.toml.example");
        assert_eq!(
            parse_ui_settings(example).unwrap(),
            (true, FooterPosition::Bottom)
        );
        assert!(OpenConfig::parse(example).is_ok());
        assert!(SortConfig::parse(example).is_ok());
    }
}

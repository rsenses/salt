use crate::{open::OpenConfig, sort::SortConfig};
use std::{env, fs, io, path::PathBuf};

pub struct AppConfig {
    pub open: OpenConfig,
    pub sort: SortConfig,
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

use serde::Deserialize;
use std::{
    env,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    #[default]
    Name,
    Mtime,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub by: SortBy,
    pub reverse: bool,
    pub directories_first: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            by: SortBy::Name,
            reverse: false,
            directories_first: true,
        }
    }
}

impl SortSpec {
    pub fn label(self) -> &'static str {
        match (self.by, self.reverse) {
            (SortBy::Name, false) => "name A–Z",
            (SortBy::Name, true) => "name Z–A",
            (SortBy::Mtime, false) => "oldest first",
            (SortBy::Mtime, true) => "newest first",
            (SortBy::Size, false) => "smallest first",
            (SortBy::Size, true) => "largest first",
        }
    }
}

#[derive(Default, Deserialize)]
struct Root {
    #[serde(default)]
    sort: RawSort,
}

#[derive(Default, Deserialize)]
struct RawSort {
    by: Option<SortBy>,
    reverse: Option<bool>,
    directories_first: Option<bool>,
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Deserialize)]
struct RawRule {
    path: String,
    by: Option<SortBy>,
    reverse: Option<bool>,
    directories_first: Option<bool>,
}

#[derive(Default)]
pub struct SortConfig {
    default: SortSpec,
    rules: Vec<(PathBuf, SortSpec)>,
}

impl SortConfig {
    pub fn parse(source: &str) -> Result<Self, String> {
        let root: Root = toml::from_str(source).map_err(|error| error.to_string())?;
        let raw = root.sort;
        let default = SortSpec {
            by: raw.by.unwrap_or_default(),
            reverse: raw.reverse.unwrap_or(false),
            directories_first: raw.directories_first.unwrap_or(true),
        };
        let mut rules = Vec::with_capacity(raw.rules.len());
        for rule in raw.rules {
            let path = expand_path(&rule.path)?;
            if rules.iter().any(|(existing, _)| existing == &path) {
                return Err(format!("Duplicate sort rule for {}", path.display()));
            }
            rules.push((
                path,
                SortSpec {
                    by: rule.by.unwrap_or(default.by),
                    reverse: rule.reverse.unwrap_or(default.reverse),
                    directories_first: rule.directories_first.unwrap_or(default.directories_first),
                },
            ));
        }
        Ok(Self { default, rules })
    }

    pub fn for_path(&self, path: &Path) -> SortSpec {
        self.rules
            .iter()
            .find(|(rule, _)| rule == path)
            .map_or(self.default, |(_, spec)| *spec)
    }
}

fn expand_path(raw: &str) -> Result<PathBuf, String> {
    let path = if raw == "~" || raw.starts_with("~/") {
        let home = env::var_os("HOME").ok_or("HOME is unset")?;
        PathBuf::from(home).join(raw.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(raw)
    };
    if !path.is_absolute() {
        return Err(format!(
            "Sort rule path must be absolute or start with ~/: {raw}"
        ));
    }
    Ok(path.canonicalize().unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_directory_rule_overrides_default() {
        let root = tempfile::tempdir().unwrap();
        let downloads = root.path().join("Downloads");
        std::fs::create_dir(&downloads).unwrap();
        let source = format!(
            r#"
            [sort]
            by = "name"
            directories_first = true

            [[sort.rules]]
            path = "{}"
            by = "mtime"
            reverse = true
            directories_first = false
        "#,
            downloads.display()
        );
        let config = SortConfig::parse(&source).unwrap();
        assert_eq!(
            config.for_path(&downloads.canonicalize().unwrap()),
            SortSpec {
                by: SortBy::Mtime,
                reverse: true,
                directories_first: false
            }
        );
        assert_eq!(config.for_path(root.path()), SortSpec::default());
    }

    #[test]
    fn rejects_relative_and_duplicate_rule_paths() {
        assert!(SortConfig::parse("[[sort.rules]]\npath = 'Downloads'\nby = 'mtime'").is_err());
        assert!(
            SortConfig::parse("[[sort.rules]]\npath = '/tmp'\n[[sort.rules]]\npath = '/tmp'")
                .is_err()
        );
    }
}

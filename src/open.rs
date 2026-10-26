use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, io,
    path::Path,
    process::{Command, Stdio},
    sync::mpsc::Sender,
};

#[derive(Default, Deserialize)]
struct FileConfig {
    #[serde(default)]
    opener: BTreeMap<String, Vec<Opener>>,
    #[serde(default)]
    open: OpenRules,
}

#[derive(Default, Deserialize)]
struct OpenRules {
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Clone, Deserialize)]
pub struct Opener {
    command: Vec<String>,
    desc: String,
    #[serde(rename = "for")]
    platform: Option<String>,
    #[serde(default)]
    pub block: bool,
}

#[derive(Deserialize)]
struct Rule {
    kind: Option<String>,
    glob: Option<String>,
    mime: Option<String>,
    #[serde(rename = "use")]
    openers: Vec<String>,
}

struct CompiledRule {
    directory: bool,
    name: Option<GlobMatcher>,
    mime: Option<GlobMatcher>,
    openers: Vec<String>,
}

pub struct OpenConfig {
    opener: BTreeMap<String, Vec<Opener>>,
    rules: Vec<CompiledRule>,
}

#[derive(Clone)]
pub struct Choice {
    pub desc: String,
    pub opener: Opener,
}

pub struct OpenPlan {
    pub command: Command,
    pub block: bool,
}

impl OpenConfig {
    pub(crate) fn parse(source: &str) -> Result<Self, String> {
        let file: FileConfig = toml::from_str(source).map_err(|e| e.to_string())?;
        for (name, variants) in &file.opener {
            if variants.is_empty() {
                return Err(format!("opener.{name} has no commands"));
            }
            for variant in variants {
                if variant.command.is_empty() || !variant.command.iter().any(|arg| arg == "{file}")
                {
                    return Err(format!(
                        "opener.{name} must contain a command and a separate {{file}} argument"
                    ));
                }
                if variant
                    .command
                    .iter()
                    .any(|arg| arg != "{file}" && arg.contains("{file}"))
                {
                    return Err(format!(
                        "opener.{name}: {{file}} must be a separate argument"
                    ));
                }
                if variant.desc.trim().is_empty() {
                    return Err(format!("opener.{name} needs desc"));
                }
                if let Some(platform) = &variant.platform
                    && !matches!(platform.as_str(), "macos" | "linux" | "unix")
                {
                    return Err(format!("opener.{name} has unsupported platform {platform}"));
                }
            }
        }
        let mut rules = Vec::new();
        for rule in file.open.rules {
            let directory = match rule.kind.as_deref() {
                None | Some("file") => false,
                Some("directory") => true,
                Some(other) => return Err(format!("unsupported open rule kind: {other}")),
            };
            if rule.glob.is_none() && rule.mime.is_none() && !directory {
                return Err("file open rule needs glob or mime".into());
            }
            if directory && rule.mime.is_some() {
                return Err("directory open rule cannot use mime".into());
            }
            if rule.openers.is_empty() {
                return Err("open rule needs at least one opener".into());
            }
            for name in &rule.openers {
                if !file.opener.contains_key(name) {
                    return Err(format!("unknown opener: {name}"));
                }
            }
            rules.push(CompiledRule {
                directory,
                name: rule.glob.as_deref().map(compile_glob).transpose()?,
                mime: rule.mime.as_deref().map(compile_glob).transpose()?,
                openers: rule.openers,
            });
        }
        Ok(Self {
            opener: file.opener,
            rules,
        })
    }

    pub fn choices(&self, path: &Path) -> Vec<Choice> {
        let mut choices = Vec::new();
        let is_directory = path.is_dir();
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        if let Some(rule) = self.rules.iter().find(|rule| {
            rule.directory == is_directory
                && rule
                    .name
                    .as_ref()
                    .is_none_or(|glob| glob.is_match(filename.as_ref()))
                && rule.mime.as_ref().is_none_or(|glob| glob.is_match(&mime))
        }) {
            for name in &rule.openers {
                if let Some(variants) = self.opener.get(name) {
                    for opener in variants {
                        if opener.supports_current_platform() {
                            choices.push(Choice {
                                desc: opener.desc.clone(),
                                opener: opener.clone(),
                            });
                        }
                    }
                }
            }
        }
        choices
    }
}

fn compile_glob(pattern: &str) -> Result<GlobMatcher, String> {
    GlobBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| e.to_string())
}

impl Opener {
    fn supports_current_platform(&self) -> bool {
        match self.platform.as_deref() {
            None | Some("unix") => true,
            Some("macos") => cfg!(target_os = "macos"),
            Some("linux") => cfg!(target_os = "linux"),
            _ => false,
        }
    }

    pub fn command(&self, path: &Path) -> Command {
        let mut command = Command::new(&self.command[0]);
        for arg in self.command.iter().skip(1) {
            if arg == "{file}" {
                command.arg(path);
            } else {
                command.arg(arg);
            }
        }
        command
    }
}

fn is_text_file(path: &Path) -> bool {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    mime.type_().as_str() == "text"
        || matches!(
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some(
                "rs" | "toml" | "json" | "yaml" | "yml" | "sh" | "py" | "js" | "ts" | "tsx" | "jsx"
            )
        )
}

fn editor_value() -> Option<String> {
    env::var("VISUAL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| env::var("EDITOR").ok().filter(|v| !v.trim().is_empty()))
}

pub fn default_plan(path: &Path) -> io::Result<OpenPlan> {
    plan_for(path, editor_value().as_deref())
}

fn plan_for(path: &Path, editor: Option<&str>) -> io::Result<OpenPlan> {
    if is_text_file(path)
        && let Some(editor) = editor
    {
        let words = shell_words::split(editor).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("EDITOR/VISUAL: {e}"))
        })?;
        let Some(program) = words.first() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "EDITOR/VISUAL is empty",
            ));
        };
        let mut command = Command::new(program);
        command.args(&words[1..]).arg(path);
        return Ok(OpenPlan {
            command,
            block: true,
        });
    }
    Ok(OpenPlan {
        command: system_command(path)?,
        block: false,
    })
}

pub fn system_command(path: &Path) -> io::Result<Command> {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "linux") {
        Command::new("xdg-open")
    } else {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no default opener for this OS",
        ));
    };
    command.arg(path);
    Ok(command)
}

pub fn launch_background(mut command: Command, errors: Sender<String>) -> io::Result<()> {
    let program = command.get_program().to_string_lossy().into_owned();
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    std::thread::spawn(move || match child.wait() {
        Ok(status) if !status.success() => {
            let _ = errors.send(format!("{program} exited with {status}"));
        }
        Err(e) => {
            let _ = errors.send(format!("{program}: {e}"));
        }
        _ => {}
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    #[test]
    fn file_rules_select_only_configured_apps_in_order() {
        let config = OpenConfig::parse(
            r#"
            [opener]
            image = [{ command = ["viewer", "{file}"], desc = "Viewer" }]
            editor = [{ command = ["editor", "{file}"], desc = "Editor" }]
            [[open.rules]]
            glob = "*.{png,jpg}"
            use = ["image", "editor"]
        "#,
        )
        .unwrap();
        let choices = config.choices(Path::new("a photo.PNG"));
        assert_eq!(
            choices.iter().map(|c| c.desc.as_str()).collect::<Vec<_>>(),
            ["Viewer", "Editor"]
        );
        let command = choices[0].opener.command(Path::new("a photo.PNG"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsString::from("a photo.PNG").as_os_str()]
        );
    }

    #[test]
    fn config_rejects_unknown_opener_and_embedded_placeholder() {
        assert!(
            OpenConfig::parse("[[open.rules]]\nglob = \"*.pdf\"\nuse = [\"missing\"]").is_err()
        );
        assert!(
            OpenConfig::parse(
                "[opener]\nbad = [{ command = [\"app\", \"--file={file}\"], desc = \"Bad\" }]"
            )
            .is_err()
        );
    }

    #[test]
    fn rust_file_uses_editor_as_blocking_default() {
        let path = Path::new("/tmp/example file.rs");
        let plan = plan_for(path, Some("nvim -u NONE")).unwrap();
        assert!(plan.block);
        assert_eq!(plan.command.get_program(), "nvim");
        assert_eq!(
            plan.command.get_args().collect::<Vec<_>>(),
            ["-u", "NONE", "/tmp/example file.rs"].map(std::ffi::OsStr::new)
        );
    }

    #[test]
    fn non_text_file_keeps_system_default() {
        let plan = plan_for(Path::new("/tmp/image.png"), Some("nvim")).unwrap();
        assert!(!plan.block);
        assert_eq!(
            plan.command.get_program(),
            if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            }
        );
    }

    #[test]
    fn example_config_matches_directories_images_and_source() {
        let config = OpenConfig::parse(include_str!("../salt.toml.example")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let folders = config.choices(dir.path());
        assert_eq!(
            folders.iter().map(|c| c.desc.as_str()).collect::<Vec<_>>(),
            if cfg!(target_os = "macos") {
                vec!["Finder", "Neovim", "VS Code"]
            } else {
                vec!["Neovim", "VS Code"]
            }
        );
        let image = config.choices(Path::new("photo.PNG"));
        if cfg!(target_os = "macos") {
            assert_eq!(
                image.iter().map(|c| c.desc.as_str()).collect::<Vec<_>>(),
                ["Preview", "ImageOptim"]
            );
        }
        let source = config.choices(Path::new("main.rs"));
        assert_eq!(source[0].desc, "Neovim");
        assert!(source[0].opener.block);
        assert!(config.choices(Path::new("unknown.bin")).is_empty());
    }
}

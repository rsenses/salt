//! Nerd Font glyphs inspired by Superfile's file icon table.
//! Colors are supplied by the terminal theme in the UI, not by this mapping.

use crate::model::EntryKind;

pub fn icon(name: &str, kind: EntryKind, enabled: bool) -> &'static str {
    if !enabled {
        return "";
    }

    match kind {
        EntryKind::Directory => directory_icon(name.trim_end_matches('/')),
        EntryKind::File => file_icon(name),
        EntryKind::Symlink => "\u{f0c1} ",
    }
}

fn directory_icon(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "downloads" => "\u{f03d4} ",
        "documents" => "\u{f0219} ",
        "pictures" | "images" => "\u{f02e9} ",
        "videos" | "movies" => "\u{f0381} ",
        "music" => "\u{f001} ",
        "desktop" => "\u{f01c4} ",
        ".git" => "\u{e5fb} ",
        ".github" => "\u{e5fd} ",
        ".vscode" => "\u{e70c} ",
        "node_modules" => "\u{e5fa} ",
        "config" | ".config" => "\u{e5fc} ",
        _ => "\u{f07b} ",
    }
}

fn file_icon(name: &str) -> &'static str {
    let name = name.to_ascii_lowercase();

    // Names with a meaningful extension (Cargo.toml) take precedence over it.
    match name.as_str() {
        "cargo.toml" | "cargo.lock" | "package.json" | "package-lock.json" | "pnpm-lock.yaml"
        | "yarn.lock" | "pyproject.toml" | "poetry.lock" | "go.mod" | "go.sum" | "gemfile"
        | "brewfile" => return "\u{f03d7} ",
        "dockerfile" | ".dockerignore" => return "\u{f0868} ",
        "makefile" | "justfile" | "cmakelists.txt" => return "\u{e20f} ",
        "readme" | "readme.md" | "readme.txt" => return "\u{f48a} ",
        "license" | "license.md" | "license.txt" | "licence" => return "\u{f43d} ",
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitconfig" => {
            return "\u{e702} ";
        }
        ".env" | ".env.local" | ".env.example" => return "\u{f462} ",
        ".zshrc" | ".bashrc" | ".bash_profile" | ".zprofile" => {
            return "\u{f489} ";
        }
        ".vimrc" | ".gvimrc" => return "\u{e62b} ",
        _ => {}
    }

    let extension = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
    match extension {
        "rs" => "\u{e7a8} ",
        "py" | "pyc" | "pyx" => "\u{e606} ",
        "js" | "cjs" => "\u{e781} ",
        "mjs" => "\u{e718} ",
        "ts" => "\u{f06e6} ",
        "jsx" | "tsx" => "\u{e7ba} ",
        "go" => "\u{e627} ",
        "c" => "\u{e649} ",
        "cc" | "cpp" | "cxx" | "c++" => "\u{e646} ",
        "h" | "hh" | "hpp" | "hxx" => "\u{f0fd} ",
        "java" | "jar" => "\u{e738} ",
        "kt" | "kts" => "\u{e634} ",
        "rb" | "ru" => "\u{e21e} ",
        "php" | "phtml" => "\u{e608} ",
        "lua" => "\u{e620} ",
        "swift" => "\u{e755} ",
        "zig" => "\u{e6a9} ",
        "sh" | "bash" | "zsh" | "fish" | "ksh" => "\u{f489} ",
        "vim" => "\u{e62b} ",
        "html" | "htm" | "xhtml" => "\u{f13b} ",
        "css" | "scss" => "\u{f13c} ",
        "sass" => "\u{e603} ",
        "vue" => "\u{e6a0} ",
        "svelte" => "\u{e697} ",
        "json" | "jsonc" | "webmanifest" => "\u{e60b} ",
        "yaml" | "yml" => "\u{e601} ",
        "toml" | "ini" => "\u{f016a} ",
        "xml" | "plist" => "\u{e796} ",
        "md" | "markdown" | "rst" => "\u{f48a} ",
        "txt" | "log" => "\u{f15c} ",
        "pdf" => "\u{f1c1} ",
        "doc" | "docx" | "odt" => "\u{e6a5} ",
        "xls" | "xlsx" | "ods" | "csv" | "tsv" => "\u{f1c3} ",
        "ppt" | "pptx" | "odp" => "\u{f1c4} ",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "avif"
        | "ico" | "svg" => "\u{f1c5} ",
        "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aac" => "\u{f001} ",
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "mpeg" | "mpg" => "\u{f03d} ",
        "zip" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "7z" | "rar" => "\u{f410} ",
        "db" | "sqlite" | "sqlite3" | "sql" => "\u{f1c0} ",
        "ttf" | "otf" | "woff" | "woff2" | "eot" => "\u{f031} ",
        "pem" | "key" | "crt" | "cer" | "gpg" => "\u{f43d} ",
        "dmg" | "iso" | "img" => "\u{f02ca} ",
        "exe" | "bat" | "cmd" => "\u{f17a} ",
        _ => "\u{f15b} ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizable_types_and_special_names() {
        assert_eq!(icon("main.rs", EntryKind::File, true), "\u{e7a8} ");
        assert_eq!(icon("PHOTO.JPG", EntryKind::File, true), "\u{f1c5} ");
        assert_eq!(icon("Cargo.toml", EntryKind::File, true), "\u{f03d7} ");
        assert_eq!(icon(".gitignore", EntryKind::File, true), "\u{e702} ");
        assert_eq!(icon("unknown", EntryKind::File, true), "\u{f15b} ");
    }

    #[test]
    fn directories_and_symlinks_keep_their_own_icons() {
        assert_eq!(icon("Downloads/", EntryKind::Directory, true), "\u{f03d4} ");
        assert_eq!(icon("test", EntryKind::Directory, true), "\u{f07b} ");
        assert_eq!(icon("main.rs", EntryKind::Symlink, true), "\u{f0c1} ");
    }

    #[test]
    fn disabled_icons_have_no_glyph_or_padding() {
        assert_eq!(icon("main.rs", EntryKind::File, false), "");
        assert_eq!(icon("Downloads", EntryKind::Directory, false), "");
    }
}

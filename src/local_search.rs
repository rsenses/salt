use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn choose_path(root: &Path, show_hidden: bool) -> io::Result<Option<PathBuf>> {
    let paths = candidates(root, show_hidden)?;
    if paths.is_empty() {
        return Ok(None);
    }
    let mut child = Command::new("fzf")
        .args(["--no-multi", "--read0", "--print0", "--prompt", "salt> "])
        .env_remove("FZF_DEFAULT_OPTS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        for path in &paths {
            let relative = path.strip_prefix(root).expect("candidate beneath root");
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStrExt;
                stdin.write_all(relative.as_os_str().as_bytes())?;
            }
            #[cfg(not(unix))]
            stdin.write_all(relative.to_string_lossy().as_bytes())?;
            stdin.write_all(&[0])?;
        }
    }
    let output = child.wait_with_output()?;
    if matches!(output.status.code(), Some(1 | 130)) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(io::Error::other(format!("fzf failed: {}", output.status)));
    }
    let selected = output.stdout.strip_suffix(&[0]).unwrap_or(&output.stdout);
    if selected.is_empty() {
        return Ok(None);
    }
    #[cfg(unix)]
    let relative = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(selected.to_vec()))
    };
    #[cfg(not(unix))]
    let relative = PathBuf::from(
        String::from_utf8(output.stdout)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
    );
    Ok(Some(root.join(relative)))
}

fn candidates(root: &Path, show_hidden: bool) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied && dir != root => {
                continue;
            }
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            if !show_hidden && entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path.clone());
            }
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_nested_files_and_directories_without_hidden_entries() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("a/b")).unwrap();
        fs::write(root.path().join("a/b/file.txt"), "").unwrap();
        fs::write(root.path().join(".hidden"), "").unwrap();
        let visible = candidates(root.path(), false).unwrap();
        assert!(visible.contains(&root.path().join("a")));
        assert!(visible.contains(&root.path().join("a/b")));
        assert!(visible.contains(&root.path().join("a/b/file.txt")));
        assert!(!visible.contains(&root.path().join(".hidden")));
        assert!(
            candidates(root.path(), true)
                .unwrap()
                .contains(&root.path().join(".hidden"))
        );
    }
}

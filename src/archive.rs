use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub fn suggested_name(paths: &[PathBuf]) -> String {
    if paths.len() == 1 {
        format!(
            "{}.zip",
            paths[0].file_name().unwrap_or_default().to_string_lossy()
        )
    } else {
        "archive.zip".into()
    }
}

pub fn create_zip(base: &Path, paths: &[PathBuf], name: &str) -> io::Result<PathBuf> {
    if paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Nothing to compress",
        ));
    }
    let filename = Path::new(name);
    if filename
        .file_name()
        .is_none_or(|part| part != filename.as_os_str())
        || name == "."
        || name == ".."
        || !name.ends_with(".zip")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Use a .zip filename without directories",
        ));
    }
    let destination = base.join(name);
    let temp = tempfile::Builder::new()
        .prefix(".salt-archive-")
        .tempdir_in(base)?;
    let staged = temp.path().join("archive.zip");
    let mut command = Command::new("zip");
    command
        .args(["-q", "-r", "-y"])
        .arg(&staged)
        .current_dir(base);
    for path in paths {
        let relative = path.strip_prefix(base).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Selection is outside this directory",
            )
        })?;
        if relative.components().count() != 1 || fs::symlink_metadata(path).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Selected path is no longer available",
            ));
        }
        command.arg(Path::new(".").join(relative));
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "zip exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    // Creating the final directory entry atomically prevents overwriting a file.
    fs::hard_link(&staged, &destination)?;
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_multiple_items_without_overwriting() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("one.txt"), "one").unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("folder/two.txt"), "two").unwrap();
        let selected = [root.path().join("one.txt"), root.path().join("folder")];
        let archive = create_zip(root.path(), &selected, "bundle.zip").unwrap();
        assert!(fs::metadata(&archive).unwrap().len() > 0);
        assert!(create_zip(root.path(), &selected, "bundle.zip").is_err());
        assert_eq!(fs::read(&archive).unwrap()[..2], *b"PK");
    }

    #[test]
    fn rejects_paths_and_unsupported_names() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a"), "a").unwrap();
        let selected = [root.path().join("a")];
        assert!(create_zip(root.path(), &selected, "../bad.zip").is_err());
        assert!(create_zip(root.path(), &selected, "bad.tar.gz").is_err());
    }
}

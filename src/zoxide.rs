use std::{
    io::{self, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

pub fn choose_directory() -> io::Result<Option<PathBuf>> {
    let query = Command::new("zoxide").args(["query", "--list"]).output()?;
    if !query.status.success() {
        return Err(io::Error::other(format!(
            "zoxide query failed: {}",
            String::from_utf8_lossy(&query.stderr).trim()
        )));
    }
    if query.stdout.is_empty() {
        return Ok(None);
    }

    // fzf uses /dev/tty for interaction when its candidate list comes from stdin.
    // Its default display mode occupies the terminal screen.
    let mut child = Command::new("fzf")
        .args(["--no-multi", "--prompt", "zoxide> "])
        .env_remove("FZF_DEFAULT_OPTS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&query.stdout)?;
    }
    let output = child.wait_with_output()?;
    if matches!(output.status.code(), Some(1 | 130)) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err(io::Error::other(format!("fzf failed: {}", output.status)));
    }
    let selected = String::from_utf8(output.stdout)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let path = selected.trim_end_matches(['\n', '\r']);
    if path.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(path)))
}

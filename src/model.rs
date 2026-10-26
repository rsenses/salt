use crate::sort::{SortBy, SortConfig, SortSpec};
use std::{
    cmp::Ordering,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: OsString,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub modified: Option<SystemTime>,
    pub size: Option<u64>,
}

// Rows carry filesystem identity independently of display text so a later
// editable buffer can store proposed names without losing original paths.
pub struct Browser {
    path: PathBuf,
    entries: Vec<Entry>,
    selected: Option<usize>,
    show_hidden: bool,
    sort_config: SortConfig,
    sort_override: Option<SortSpec>,
}
impl Browser {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let path = path.canonicalize()?;
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path is not a directory",
            ));
        }
        let mut browser = Self {
            path,
            entries: Vec::new(),
            selected: None,
            show_hidden: false,
            sort_config: SortConfig::default(),
            sort_override: None,
        };
        browser.refresh()?;
        Ok(browser)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn shows_hidden(&self) -> bool {
        self.show_hidden
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn sort(&self) -> SortSpec {
        self.sort_override
            .unwrap_or_else(|| self.sort_config.for_path(&self.path))
    }
    pub fn set_sort_config(&mut self, config: SortConfig) -> io::Result<()> {
        self.sort_config = config;
        self.refresh()
    }
    pub fn set_sort(&mut self, spec: SortSpec) -> io::Result<()> {
        let previous = self.sort_override.replace(spec);
        if let Err(error) = self.refresh() {
            self.sort_override = previous;
            return Err(error);
        }
        Ok(())
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
    pub fn select(&mut self, index: usize) {
        self.selected = (index < self.entries.len()).then_some(index);
    }
    pub fn select_name(&mut self, name: &str) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.name.to_string_lossy() == name)
        {
            self.selected = Some(index);
        }
    }
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected
            .and_then(|i| self.entries.get(i))
            .map(|e| e.path.as_path())
    }
    pub fn refresh(&mut self) -> io::Result<()> {
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.path)? {
            let item = item?;
            let name = item.file_name();
            if !self.show_hidden && name.to_string_lossy().starts_with('.') {
                continue;
            }
            let file_type = item.file_type()?;
            let kind = if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            let metadata = fs::symlink_metadata(item.path()).ok();
            entries.push(Entry {
                name,
                path: item.path(),
                kind,
                modified: metadata.as_ref().and_then(|info| info.modified().ok()),
                size: metadata
                    .as_ref()
                    .and_then(|info| info.is_file().then_some(info.len())),
            });
        }
        let sort = self.sort();
        entries.sort_by(|a, b| compare_entries(a, b, sort));
        let old_path = self
            .selected
            .and_then(|i| self.entries.get(i))
            .map(|e| &e.path);
        let selected = old_path
            .and_then(|p| entries.iter().position(|e| &e.path == p))
            .or_else(|| {
                (!entries.is_empty()).then_some(
                    self.selected
                        .unwrap_or(0)
                        .min(entries.len().saturating_sub(1)),
                )
            });
        self.entries = entries;
        self.selected = selected;
        Ok(())
    }
    pub fn toggle_hidden(&mut self) -> io::Result<()> {
        self.show_hidden = !self.show_hidden;
        self.refresh()
    }
    #[cfg(test)]
    pub fn move_down(&mut self) {
        if let Some(i) = self.selected {
            self.selected = Some((i + 1).min(self.entries.len() - 1));
        }
    }
    #[cfg(test)]
    pub fn move_up(&mut self) {
        if let Some(i) = self.selected {
            self.selected = Some(i.saturating_sub(1));
        }
    }
    pub fn enter(&mut self) -> io::Result<()> {
        let Some(entry) = self.selected.and_then(|i| self.entries.get(i)) else {
            return Ok(());
        };
        let path = entry.path.canonicalize()?;
        if !path.is_dir() {
            return Ok(());
        }
        self.change_dir(path)
    }
    pub fn parent(&mut self) -> io::Result<()> {
        let Some(path) = self.path.parent() else {
            return Ok(());
        };
        let old = self.path.clone();
        self.change_dir(path.to_path_buf())?;
        self.selected = self
            .entries
            .iter()
            .position(|e| e.path == old)
            .or(self.selected);
        Ok(())
    }
    pub fn navigate_to(&mut self, path: &Path) -> io::Result<()> {
        let path = path.canonicalize()?;
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path is not a directory",
            ));
        }
        self.change_dir(path)
    }
    pub fn reveal(&mut self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Path has no parent directory"))?;
        self.navigate_to(parent)?;
        self.selected = self
            .entries
            .iter()
            .position(|entry| entry.name == path.file_name().unwrap_or_default());
        if self.selected.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "File is no longer visible",
            ));
        }
        Ok(())
    }
    fn change_dir(&mut self, path: PathBuf) -> io::Result<()> {
        let old = self.path.clone();
        let old_override = self.sort_override.take();
        self.path = path;
        self.selected = None;
        if let Err(e) = self.refresh() {
            self.path = old;
            self.sort_override = old_override;
            let _ = self.refresh();
            return Err(e);
        }
        Ok(())
    }
}

pub(crate) fn compare_entries(a: &Entry, b: &Entry, spec: SortSpec) -> Ordering {
    let directories = if spec.directories_first {
        (a.kind != EntryKind::Directory).cmp(&(b.kind != EntryKind::Directory))
    } else {
        Ordering::Equal
    };
    let names = || {
        a.name
            .to_string_lossy()
            .to_lowercase()
            .cmp(&b.name.to_string_lossy().to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    };
    let primary = match spec.by {
        SortBy::Name => names(),
        SortBy::Mtime => compare_optional(a.modified, b.modified, spec.reverse),
        SortBy::Size => compare_optional(a.size, b.size, spec.reverse),
    };
    directories
        .then_with(|| {
            if spec.by == SortBy::Name && spec.reverse {
                primary.reverse()
            } else {
                primary
            }
        })
        .then_with(names)
}

fn compare_optional<T: Ord>(a: Option<T>, b: Option<T>, reverse: bool) -> Ordering {
    match (a, b) {
        (Some(left), Some(right)) => {
            let order = left.cmp(&right);
            if reverse { order.reverse() } else { order }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_and_hidden_filter() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("folder")).unwrap();
        fs::write(dir.path().join("file"), "").unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        let mut b = Browser::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(b.entries.len(), 2);
        assert_eq!(b.entries[0].name, "folder");
        b.enter().unwrap();
        assert_eq!(b.path.file_name().unwrap(), "folder");
        b.parent().unwrap();
        assert_eq!(b.entries[b.selected.unwrap()].name, "folder");
        b.toggle_hidden().unwrap();
        assert_eq!(b.entries.len(), 3);
        b.toggle_hidden().unwrap();
        assert_eq!(b.entries.len(), 2);
    }
    #[test]
    fn empty_directory_navigation_is_safe() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = Browser::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(b.selected(), None);
        b.move_up();
        b.move_down();
        b.enter().unwrap();
        assert_eq!(b.path(), dir.path().canonicalize().unwrap());
    }
    #[test]
    fn navigate_to_validates_destination_and_preserves_current_dir_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let mut b = Browser::new(dir.path().to_path_buf()).unwrap();
        b.navigate_to(other.path()).unwrap();
        assert_eq!(b.path(), other.path().canonicalize().unwrap());
        fs::write(other.path().join("file"), "").unwrap();
        assert!(b.navigate_to(&other.path().join("file")).is_err());
        assert_eq!(b.path(), other.path().canonicalize().unwrap());
    }
    #[test]
    fn reveal_selects_file_in_nested_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("a")).unwrap();
        fs::write(root.path().join("a/file.txt"), "").unwrap();
        let mut browser = Browser::new(root.path().to_path_buf()).unwrap();
        browser.reveal(&root.path().join("a/file.txt")).unwrap();
        assert_eq!(
            browser.path(),
            root.path().join("a").canonicalize().unwrap()
        );
        assert_eq!(
            browser.selected_path().unwrap().file_name().unwrap(),
            "file.txt"
        );
    }
    #[test]
    fn selects_new_file_by_name_after_refresh() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("alpha.txt"), "").unwrap();
        let mut browser = Browser::new(root.path().to_path_buf()).unwrap();
        fs::write(root.path().join("zeta.txt"), "").unwrap();
        browser.refresh().unwrap();
        browser.select_name("zeta.txt");
        assert_eq!(
            browser.selected_path().unwrap().file_name().unwrap(),
            "zeta.txt"
        );
    }

    #[test]
    fn changing_sort_keeps_selected_entry() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("alpha"), "a").unwrap();
        fs::write(root.path().join("beta"), "b").unwrap();
        let mut browser = Browser::new(root.path().to_path_buf()).unwrap();
        browser.select_name("alpha");
        browser
            .set_sort(SortSpec {
                by: SortBy::Name,
                reverse: true,
                directories_first: true,
            })
            .unwrap();
        assert_eq!(browser.entries()[0].name, "beta");
        assert_eq!(
            browser.selected_path().unwrap().file_name().unwrap(),
            "alpha"
        );
    }

    #[test]
    fn size_sort_and_directory_grouping() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("small"), "a").unwrap();
        fs::write(root.path().join("large"), "abcdefgh").unwrap();
        let mut browser = Browser::new(root.path().to_path_buf()).unwrap();
        browser
            .set_sort(SortSpec {
                by: SortBy::Size,
                reverse: true,
                directories_first: false,
            })
            .unwrap();
        let names: Vec<_> = browser
            .entries()
            .iter()
            .map(|entry| entry.name.to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["large", "small", "folder"]);
        browser
            .set_sort(SortSpec {
                by: SortBy::Size,
                reverse: true,
                directories_first: true,
            })
            .unwrap();
        assert_eq!(browser.entries()[0].name, "folder");
    }

    #[test]
    fn entering_directory_uses_its_configured_sort() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("alpha"), "").unwrap();
        fs::write(child.join("beta"), "").unwrap();
        let config = SortConfig::parse(&format!(
            "[[sort.rules]]\npath = '{}'\nby = 'name'\nreverse = true",
            child.display()
        ))
        .unwrap();
        let mut browser = Browser::new(root.path().to_path_buf()).unwrap();
        browser.set_sort_config(config).unwrap();
        browser.select_name("child");
        browser.enter().unwrap();
        assert_eq!(browser.entries()[0].name, "beta");
        browser.set_sort(SortSpec::default()).unwrap();
        assert_eq!(browser.entries()[0].name, "alpha");
        browser.parent().unwrap();
        browser.enter().unwrap();
        assert_eq!(browser.entries()[0].name, "beta");
    }
}

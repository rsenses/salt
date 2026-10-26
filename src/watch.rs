//! Event-driven updates for the directory currently shown in the browser.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub struct DirectoryWatcher {
    watcher: RecommendedWatcher,
    events: Receiver<notify::Result<Event>>,
    path: PathBuf,
    last_change: Option<Instant>,
}

impl DirectoryWatcher {
    const QUIET_PERIOD: Duration = Duration::from_millis(150);

    pub fn new(path: &Path) -> notify::Result<Self> {
        let (sender, events) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(sender)?;
        watcher.watch(path, RecursiveMode::NonRecursive)?;
        Ok(Self {
            watcher,
            events,
            path: path.to_path_buf(),
            last_change: None,
        })
    }

    pub fn watch(&mut self, path: &Path) -> notify::Result<()> {
        if path == self.path {
            return Ok(());
        }
        self.watcher.watch(path, RecursiveMode::NonRecursive)?;
        let _ = self.watcher.unwatch(&self.path);
        self.path = path.to_path_buf();
        self.last_change = None;
        while self.events.try_recv().is_ok() {}
        Ok(())
    }

    pub fn collect(&mut self) {
        while let Ok(result) = self.events.try_recv() {
            match result {
                Ok(event) if is_visible_change(&event, &self.path) => {
                    self.last_change = Some(Instant::now());
                }
                Err(_) => self.last_change = Some(Instant::now()),
                _ => {}
            }
        }
    }

    pub fn ready(&self) -> bool {
        self.last_change
            .is_some_and(|change| change.elapsed() >= Self::QUIET_PERIOD)
    }

    pub fn acknowledge(&mut self) {
        self.last_change = None;
    }
}

fn is_visible_change(event: &Event, directory: &Path) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.is_empty()
        || event
            .paths
            .iter()
            .any(|path| path == directory || path.parent() == Some(directory))
}

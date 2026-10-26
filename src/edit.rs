use crate::model::{Browser, EntryKind};
use std::{
    collections::{BTreeSet, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub original: Option<PathBuf>,
    pub copy_from: Option<PathBuf>,
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
}
#[derive(Clone)]
enum Register {
    Copy(Row),
    Cut(Row),
    Rows(Vec<Row>, bool),
    Text(String),
}

pub struct Buffer {
    pub rows: Vec<Row>,
    pub row: usize,
    pub col: usize,
    pub mode: Mode,
    pub dirty: bool,
    pub marked: BTreeSet<usize>,
    undo: Vec<Vec<Row>>,
    redo: Vec<Vec<Row>>,
    register: Option<Register>,
    pub pending: Option<char>,
    pub visual_anchor: Option<(usize, usize)>,
    block_insert_rows: Vec<usize>,
}

impl Buffer {
    pub fn from_browser(browser: &Browser) -> Self {
        Self {
            rows: browser
                .entries()
                .iter()
                .map(|entry| Row {
                    original: Some(entry.path.clone()),
                    copy_from: None,
                    name: entry.name.to_string_lossy().into_owned(),
                    kind: entry.kind,
                })
                .collect(),
            row: browser.selected().unwrap_or(0),
            col: 0,
            mode: Mode::Normal,
            dirty: false,
            marked: BTreeSet::new(),
            undo: vec![],
            redo: vec![],
            register: None,
            pending: None,
            visual_anchor: None,
            block_insert_rows: Vec::new(),
        }
    }
    pub fn load(&mut self, browser: &Browser) {
        let register = self.register.clone();
        *self = Self::from_browser(browser);
        self.register = register;
    }
    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.row)
    }
    pub fn selected_path(&self, base: &Path) -> Option<PathBuf> {
        self.selected()
            .map(|row| row.original.clone().unwrap_or_else(|| base.join(&row.name)))
    }
    pub fn snapshot(&mut self) {
        self.marked.clear();
        self.undo.push(self.rows.clone());
        self.redo.clear();
        self.dirty = true;
    }
    pub fn start_visual(&mut self, mode: Mode) {
        if self.rows.is_empty() {
            return;
        }
        self.mode = mode;
        self.marked.clear();
        self.visual_anchor = Some((self.row, self.col));
    }
    pub fn toggle_mark_current(&mut self) {
        if self.selected().is_none() {
            return;
        }
        if !self.marked.insert(self.row) {
            self.marked.remove(&self.row);
        }
        self.down();
    }
    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }
    pub fn stop_visual(&mut self) {
        self.mode = Mode::Normal;
        self.visual_anchor = None;
    }
    pub fn leave_insert(&mut self) {
        self.mode = Mode::Normal;
        self.block_insert_rows.clear();
        self.col = self.col.saturating_sub(1);
    }
    pub fn visual_range(&self, row: usize) -> Option<(usize, usize)> {
        let (anchor_row, anchor_col) = self.visual_anchor?;
        let len = self.rows.get(row)?.name.chars().count();
        match self.mode {
            Mode::VisualLine => {
                if row >= anchor_row.min(self.row) && row <= anchor_row.max(self.row) {
                    Some((0, len))
                } else {
                    None
                }
            }
            Mode::VisualBlock => {
                if row >= anchor_row.min(self.row) && row <= anchor_row.max(self.row) {
                    Some((
                        anchor_col.min(self.col).min(len),
                        (anchor_col.max(self.col) + 1).min(len),
                    ))
                } else {
                    None
                }
            }
            Mode::Visual => {
                let start = (anchor_row, anchor_col).min((self.row, self.col));
                let end = (anchor_row, anchor_col).max((self.row, self.col));
                if row < start.0 || row > end.0 {
                    None
                } else {
                    Some((
                        if row == start.0 { start.1.min(len) } else { 0 },
                        if row == end.0 {
                            (end.1 + 1).min(len)
                        } else {
                            len
                        },
                    ))
                }
            }
            _ => None,
        }
    }
    pub fn visual_operator(&mut self, op: char) {
        if self.visual_anchor.is_none() {
            return;
        }
        if self.mode == Mode::VisualLine {
            let (anchor, _) = self.visual_anchor.unwrap();
            let start = anchor.min(self.row);
            let end = anchor.max(self.row);
            if op == 'c' {
                self.stop_visual();
                if start == end {
                    self.change_line();
                }
                return;
            }
            self.rows_operator(&(start..=end).collect::<Vec<_>>(), op);
            self.stop_visual();
            return;
        }
        let selected: Vec<(usize, usize, usize)> = (0..self.rows.len())
            .filter_map(|row| self.visual_range(row).map(|(start, end)| (row, start, end)))
            .filter(|(_, start, end)| start < end)
            .collect();
        let copied = selected
            .iter()
            .map(|(row, start, end)| {
                let name = &self.rows[*row].name;
                name[byte_index(name, *start)..byte_index(name, *end)].to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.register = Some(Register::Text(copied));
        if op != 'y' && !selected.is_empty() {
            self.snapshot();
            for (row, start, end) in &selected {
                let name = &mut self.rows[*row].name;
                let a = byte_index(name, *start);
                let b = byte_index(name, *end);
                name.replace_range(a..b, "");
            }
            self.row = selected[0].0;
            self.col = selected[0].1;
            if op == 'c' {
                self.col = self.col.min(self.rows[self.row].name.chars().count());
            } else {
                self.clamp();
            }
        }
        let block_rows = if self.mode == Mode::VisualBlock && op == 'c' {
            selected.iter().map(|(row, _, _)| *row).collect()
        } else {
            Vec::new()
        };
        self.stop_visual();
        if op == 'c' {
            self.enter_insert(false, false);
            self.block_insert_rows = block_rows;
        }
    }
    pub fn rows_operator(&mut self, indices: &[usize], op: char) {
        if indices.is_empty()
            || indices.iter().any(|&index| index >= self.rows.len())
            || indices.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return;
        }
        let selected = indices
            .iter()
            .map(|&index| self.rows[index].clone())
            .collect();
        self.register = Some(Register::Rows(selected, op != 'y'));
        if op != 'y' {
            self.snapshot();
            for &index in indices.iter().rev() {
                self.rows.remove(index);
            }
            self.row = indices[0];
            self.col = 0;
            self.clamp();
        }
    }
    fn clamp(&mut self) {
        if self.rows.is_empty() {
            self.row = 0;
            self.col = 0;
            return;
        }
        self.row = self.row.min(self.rows.len() - 1);
        self.col = self
            .col
            .min(self.rows[self.row].name.chars().count().saturating_sub(1));
    }
    pub fn down(&mut self) {
        if !self.rows.is_empty() {
            self.row = (self.row + 1).min(self.rows.len() - 1);
            self.clamp();
        }
    }
    pub fn up(&mut self) {
        self.row = self.row.saturating_sub(1);
        self.clamp();
    }
    pub fn left(&mut self) {
        self.col = self.col.saturating_sub(1);
    }
    pub fn right(&mut self) {
        if let Some(row) = self.selected() {
            self.col = (self.col + 1).min(row.name.chars().count().saturating_sub(1));
        }
    }
    pub fn end(&mut self) {
        if let Some(row) = self.selected() {
            self.col = row.name.chars().count().saturating_sub(1);
        }
    }
    pub fn word_forward(&mut self) {
        self.word_motion(true);
    }
    pub fn word_back(&mut self) {
        self.word_motion(false);
    }
    fn word_motion(&mut self, forward: bool) {
        let Some(row) = self.selected() else {
            return;
        };
        let chars: Vec<char> = row.name.chars().collect();
        if forward {
            let mut i = (self.col + 1).min(chars.len());
            while i < chars.len() && word(chars[i]) == word(chars[i - 1]) {
                i += 1;
            }
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            self.col = i.min(chars.len().saturating_sub(1));
        } else {
            let mut i = self.col.saturating_sub(1);
            while i > 0 && chars[i].is_whitespace() {
                i -= 1;
            }
            while i > 0 && word(chars[i - 1]) == word(chars[i]) {
                i -= 1;
            }
            self.col = i;
        }
    }
    pub fn enter_insert(&mut self, after: bool, at_end: bool) {
        if at_end {
            self.col = self.selected().map_or(0, |r| r.name.chars().count());
        } else if after {
            self.col = self
                .col
                .saturating_add(1)
                .min(self.selected().map_or(0, |r| r.name.chars().count()));
        }
        self.mode = Mode::Insert;
    }
    pub fn insert(&mut self, c: char) {
        if self.selected().is_none() || c == '\0' || c == '\n' {
            return;
        }
        self.snapshot();
        if !self.block_insert_rows.is_empty() {
            for &index in &self.block_insert_rows {
                let row = &mut self.rows[index];
                let byte = byte_index(&row.name, self.col);
                row.name.insert(byte, c);
            }
            self.col += 1;
            return;
        }
        let row = &mut self.rows[self.row];
        let byte = byte_index(&row.name, self.col);
        row.name.insert(byte, c);
        if row.original.is_none() {
            row.kind = if row.name.ends_with('/') {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
        }
        self.col += 1;
    }
    pub fn backspace(&mut self) {
        if self.selected().is_none() || self.col == 0 {
            return;
        }
        self.snapshot();
        if !self.block_insert_rows.is_empty() {
            for &index in &self.block_insert_rows {
                let row = &mut self.rows[index];
                if self.col <= row.name.chars().count() {
                    let a = byte_index(&row.name, self.col - 1);
                    let b = byte_index(&row.name, self.col);
                    row.name.replace_range(a..b, "");
                }
            }
            self.col -= 1;
            return;
        }
        let row = &mut self.rows[self.row];
        let a = byte_index(&row.name, self.col - 1);
        let b = byte_index(&row.name, self.col);
        row.name.replace_range(a..b, "");
        self.col -= 1;
    }
    pub fn delete_char(&mut self) {
        if self.selected().is_none() {
            return;
        }
        let len = self.rows[self.row].name.chars().count();
        if self.col >= len {
            return;
        }
        self.snapshot();
        let row = &mut self.rows[self.row];
        let a = byte_index(&row.name, self.col);
        let b = byte_index(&row.name, self.col + 1);
        row.name.replace_range(a..b, "");
        self.clamp();
    }
    pub fn replace_char(&mut self, c: char) {
        if self.selected().is_none() || c == '/' || c == '\0' || c == '\n' {
            return;
        }
        if self.col >= self.rows[self.row].name.chars().count() {
            return;
        }
        self.snapshot();
        let row = &mut self.rows[self.row];
        let a = byte_index(&row.name, self.col);
        let b = byte_index(&row.name, self.col + 1);
        row.name.replace_range(a..b, &c.to_string());
    }
    pub fn word_end(&mut self) {
        let Some(row) = self.selected() else {
            return;
        };
        let chars: Vec<char> = row.name.chars().collect();
        let mut i = self.col.min(chars.len().saturating_sub(1));
        while i + 1 < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        while i + 1 < chars.len() && word(chars[i + 1]) == word(chars[i]) {
            i += 1;
        }
        self.col = i;
    }
    pub fn delete_word(&mut self, insert: bool) {
        if self.selected().is_none() {
            return;
        }
        let old = self.col;
        let chars: Vec<char> = self.rows[self.row].name.chars().collect();
        if old >= chars.len() {
            return;
        }
        let mut end = (old + 1).min(chars.len());
        while end < chars.len() && word(chars[end]) == word(chars[old]) {
            end += 1;
        }
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        self.snapshot();
        let row = &mut self.rows[self.row];
        let a = byte_index(&row.name, old);
        let b = byte_index(&row.name, end);
        row.name.replace_range(a..b, "");
        self.col = old;
        if insert {
            self.mode = Mode::Insert;
        }
        self.clamp();
    }
    pub fn delete_to_end(&mut self, insert: bool) {
        if self.selected().is_none() {
            return;
        }
        self.snapshot();
        let row = &mut self.rows[self.row];
        row.name.truncate(byte_index(&row.name, self.col));
        if insert {
            self.mode = Mode::Insert;
        }
        self.clamp();
    }
    pub fn change_line(&mut self) {
        if self.selected().is_none() {
            return;
        }
        self.snapshot();
        self.rows[self.row].name.clear();
        self.col = 0;
        self.mode = Mode::Insert;
    }
    pub fn open_line(&mut self, above: bool) {
        self.snapshot();
        let pos = if above {
            self.row
        } else {
            (self.row + 1).min(self.rows.len())
        };
        self.rows.insert(
            pos,
            Row {
                original: None,
                copy_from: None,
                name: String::new(),
                kind: EntryKind::File,
            },
        );
        self.row = pos;
        self.col = 0;
        self.mode = Mode::Insert;
    }
    pub fn yank_line(&mut self) {
        self.register = self.selected().cloned().map(Register::Copy);
    }
    pub fn cut_line(&mut self) {
        self.register = self
            .selected()
            .cloned()
            .filter(|row| row.original.is_some())
            .map(Register::Cut);
    }
    pub fn cut_pending(&self) -> bool {
        matches!(self.register.as_ref(), Some(Register::Cut(_)))
    }
    pub fn forget_deleted(&mut self, path: &Path) {
        let refers_to_deleted = match &self.register {
            Some(Register::Copy(row) | Register::Cut(row)) => row
                .original
                .as_ref()
                .is_some_and(|source| source.starts_with(path)),
            Some(Register::Rows(rows, _)) => rows.iter().any(|row| {
                row.original
                    .as_ref()
                    .is_some_and(|source| source.starts_with(path))
            }),
            Some(Register::Text(_)) => false,
            None => false,
        };
        if refers_to_deleted {
            self.register = None;
        }
    }
    pub fn paste(&mut self, above: bool, base: &Path) {
        let Some(register) = self.register.clone() else {
            return;
        };
        if let Register::Text(text) = &register {
            if self.selected().is_some() && !text.contains('\n') {
                self.snapshot();
                let name = &mut self.rows[self.row].name;
                let at = (self.col + usize::from(!above)).min(name.chars().count());
                name.insert_str(byte_index(name, at), text);
                self.col = at + text.chars().count().saturating_sub(1);
            }
            return;
        }
        if let Register::Rows(rows, cut) = &register {
            if rows.is_empty() {
                return;
            }
            self.snapshot();
            let pos = if above {
                self.row
            } else {
                (self.row + 1).min(self.rows.len())
            };
            for (offset, source) in rows.iter().enumerate() {
                let mut row = source.clone();
                if !cut {
                    row.copy_from = row.original.take().or(row.copy_from);
                }
                self.rows.insert(pos + offset, row);
            }
            if *cut {
                self.register = None;
            }
            self.row = pos;
            self.col = 0;
            return;
        }
        let (mut row, cut) = match register {
            Register::Copy(row) => (row, false),
            Register::Cut(row) => (row, true),
            _ => unreachable!(),
        };
        if cut
            && row
                .original
                .as_ref()
                .is_some_and(|path| path.parent() == Some(base))
        {
            self.register = None;
            return;
        }
        self.snapshot();
        if cut {
            self.register = None;
        } else {
            row.copy_from = row.original.take().or(row.copy_from);
        }
        let pos = if above {
            self.row
        } else {
            (self.row + 1).min(self.rows.len())
        };
        self.rows.insert(pos, row);
        self.row = pos;
        self.col = 0;
    }
    pub fn undo(&mut self) {
        self.marked.clear();
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.rows.clone());
            self.rows = previous;
            self.clamp();
            self.dirty = !self.undo.is_empty();
        }
    }
    pub fn redo(&mut self) {
        self.marked.clear();
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.rows.clone());
            self.rows = next;
            self.clamp();
            self.dirty = true;
        }
    }
    pub fn plan(&self, base: &Path, original: &[Row]) -> io::Result<Plan> {
        let mut targets = HashSet::new();
        let present: HashSet<_> = self
            .rows
            .iter()
            .filter_map(|r| r.original.clone())
            .collect();
        let deletes: Vec<_> = original
            .iter()
            .filter_map(|r| r.original.as_ref())
            .filter(|p| !present.contains(*p))
            .cloned()
            .collect();
        let mut renames = Vec::new();
        let mut creates = Vec::new();
        let mut copies = Vec::new();
        for row in &self.rows {
            let target = match &row.original {
                Some(source)
                    if source
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy() == row.name) =>
                {
                    if source.parent() == Some(base) {
                        source.clone()
                    } else {
                        base.join(source.file_name().unwrap())
                    }
                }
                _ => resolve_target(base, &row.name)?,
            };
            if !targets.insert(target.clone()) {
                return Err(io::Error::other(format!(
                    "Duplicate destination: {}",
                    target.display()
                )));
            }
            match &row.original {
                Some(source) if *source != target => renames.push((source.clone(), target)),
                None if row.copy_from.is_some() => {
                    let source = row.copy_from.clone().unwrap();
                    if fs::symlink_metadata(&source)?.is_dir() && target.starts_with(&source) {
                        return Err(io::Error::other("Cannot copy a directory into itself"));
                    }
                    copies.push((source, target))
                }
                None => creates.push((target, row.kind)),
                _ => {}
            }
        }
        // Refuse overwriting an unrelated existing path; renamed sources are staged below.
        let moving: HashSet<_> = renames.iter().map(|(from, _)| from.clone()).collect();
        for (_, target) in &renames {
            if fs::symlink_metadata(target).is_ok() && !moving.contains(target) {
                return Err(io::Error::other(format!(
                    "Already exists: {}",
                    target.display()
                )));
            }
        }
        for (target, _) in &creates {
            if fs::symlink_metadata(target).is_ok() || moving.contains(target) {
                return Err(io::Error::other(format!(
                    "Already exists: {}",
                    target.display()
                )));
            }
        }
        for (_, target) in &copies {
            if fs::symlink_metadata(target).is_ok() || moving.contains(target) {
                return Err(io::Error::other(format!(
                    "Already exists: {}",
                    target.display()
                )));
            }
        }
        for source in original.iter().filter_map(|r| r.original.as_ref()) {
            if fs::symlink_metadata(source).is_err() {
                return Err(io::Error::other(format!(
                    "Changed on disk: {}",
                    source.display()
                )));
            }
        }
        Ok(Plan {
            renames,
            creates,
            copies,
            deletes,
        })
    }
}
fn word(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}
fn byte_index(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map_or(s.len(), |(i, _)| i)
}
fn resolve_target(base: &Path, name: &str) -> io::Result<PathBuf> {
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty() || name.starts_with('/') || name.contains('\0') || name.ends_with('/') {
        return Err(io::Error::other(format!("Invalid name: {name:?}")));
    }
    let path = Path::new(name);
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Empty name"))?;
    if file_name == "." || file_name == ".." {
        return Err(io::Error::other(format!("Invalid name: {name:?}")));
    }
    let mut parent = base.join(path.parent().unwrap_or(Path::new("")));
    let mut missing = Vec::new();
    while fs::symlink_metadata(&parent).is_err() {
        let segment = parent
            .file_name()
            .ok_or_else(|| io::Error::other("Invalid path"))?;
        missing.push(segment.to_os_string());
        parent = parent
            .parent()
            .ok_or_else(|| io::Error::other("Invalid path"))?
            .to_path_buf();
    }
    parent = parent.canonicalize()?;
    if !parent.is_dir() {
        return Err(io::Error::other(format!(
            "Not a directory: {}",
            parent.display()
        )));
    }
    for segment in missing.iter().rev() {
        parent.push(segment);
    }
    Ok(parent.join(file_name))
}
#[derive(Debug)]
pub struct Plan {
    pub renames: Vec<(PathBuf, PathBuf)>,
    pub creates: Vec<(PathBuf, EntryKind)>,
    pub copies: Vec<(PathBuf, PathBuf)>,
    pub deletes: Vec<PathBuf>,
}
impl Plan {
    pub fn is_empty(&self) -> bool {
        self.renames.is_empty()
            && self.creates.is_empty()
            && self.copies.is_empty()
            && self.deletes.is_empty()
    }
    pub fn summary(&self) -> String {
        format!(
            "{} renames, {} creates, {} copies, {} deletes",
            self.renames.len(),
            self.creates.len(),
            self.copies.len(),
            self.deletes.len()
        )
    }
    pub fn apply(self) -> io::Result<()> {
        let moving: HashSet<_> = self
            .renames
            .iter()
            .map(|(source, _)| source.clone())
            .collect();
        for target in self
            .renames
            .iter()
            .map(|(_, target)| target)
            .chain(self.creates.iter().map(|(target, _)| target))
            .chain(self.copies.iter().map(|(_, target)| target))
        {
            if fs::symlink_metadata(target).is_ok() && !moving.contains(target) {
                return Err(io::Error::other(format!(
                    "Already exists: {}",
                    target.display()
                )));
            }
        }
        let mut created_parents = HashSet::new();
        for target in self
            .renames
            .iter()
            .map(|(_, to)| to)
            .chain(self.creates.iter().map(|(to, _)| to))
            .chain(self.copies.iter().map(|(_, to)| to))
        {
            let mut missing = Vec::new();
            let mut parent = target
                .parent()
                .ok_or_else(|| io::Error::other("Path has no parent directory"))?;
            while fs::symlink_metadata(parent).is_err() {
                missing.push(parent.to_path_buf());
                parent = parent
                    .parent()
                    .ok_or_else(|| io::Error::other("Path has no parent directory"))?;
            }
            if !parent.is_dir() {
                return Err(io::Error::other(format!(
                    "Not a directory: {}",
                    parent.display()
                )));
            }
            for directory in missing.iter().rev() {
                fs::create_dir(directory)?;
                created_parents.insert(directory.clone());
            }
        }
        let mut copied: Vec<PathBuf> = Vec::new();
        for (source, target) in &self.copies {
            if let Err(error) = copy_staged(source, target) {
                for path in &copied {
                    remove_if_exists(path);
                }
                return Err(error);
            }
            copied.push(target.clone());
        }
        // Stage every source before installing destinations, including rename cycles.
        let mut staged: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new();
        for (source, target) in &self.renames {
            let stage = || -> io::Result<PathBuf> {
                let parent = source
                    .parent()
                    .ok_or_else(|| io::Error::other("Path has no parent directory"))?;
                let temp_path = if fs::symlink_metadata(source)?.is_dir() {
                    tempfile::Builder::new()
                        .prefix(".salt-rename-")
                        .tempdir_in(parent)?
                        .keep()
                } else {
                    let temp = tempfile::Builder::new()
                        .prefix(".salt-rename-")
                        .tempfile_in(parent)?;
                    temp.keep().map_err(|e| e.error)?.1
                };
                if let Err(error) = fs::rename(source, &temp_path) {
                    remove_if_exists(&temp_path);
                    return Err(error);
                }
                Ok(temp_path)
            };
            match stage() {
                Ok(temp) => staged.push((source.clone(), temp, target.clone())),
                Err(error) => {
                    let rollback = rollback_renames(&staged, 0);
                    for path in &copied {
                        remove_if_exists(path);
                    }
                    return Err(with_rollback(error, rollback));
                }
            }
        }
        for (index, (_, temp, target)) in staged.iter().enumerate() {
            if let Err(error) = fs::rename(temp, target) {
                let rollback = rollback_renames(&staged, index);
                for path in &copied {
                    remove_if_exists(path);
                }
                return Err(with_rollback(error, rollback));
            }
        }
        let mut created: Vec<PathBuf> = Vec::new();
        for (target, kind) in &self.creates {
            let result = match kind {
                EntryKind::Directory if created_parents.contains(target) => Ok(()),
                EntryKind::Directory => fs::create_dir(target),
                _ => fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)
                    .map(|_| ()),
            };
            if let Err(error) = result {
                for path in &created {
                    remove_if_exists(path);
                }
                let rollback = rollback_renames(&staged, staged.len());
                for path in &copied {
                    remove_if_exists(path);
                }
                return Err(with_rollback(error, rollback));
            }
            created.push(target.clone());
        }
        for source in &self.deletes {
            delete_disk(source)?;
        }
        Ok(())
    }
}
fn rollback_renames(staged: &[(PathBuf, PathBuf, PathBuf)], committed: usize) -> io::Result<()> {
    for (_, temp, target) in staged.iter().take(committed) {
        fs::rename(target, temp)?;
    }
    for (source, temp, _) in staged.iter().rev() {
        fs::rename(temp, source)?;
    }
    Ok(())
}
fn with_rollback(error: io::Error, rollback: io::Result<()>) -> io::Error {
    match rollback {
        Ok(()) => error,
        Err(rollback_error) => {
            io::Error::other(format!("{error}; rollback failed: {rollback_error}"))
        }
    }
}
fn remove_if_exists(path: &Path) {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.is_dir() {
            let _ = fs::remove_dir_all(path);
        } else {
            let _ = fs::remove_file(path);
        }
    }
}
pub fn delete_disk(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}
fn copy_staged(source: &Path, target: &Path) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("Path has no parent directory"))?;
    let staging = tempfile::Builder::new()
        .prefix(".salt-copy-")
        .tempdir_in(parent)?;
    let payload = staging.path().join("payload");
    copy_recursive(source, &payload)?;
    if fs::symlink_metadata(target).is_ok() {
        return Err(io::Error::other(format!(
            "Already exists: {}",
            target.display()
        )));
    }
    fs::rename(payload, target)
}
fn copy_recursive(source: &Path, target: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(source)?;
    if meta.is_dir() {
        fs::create_dir(target)?;
        for item in fs::read_dir(source)? {
            let item = item?;
            copy_recursive(&item.path(), &target.join(item.file_name()))?;
        }
    } else if meta.file_type().is_symlink() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(fs::read_link(source)?, target)?;
        #[cfg(not(unix))]
        return Err(io::Error::other(
            "Copiar enlaces no disponible en este sistema",
        ));
    } else {
        let mut input = fs::File::open(source)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        io::copy(&mut input, &mut output)?;
        fs::set_permissions(target, meta.permissions())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renames_directory_without_losing_contents() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("old")).unwrap();
        fs::write(root.path().join("old/file.txt"), "data").unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut buffer = Buffer::from_browser(&browser);
        buffer.rows[0].name = "new".into();
        buffer
            .plan(browser.path(), &original.rows)
            .unwrap()
            .apply()
            .unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("new/file.txt")).unwrap(),
            "data"
        );
    }
    #[test]
    fn visual_line_change_keeps_original_file_contents() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("old.txt"), "important").unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut buffer = Buffer::from_browser(&browser);
        buffer.start_visual(Mode::VisualLine);
        buffer.visual_operator('c');
        for character in "new.txt".chars() {
            buffer.insert(character);
        }
        buffer.leave_insert();
        let plan = buffer.plan(browser.path(), &original.rows).unwrap();
        assert!(plan.deletes.is_empty());
        plan.apply().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("new.txt")).unwrap(),
            "important"
        );
    }
    #[test]
    fn delete_word_on_empty_new_row_is_safe() {
        let root = tempfile::tempdir().unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let mut buffer = Buffer::from_browser(&browser);
        buffer.open_line(false);
        buffer.leave_insert();
        buffer.delete_word(false);
        assert_eq!(buffer.rows[0].name, "");
    }
    #[test]
    fn rejects_copying_directory_into_itself() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("dir")).unwrap();
        fs::write(root.path().join("dir/file.txt"), "data").unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut buffer = Buffer::from_browser(&browser);
        buffer.yank_line();
        buffer.paste(false, browser.path());
        buffer.rows[1].name = "dir/nested".into();
        assert!(buffer.plan(browser.path(), &original.rows).is_err());
    }
    #[test]
    fn creates_nested_file_and_parent_directory() {
        let root = tempfile::tempdir().unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.open_line(false);
        for c in "test/test1.txt".chars() {
            b.insert(c);
        }
        let plan = b.plan(browser.path(), &original.rows).unwrap();
        assert!(!root.path().join("test").exists());
        plan.apply().unwrap();
        assert!(root.path().join("test/test1.txt").is_file());
    }
    #[test]
    fn visual_line_delete_is_deferred_and_undoable() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a"), "data").unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.start_visual(Mode::VisualLine);
        b.visual_operator('d');
        assert!(root.path().join("a").exists());
        assert_eq!(
            b.plan(browser.path(), &original.rows)
                .unwrap()
                .deletes
                .len(),
            1
        );
        b.undo();
        assert_eq!(b.rows.len(), 1);
        assert!(root.path().join("a").exists());
    }
    #[test]
    fn visual_block_changes_columns_across_names() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("abc"), "").unwrap();
        fs::write(root.path().join("axc"), "").unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let mut b = Buffer::from_browser(&browser);
        b.col = 1;
        b.start_visual(Mode::VisualBlock);
        b.down();
        b.visual_operator('d');
        assert_eq!(
            b.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["ac", "ac"]
        );
        b.undo();
        assert_eq!(
            b.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["abc", "axc"]
        );
    }
    #[test]
    fn visual_block_change_inserts_on_each_selected_row() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("abc"), "").unwrap();
        fs::write(root.path().join("axc"), "").unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let mut b = Buffer::from_browser(&browser);
        b.col = 1;
        b.start_visual(Mode::VisualBlock);
        b.down();
        b.visual_operator('c');
        b.insert('Z');
        b.leave_insert();
        assert_eq!(
            b.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["aZc", "aZc"]
        );
    }
    #[test]
    fn edit_rename_and_create() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("old.rs"), "contents").unwrap();
        let browser = Browser::new(dir.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.rows[0].name = "new.rs".into();
        b.open_line(false);
        b.insert('x');
        b.mode = Mode::Normal;
        let plan = b.plan(browser.path(), &original.rows).unwrap();
        plan.apply().unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("new.rs")).unwrap(),
            "contents"
        );
        assert!(dir.path().join("x").exists());
    }
    #[test]
    fn rejects_collisions_and_plans_deletion() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "").unwrap();
        let browser = Browser::new(dir.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.open_line(false);
        b.insert('a');
        assert!(b.plan(browser.path(), &original.rows).is_err());
        b.rows.remove(1);
        b.rows.remove(0);
        let plan = b.plan(browser.path(), &original.rows).unwrap();
        assert_eq!(plan.deletes, vec![browser.path().join("a")]);
        assert!(dir.path().join("a").exists());
    }
    #[test]
    fn copies_and_deletes_after_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "data").unwrap();
        let browser = Browser::new(dir.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.yank_line();
        b.paste(false, browser.path());
        b.rows[1].name = "b".into();
        let plan = b.plan(browser.path(), &original.rows).unwrap();
        plan.apply().unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("b")).unwrap(), "data");
        let browser = Browser::new(dir.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.rows.remove(0);
        let plan = b.plan(browser.path(), &original.rows).unwrap();
        assert_eq!(plan.deletes.len(), 1);
        plan.apply().unwrap();
        assert!(!dir.path().join("a").exists());
    }
    #[test]
    fn vim_motions_edit_unicode_name_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("café.txt"), "").unwrap();
        let browser = Browser::new(dir.path().to_path_buf()).unwrap();
        let mut b = Buffer::from_browser(&browser);
        b.word_forward();
        assert_eq!(b.col, 4);
        b.word_back();
        assert_eq!(b.col, 0);
        b.right();
        b.right();
        b.replace_char('X');
        assert_eq!(b.rows[0].name, "caXé.txt");
        b.undo();
        assert_eq!(b.rows[0].name, "café.txt");
        assert!(!b.dirty);
    }
    #[test]
    fn moves_to_sibling_directory_and_creates_folder() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("from")).unwrap();
        fs::create_dir(root.path().join("to")).unwrap();
        fs::write(root.path().join("from/a"), "data").unwrap();
        let browser = Browser::new(root.path().join("from")).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.rows[0].name = "../to/a".into();
        b.open_line(false);
        for c in "new/".chars() {
            b.insert(c);
        }
        assert_eq!(b.rows[1].kind, EntryKind::Directory);
        let plan = b.plan(browser.path(), &original.rows).unwrap();
        plan.apply().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("to/a")).unwrap(),
            "data"
        );
        assert!(root.path().join("from/new").is_dir());
    }
    #[test]
    fn yank_survives_directory_change_for_cross_directory_copy() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("one")).unwrap();
        fs::create_dir(root.path().join("two")).unwrap();
        fs::write(root.path().join("one/a"), "data").unwrap();
        let mut browser = Browser::new(root.path().join("one")).unwrap();
        let mut b = Buffer::from_browser(&browser);
        b.yank_line();
        browser.navigate_to(&root.path().join("two")).unwrap();
        b.load(&browser);
        let original = b.rows.clone();
        b.paste(false, browser.path());
        b.plan(browser.path(), &original).unwrap().apply().unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("two/a")).unwrap(),
            "data"
        );
    }
    #[test]
    fn rename_and_nested_creation_apply_together() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a"), "data").unwrap();
        let plan = Plan {
            renames: vec![(root.path().join("a"), root.path().join("b"))],
            creates: vec![(root.path().join("missing/new"), EntryKind::File)],
            copies: vec![],
            deletes: vec![],
        };
        plan.apply().unwrap();
        assert!(!root.path().join("a").exists());
        assert_eq!(fs::read_to_string(root.path().join("b")).unwrap(), "data");
        assert!(root.path().join("missing/new").is_file());
    }
    #[test]
    fn cut_without_paste_does_not_change_disk_or_plan() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a"), "data").unwrap();
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut b = Buffer::from_browser(&browser);
        b.cut_line();
        assert!(!b.dirty);
        assert!(b.plan(browser.path(), &original.rows).unwrap().is_empty());
        assert_eq!(fs::read_to_string(root.path().join("a")).unwrap(), "data");
    }
    #[test]
    fn cut_and_paste_moves_only_on_save() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("from")).unwrap();
        fs::create_dir(root.path().join("to")).unwrap();
        fs::write(root.path().join("from/a"), "data").unwrap();
        let mut browser = Browser::new(root.path().join("from")).unwrap();
        let mut b = Buffer::from_browser(&browser);
        b.cut_line();
        browser.navigate_to(&root.path().join("to")).unwrap();
        b.load(&browser);
        let original = b.rows.clone();
        b.paste(false, browser.path());
        assert!(b.dirty);
        assert!(root.path().join("from/a").exists());
        assert!(!root.path().join("to/a").exists());
        let plan = b.plan(browser.path(), &original).unwrap();
        assert_eq!(plan.renames.len(), 1);
        plan.apply().unwrap();
        assert!(!root.path().join("from/a").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("to/a")).unwrap(),
            "data"
        );
    }
    #[test]
    fn confirmed_delete_removes_directory_contents() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("folder")).unwrap();
        fs::write(root.path().join("folder/file"), "data").unwrap();
        delete_disk(&root.path().join("folder")).unwrap();
        assert!(!root.path().join("folder").exists());
    }
    #[test]
    fn space_marks_nonadjacent_rows_and_deletion_is_undoable() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b", "c", "d"] {
            fs::write(root.path().join(name), name).unwrap();
        }
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let original = Buffer::from_browser(&browser);
        let mut buffer = Buffer::from_browser(&browser);
        buffer.toggle_mark_current();
        buffer.row = 2;
        buffer.toggle_mark_current();
        assert_eq!(buffer.marked.iter().copied().collect::<Vec<_>>(), [0, 2]);
        buffer.rows_operator(&[0, 2], 'd');
        assert!(buffer.marked.is_empty());
        assert_eq!(
            buffer
                .rows
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["b", "d"]
        );
        assert_eq!(
            buffer
                .plan(browser.path(), &original.rows)
                .unwrap()
                .deletes
                .len(),
            2
        );
        assert!(root.path().join("a").exists());
        buffer.undo();
        assert_eq!(buffer.rows.len(), 4);
    }
    #[test]
    fn nonadjacent_row_copy_pastes_both_files_in_another_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("from")).unwrap();
        fs::create_dir(root.path().join("to")).unwrap();
        for name in ["a", "b", "c"] {
            fs::write(root.path().join("from").join(name), name).unwrap();
        }
        let mut browser = Browser::new(root.path().join("from")).unwrap();
        let mut buffer = Buffer::from_browser(&browser);
        buffer.rows_operator(&[0, 2], 'y');
        browser.navigate_to(&root.path().join("to")).unwrap();
        buffer.load(&browser);
        let original = buffer.rows.clone();
        buffer.paste(false, browser.path());
        assert_eq!(
            buffer.plan(browser.path(), &original).unwrap().copies.len(),
            2
        );
        buffer
            .plan(browser.path(), &original)
            .unwrap()
            .apply()
            .unwrap();
        assert_eq!(fs::read_to_string(root.path().join("to/a")).unwrap(), "a");
        assert_eq!(fs::read_to_string(root.path().join("to/c")).unwrap(), "c");
        assert!(!root.path().join("to/b").exists());
    }
}

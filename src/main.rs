mod archive;
mod config;
mod edit;
mod local_search;
mod model;
mod open;
mod sort;
mod zoxide;

use config::AppConfig;
use crossterm::{
    cursor::Show,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use edit::{Buffer, Mode, Plan};
use model::{Browser, EntryKind};
use open::{Choice, OpenConfig};
use ratatui::{
    Terminal,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use sort::{SortBy, SortSpec};
use std::{
    collections::HashSet,
    env, io,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum Icons {
    Auto,
    Always,
    Never,
}
impl Icons {
    fn enabled(self) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            // Font detection is unreliable; auto requires an explicit terminal hint.
            Self::Auto => env::var("SALT_NERD_FONT").is_ok_and(|v| v == "1"),
        }
    }
}

fn args() -> Result<(PathBuf, Icons), String> {
    let mut path = None;
    let mut icons = Icons::Always;
    let mut iter = env::args_os().skip(1);
    while let Some(arg) = iter.next() {
        let s = arg.to_string_lossy();
        if s == "--help" || s == "-h" {
            println!(
                "Usage: salt [--icons auto|always|never] [PATH]\nIcons are enabled by default. Use --icons never to disable them. Set SALT_NERD_FONT=1 to enable icons in auto mode."
            );
            std::process::exit(0);
        } else if s == "--icons" {
            icons = parse_icons(
                &iter
                    .next()
                    .ok_or("--icons requires a value")?
                    .to_string_lossy(),
            )?;
        } else if let Some(value) = s.strip_prefix("--icons=") {
            icons = parse_icons(value)?;
        } else if s.starts_with('-') {
            return Err(format!("Unknown option: {s}"));
        } else if path.replace(PathBuf::from(arg)).is_some() {
            return Err("Only one path is supported".into());
        }
    }
    Ok((
        path.unwrap_or(env::current_dir().map_err(|e| e.to_string())?),
        icons,
    ))
}
fn parse_icons(value: &str) -> Result<Icons, String> {
    match value {
        "auto" => Ok(Icons::Auto),
        "always" => Ok(Icons::Always),
        "never" => Ok(Icons::Never),
        _ => Err(format!("Invalid icon mode: {value}")),
    }
}
fn icon(kind: EntryKind, enabled: bool) -> &'static str {
    if !enabled {
        return "";
    }
    match kind {
        EntryKind::Directory => "\u{f07b} ",
        EntryKind::File => "\u{f15b} ",
        EntryKind::Symlink => "\u{f0c1} ",
    }
}

const HELP_LINES: &[&str] = &[
    "NAVIGATION",
    "  j / k, ↑ / ↓       next / previous row",
    "  h / l, ← / →        move within a name",
    "  w / b / e           next / previous / end of word",
    "  0 / $               start / end of name",
    "  gg / G              first / last row",
    "  L / Enter           enter directory or open file",
    "  H / - / Backspace   parent directory",
    "  z                   jump with zoxide and fzf",
    "  Z                   find a local path with fzf",
    "  /                   search names in this directory",
    "  g.                  toggle hidden entries",
    "  ,a / ,A             sort name A–Z / Z–A",
    "  ,m / ,M             sort oldest / newest first",
    "  ,s / ,S             sort smallest / largest first",
    "",
    "EDITING NAMES",
    "  i / a               insert before / after cursor",
    "  I / A               insert at start / end",
    "  o / O               new row below / above",
    "  Esc                 return to normal mode",
    "  x / X               delete current / previous character",
    "  s / r               substitute / replace character",
    "  C / D               change / delete to end of name",
    "  cc / cw / dw        change row / word; delete word",
    "  u / Ctrl+r          undo / redo buffer edits",
    "  v / V / Ctrl+v      select characters / rows / block",
    "  Space               mark separate rows; Space again unmarks",
    "  in visual: d/c/y    delete / change / copy selection",
    "  in visual: x/s      delete / substitute selection",
    "  name/               create a directory (new row)",
    "  ../other/name       move to another local directory",
    "",
    "FILES AND REGISTERS",
    "  yy                  copy row to Salt's register",
    "  xx                  mark for move; no paste means no change",
    "  p / P               paste below / above; :w applies move",
    "  dd                  confirm and delete immediately",
    "  gx                  choose a configured opener",
    "  gc                  create .zip (in V: selected rows)",
    "  with marks: d/x/y    delete / copy selected rows",
    "  with marks: gc       compress selected rows",
    "  g/d/c/y/r/x         show available next keys",
    "  ,                   show sorting options",
    "",
    "SAVE AND QUIT",
    "  :w / :wq            save / save and quit",
    "                      deletions require confirmation",
    "  :e!                 discard edits and reload",
    "  q / :q              quit when no edits are pending",
    "  :q!                 quit without saving",
    "  ?                   show / close this help",
];

fn prefix_hints(prefix: char) -> &'static [(&'static str, &'static str)] {
    match prefix {
        'g' => &[
            ("g", "first row"),
            (".", "toggle hidden entries"),
            ("x", "open with…"),
            ("c", "create .zip"),
        ],
        'd' => &[("d", "delete file or directory"), ("w", "delete word")],
        'c' => &[("c", "change entire name"), ("w", "change word")],
        'y' => &[("y", "copy row")],
        'r' => &[("character", "replace character")],
        'x' => &[
            ("x", "mark file or directory for move"),
            ("wait", "delete character"),
        ],
        ',' => &[
            ("a / A", "name A–Z / Z–A"),
            ("m / M", "oldest / newest first"),
            ("s / S", "smallest / largest first"),
        ],
        _ => &[],
    }
}

fn visual_hints(mode: Mode, multiple_rows: bool) -> &'static [(&'static str, &'static str)] {
    match mode {
        Mode::VisualLine if multiple_rows => &[
            ("d / x", "delete selected rows"),
            ("y", "copy selected rows"),
            ("g c", "compress selected rows"),
            ("Esc", "cancel selection"),
        ],
        Mode::VisualLine => &[
            ("d / x", "delete selected row"),
            ("c / s", "change selected row"),
            ("y", "copy selected row"),
            ("g c", "compress selected row"),
            ("Esc", "cancel selection"),
        ],
        Mode::Visual | Mode::VisualBlock => &[
            ("d / x", "delete selected text"),
            ("c / s", "change selected text"),
            ("y", "copy selected text"),
            ("Esc", "cancel selection"),
        ],
        _ => &[],
    }
}

fn marked_hints(multiple_rows: bool) -> &'static [(&'static str, &'static str)] {
    if multiple_rows {
        &[
            ("Space", "mark or unmark current row"),
            ("d / x", "delete marked rows"),
            ("y", "copy marked rows"),
            ("g c", "compress marked rows"),
            ("Esc", "clear marks"),
        ]
    } else {
        &[
            ("Space", "mark or unmark current row"),
            ("d / x", "delete marked row"),
            ("c / s", "change marked row"),
            ("y", "copy marked row"),
            ("g c", "compress marked row"),
            ("Esc", "clear marks"),
        ]
    }
}

fn apply_sort(
    browser: &mut Browser,
    edit: &mut Buffer,
    original: &mut Vec<edit::Row>,
    spec: SortSpec,
) -> Result<(), String> {
    if edit.dirty {
        return Err("Save or discard changes before sorting".into());
    }
    let marked: HashSet<_> = edit
        .marked
        .iter()
        .filter_map(|index| edit.rows.get(*index).and_then(|row| row.original.clone()))
        .collect();
    browser.select(edit.row);
    browser.set_sort(spec).map_err(|error| error.to_string())?;
    edit.load(browser);
    edit.marked = edit
        .rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            row.original
                .as_ref()
                .filter(|path| marked.contains(*path))
                .map(|_| index)
        })
        .collect();
    *original = edit.rows.clone();
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (path, icons) = args().map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut browser = Browser::new(path)?;
    let config = AppConfig::load()?;
    browser.set_sort_config(config.sort)?;
    let open_config = config.open;
    enable_raw_mode()?;
    let mut screen = ScreenGuard {
        alternate: false,
        active: true,
    };
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    screen.alternate = true;
    let mut terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;
    let result = run(&mut terminal, &mut browser, &open_config, icons);
    drop(terminal);
    let restore = screen.restore();
    result?;
    restore?;
    Ok(())
}

struct ScreenGuard {
    alternate: bool,
    active: bool,
}

impl ScreenGuard {
    fn restore(&mut self) -> io::Result<()> {
        let raw_result = disable_raw_mode();
        let screen_result = if self.alternate {
            execute!(io::stdout(), LeaveAlternateScreen, Show)
        } else {
            execute!(io::stdout(), Show)
        };
        if screen_result.is_ok() {
            self.alternate = false;
        }
        let result = raw_result.and(screen_result);
        if result.is_ok() {
            self.active = false;
        }
        result
    }
}

impl Drop for ScreenGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = self.restore();
        }
    }
}
fn run(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    browser: &mut Browser,
    open_config: &OpenConfig,
    icons: Icons,
) -> io::Result<()> {
    let mut edit = Buffer::from_browser(browser);
    let mut original = edit.rows.clone();
    let mut command: Option<String> = None;
    let mut search: Option<(String, usize, usize)> = None;
    let mut archive_input: Option<(Vec<PathBuf>, String)> = None;
    let mut marked_g_pending = false;
    let mut status = String::new();
    let mut pending_plan: Option<Plan> = None;
    let mut pending_delete: Option<PathBuf> = None;
    let mut pending_x_since: Option<Instant> = None;
    let mut queued_key: Option<KeyEvent> = None;
    let mut help_open = false;
    let mut help_scroll = 0usize;
    let mut quit_after_save = false;
    let (launch_errors, pending_errors) = mpsc::channel::<String>();
    let mut menu: Option<(PathBuf, Vec<Choice>, usize)> = None;
    loop {
        while let Ok(error) = pending_errors.try_recv() {
            status = error;
        }
        terminal.draw(|frame| {
            let chunks = Layout::default().direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
                .split(frame.area());
            frame.render_widget(Paragraph::new(format!("{} · {}", browser.path().display(), browser.sort().label()))
                .style(Style::default().add_modifier(Modifier::BOLD)), chunks[0]);
            let rows: Vec<_> = edit.rows.iter().enumerate().map(|(i, row)| {
                let suffix = if row.kind == EntryKind::Directory && !row.name.ends_with('/') { "/" } else { "" };
                let prefix = icon(row.kind, icons.enabled());
                let marked = edit.marked.contains(&i);
                let marker = if marked { "[x] " } else { "    " };
                let mut spans = vec![Span::raw(marker), Span::raw(prefix.to_string())];
                let selection = edit.visual_range(i);
                for (col, ch) in row.name.chars().enumerate() {
                    let selected = selection.is_some_and(|(start, end)| col >= start && col < end);
                    let style = if i == edit.row && col == edit.col {
                        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
                    } else if selected || marked {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else { Style::default() };
                    spans.push(Span::styled(ch.to_string(), style));
                }
                if i == edit.row && edit.col >= row.name.chars().count() {
                    spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)));
                }
                spans.push(Span::raw(suffix.to_string()));
                let line = Line::from(spans);
                ListItem::new(line)
            }).collect();
            let list = List::new(rows);
            let mut state = ListState::default().with_selected((!edit.rows.is_empty()).then_some(edit.row));
            frame.render_stateful_widget(list, chunks[1], &mut state);
            let help = if help_open { "Help · j/k scroll · ?/Esc/q close".to_string() }
                else if menu.is_some() { "j/k choose · Enter open · Esc cancel".to_string() }
                else if let Some((paths, name)) = &archive_input { format!("Compress {} item(s) · name: {} · Enter create · Esc cancel", paths.len(), name) }
                else if pending_delete.is_some() { "Delete permanently? y yes · n/Esc cancel".to_string() }
                else if pending_plan.is_some() { "Confirm deletions? y yes · n/Esc cancel".to_string() }
                else if let Some(cmd) = &command { format!(":{cmd}") }
                else if let Some((query, _, _)) = &search { format!("/{query} · Enter accept · Esc cancel") }
                else if !status.is_empty() { status.clone() }
                else { format!("{:?}{}{} · ? help · , sort · Space mark · V select · j/k rows · h/l cursor · H/L folders · :w save",
                    edit.mode, if edit.dirty { " [+]" } else { "" }, if edit.cut_pending() { " [pending cut]" } else { "" }) };
            let notice_visible = !status.is_empty() && !help_open && menu.is_none()
                && pending_delete.is_none() && pending_plan.is_none()
                && command.is_none() && search.is_none();
            let footer_style = if notice_visible {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else { Style::default() };
            frame.render_widget(Paragraph::new(help).style(footer_style), chunks[2]);
            if let Some((path, choices, selected)) = &menu {
                let width = frame.area().width.saturating_sub(4).clamp(20, 60).min(frame.area().width);
                let height = (choices.len() as u16 + 2).min(frame.area().height);
                let popup = Rect::new(frame.area().x + frame.area().width.saturating_sub(width) / 2,
                    frame.area().y + frame.area().height.saturating_sub(height) / 2, width, height);
                frame.render_widget(Clear, popup);
                let title = format!("Open with: {}", path.file_name().unwrap_or_default().to_string_lossy());
                let rows: Vec<_> = choices.iter().map(|choice| ListItem::new(choice.desc.as_str())).collect();
                let list = List::new(rows).block(Block::default().title(title).borders(Borders::ALL))
                    .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
                let mut state = ListState::default().with_selected(Some(*selected));
                frame.render_stateful_widget(list, popup, &mut state);
            }
            let hints = if marked_g_pending {
                Some(("g · marked rows".to_string(), &[("c", "compress marked rows")][..]))
            } else if let Some(prefix) = edit.pending {
                let choices = if edit.mode == Mode::VisualLine {
                    &[("c", "compress selection as .zip")][..]
                } else {
                    prefix_hints(prefix)
                };
                Some((format!("{prefix} · next keys · Esc cancels"), choices))
            } else if matches!(edit.mode, Mode::Visual | Mode::VisualLine | Mode::VisualBlock) {
                let multiple_rows = edit.visual_anchor.is_some_and(|(row, _)| row != edit.row);
                let mode = match edit.mode {
                    Mode::Visual => "v",
                    Mode::VisualLine => "V",
                    Mode::VisualBlock => "Ctrl+v",
                    _ => unreachable!(),
                };
                Some((format!("{mode} · selection"), visual_hints(edit.mode, multiple_rows)))
            } else if !edit.marked.is_empty() {
                Some((format!("Space · {} marked", edit.marked.len()), marked_hints(edit.marked.len() > 1)))
            } else {
                None
            };
            if let Some((title, hints)) = hints.filter(|(_, choices)| {
                !choices.is_empty() && !help_open && menu.is_none() && archive_input.is_none()
            }) {
                    let area = frame.area();
                    let width = area.width.saturating_sub(2).min(48);
                    let height = (hints.len() as u16 + 2).min(area.height.saturating_sub(1));
                    if width >= 12 && height >= 3 {
                        let x = if edit.pending.is_some() || marked_g_pending { area.x + 1 } else { area.right().saturating_sub(width + 1) };
                        let popup = Rect::new(x, area.bottom().saturating_sub(height + 1), width, height);
                        let lines: Vec<Line<'_>> = hints.iter().map(|(key, action)| {
                            Line::from(format!("  {key:<9} {action}"))
                        }).collect();
                        frame.render_widget(Clear, popup);
                        frame.render_widget(Paragraph::new(lines)
                            .block(Block::default().title(title)
                                .borders(Borders::ALL)), popup);
                    }
            }
            if let Some(plan) = &pending_plan {
                let area = frame.area();
                let popup = Rect::new(area.x + area.width / 6, area.y + area.height / 3,
                    area.width * 2 / 3, 5.min(area.height));
                frame.render_widget(Clear, popup);
                let details = plan.renames.iter().map(|(a,b)| format!("{} → {}", a.display(), b.display()))
                    .chain(plan.creates.iter().map(|(p,_)| format!("create {}", p.display())))
                    .chain(plan.copies.iter().map(|(a,b)| format!("copy {} → {}", a.display(), b.display())))
                    .chain(plan.deletes.iter().map(|p| format!("delete {}", p.display())))
                    .take(3).collect::<Vec<_>>().join("\n");
                frame.render_widget(Paragraph::new(format!("{}\n{}", plan.summary(), details))
                    .block(Block::default().title("Confirm :w deletions").borders(Borders::ALL)), popup);
            }
            if let Some(path) = &pending_delete {
                let area = frame.area();
                let popup = Rect::new(area.x + area.width / 6, area.y + area.height / 3,
                    area.width * 2 / 3, 5.min(area.height));
                frame.render_widget(Clear, popup);
                frame.render_widget(Paragraph::new(format!("Delete {} permanently?\nDirectories and their contents will be removed.\ny = delete · n/Esc = cancel", path.display()))
                    .block(Block::default().title("Confirm deletion").borders(Borders::ALL)), popup);
            }
            if let Some((paths, name)) = &archive_input {
                let area = frame.area();
                let width = area.width.saturating_sub(2).min(64);
                if width >= 20 && area.height >= 5 {
                    let popup = Rect::new(area.x + area.width.saturating_sub(width) / 2,
                        area.y + area.height.saturating_sub(5) / 2, width, 5);
                    frame.render_widget(Clear, popup);
                    frame.render_widget(Paragraph::new(format!("{} item(s) → {}\nEnter = create · Esc = cancel", paths.len(), name))
                        .block(Block::default().title("Create ZIP archive").borders(Borders::ALL)), popup);
                }
            }
            if help_open {
                let area = frame.area();
                let width = area.width.saturating_sub(2).min(86);
                let height = area.height.saturating_sub(2).min(28);
                let popup = Rect::new(area.x + area.width.saturating_sub(width) / 2,
                    area.y + area.height.saturating_sub(height) / 2, width, height);
                let visible = height.saturating_sub(2) as usize;
                let lines: Vec<Line<'_>> = HELP_LINES.iter().skip(help_scroll).take(visible)
                    .map(|line| Line::raw(*line)).collect();
                frame.render_widget(Clear, popup);
                frame.render_widget(Paragraph::new(lines)
                    .block(Block::default().title(format!("Salt help · {}/{}", help_scroll + 1, HELP_LINES.len())).borders(Borders::ALL)), popup);
            }
        })?;
        let key = if let Some(key) = queued_key.take() {
            key
        } else {
            if !event::poll(Duration::from_millis(100))? {
                if pending_x_since
                    .is_some_and(|since| since.elapsed() >= Duration::from_millis(700))
                {
                    edit.pending = None;
                    pending_x_since = None;
                    edit.delete_char();
                }
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            key
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if edit.mode == Mode::Insert {
                edit.leave_insert();
            } else {
                edit.pending = None;
                command = None;
                menu = None;
                archive_input = None;
                marked_g_pending = false;
                edit.clear_marks();
                pending_delete = None;
                pending_x_since = None;
                help_open = false;
            }
            continue;
        }
        if help_open {
            let visible = terminal.size()?.height.saturating_sub(4).min(26) as usize;
            let max_scroll = HELP_LINES.len().saturating_sub(visible.max(1));
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter => {
                    help_open = false
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    help_scroll = (help_scroll + 1).min(max_scroll)
                }
                KeyCode::Char('k') | KeyCode::Up => help_scroll = help_scroll.saturating_sub(1),
                KeyCode::PageDown => help_scroll = (help_scroll + visible).min(max_scroll),
                KeyCode::PageUp => help_scroll = help_scroll.saturating_sub(visible),
                _ => {}
            }
            continue;
        }
        if let Some(path) = pending_delete.take() {
            if key.code == KeyCode::Char('y') {
                match edit::delete_disk(&path) {
                    Ok(()) => {
                        browser.refresh()?;
                        edit.forget_deleted(&path);
                        edit.load(browser);
                        original = edit.rows.clone();
                        status = format!("Deleted: {}", path.display());
                    }
                    Err(e) => status = format!("Could not delete: {e}"),
                }
            }
            continue;
        }
        if let Some(plan) = pending_plan.take() {
            if key.code == KeyCode::Char('y') {
                let selected_name = edit.selected().map(|row| row.name.clone());
                match plan.apply() {
                    Ok(()) => {
                        browser.refresh()?;
                        if let Some(name) = selected_name {
                            browser.select_name(&name);
                        }
                        edit.load(browser);
                        original = edit.rows.clone();
                        status = "Changes saved".into();
                        if quit_after_save {
                            break;
                        }
                    }
                    Err(e) => status = format!("Could not save: {e}"),
                }
            }
            quit_after_save = false;
            continue;
        }
        if let Some((path, choices, selected)) = &mut menu {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => menu = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    *selected = (*selected + 1).min(choices.len() - 1)
                }
                KeyCode::Char('k') | KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Enter => {
                    let chosen = choices[*selected].clone();
                    let path = path.clone();
                    menu = None;
                    if let Err(e) = launch_choice(terminal, &chosen, &path, &launch_errors) {
                        status = e.to_string();
                    } else {
                        status.clear();
                    }
                }
                _ => {}
            }
            continue;
        }
        if let Some((paths, name)) = &mut archive_input {
            match key.code {
                KeyCode::Esc => archive_input = None,
                KeyCode::Backspace => {
                    name.pop();
                }
                KeyCode::Enter => {
                    let paths = paths.clone();
                    let name = name.clone();
                    archive_input = None;
                    match archive::create_zip(browser.path(), &paths, &name) {
                        Ok(path) => {
                            browser.refresh()?;
                            browser.select_name(&name);
                            edit.load(browser);
                            original = edit.rows.clone();
                            status = format!("Created: {}", path.display());
                        }
                        Err(e) => status = format!("Could not compress: {e}"),
                    }
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => name.push(c),
                _ => {}
            }
            continue;
        }
        if let Some(cmd) = &mut command {
            match key.code {
                KeyCode::Esc => command = None,
                KeyCode::Backspace => {
                    cmd.pop();
                }
                KeyCode::Enter => {
                    let entered = command.take().unwrap();
                    match entered.as_str() {
                        "q" if !edit.dirty => break,
                        "q" => status = "Unsaved changes; use :w, :wq or :q!".into(),
                        "q!" => break,
                        "e!" => {
                            browser.refresh()?;
                            edit.load(browser);
                            original = edit.rows.clone();
                            status = "Changes discarded".into();
                        }
                        "w" | "wq" => {
                            quit_after_save = entered == "wq";
                            match edit.plan(browser.path(), &original) {
                                Ok(plan) if plan.is_empty() => {
                                    status = "No changes".into();
                                    edit.dirty = false;
                                    if quit_after_save {
                                        break;
                                    }
                                }
                                Ok(plan) if !plan.deletes.is_empty() => pending_plan = Some(plan),
                                Ok(plan) => {
                                    let selected_name = edit.selected().map(|row| row.name.clone());
                                    match plan.apply() {
                                        Ok(()) => {
                                            browser.refresh()?;
                                            if let Some(name) = selected_name {
                                                browser.select_name(&name);
                                            }
                                            edit.load(browser);
                                            original = edit.rows.clone();
                                            status = "Changes saved".into();
                                            if quit_after_save {
                                                break;
                                            }
                                        }
                                        Err(e) => status = format!("Could not save: {e}"),
                                    }
                                }
                                Err(e) => status = e.to_string(),
                            }
                        }
                        _ => status = format!("Unknown command: :{entered}"),
                    }
                }
                KeyCode::Char(c) => cmd.push(c),
                _ => {}
            }
            continue;
        }
        if let Some((query, origin_row, origin_col)) = &mut search {
            match key.code {
                KeyCode::Esc => {
                    edit.row = *origin_row;
                    edit.col = *origin_col;
                    search = None;
                }
                KeyCode::Enter => {
                    search = None;
                    status.clear();
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => query.push(c),
                _ => {}
            }
            if let Some((query, origin_row, _)) = &search {
                if query.is_empty() {
                    edit.row = *origin_row;
                    edit.col = 0;
                } else if let Some(row) = search_match(&edit.rows, query, *origin_row) {
                    edit.row = row;
                    edit.col = 0;
                }
            }
            continue;
        }
        if edit.mode == Mode::Insert {
            match key.code {
                KeyCode::Esc => {
                    edit.leave_insert();
                }
                KeyCode::Backspace => edit.backspace(),
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    edit.insert(c)
                }
                _ => {}
            }
            continue;
        }
        if marked_g_pending {
            marked_g_pending = false;
            if key.code == KeyCode::Char('c') {
                let indices: Vec<_> = edit.marked.iter().copied().collect();
                match archive_rows(&edit, &indices) {
                    Ok(paths) => {
                        let name = archive::suggested_name(&paths);
                        archive_input = Some((paths, name));
                        edit.clear_marks();
                    }
                    Err(message) => status = message,
                }
            }
            continue;
        }
        if !edit.marked.is_empty() && edit.mode == Mode::Normal {
            match key.code {
                KeyCode::Esc => {
                    edit.clear_marks();
                    status.clear();
                    continue;
                }
                KeyCode::Char(' ') => {
                    edit.toggle_mark_current();
                    status.clear();
                    continue;
                }
                KeyCode::Char('d' | 'x' | 'y') => {
                    let indices: Vec<_> = edit.marked.iter().copied().collect();
                    let copy = key.code == KeyCode::Char('y');
                    edit.rows_operator(&indices, if copy { 'y' } else { 'd' });
                    edit.clear_marks();
                    status = if copy {
                        format!("Copied {} marked row(s)", indices.len())
                    } else {
                        format!(
                            "Removed {} row(s) from the buffer; use :w to save",
                            indices.len()
                        )
                    };
                    continue;
                }
                KeyCode::Char('c' | 's') => {
                    let selected = edit.marked.iter().next().copied();
                    if edit.marked.len() == 1 {
                        edit.row = selected.unwrap();
                        edit.clear_marks();
                        edit.change_line();
                    } else {
                        edit.clear_marks();
                        status = "Changing multiple whole rows is not available yet".into();
                    }
                    continue;
                }
                KeyCode::Char('g') => {
                    marked_g_pending = true;
                    continue;
                }
                _ => {}
            }
        }
        if matches!(
            edit.mode,
            Mode::Visual | Mode::VisualLine | Mode::VisualBlock
        ) {
            if edit.pending.take() == Some('g') {
                if key.code == KeyCode::Char('c') {
                    match archive_selection(&edit, true) {
                        Ok(paths) => {
                            let name = archive::suggested_name(&paths);
                            archive_input = Some((paths, name));
                            edit.stop_visual();
                        }
                        Err(message) => status = message,
                    }
                }
                continue;
            }
            match key.code {
                KeyCode::Esc => edit.stop_visual(),
                KeyCode::Char('v')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && edit.mode == Mode::VisualBlock =>
                {
                    edit.stop_visual()
                }
                KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    edit.mode = Mode::VisualBlock
                }
                KeyCode::Char('v') if edit.mode == Mode::Visual => edit.stop_visual(),
                KeyCode::Char('V') if edit.mode == Mode::VisualLine => edit.stop_visual(),
                KeyCode::Char('v') => edit.mode = Mode::Visual,
                KeyCode::Char('V') => edit.mode = Mode::VisualLine,
                KeyCode::Char('j') | KeyCode::Down => edit.down(),
                KeyCode::Char('k') | KeyCode::Up => edit.up(),
                KeyCode::Char('h') | KeyCode::Left => edit.left(),
                KeyCode::Char('l') | KeyCode::Right => edit.right(),
                KeyCode::Char('w') => edit.word_forward(),
                KeyCode::Char('b') => edit.word_back(),
                KeyCode::Char('e') => edit.word_end(),
                KeyCode::Char('0') => edit.col = 0,
                KeyCode::Char('$') => edit.end(),
                KeyCode::Char('G') => {
                    edit.row = edit.rows.len().saturating_sub(1);
                    edit.col = 0;
                }
                KeyCode::Char('d' | 'x') => edit.visual_operator('d'),
                KeyCode::Char('c' | 's') => {
                    if edit.mode == Mode::VisualLine
                        && edit.visual_anchor.is_some_and(|(row, _)| row != edit.row)
                    {
                        status = "Changing multiple whole rows is not available yet".into();
                        edit.stop_visual();
                    } else {
                        edit.visual_operator('c');
                    }
                }
                KeyCode::Char('y') => edit.visual_operator('y'),
                KeyCode::Char('g') if edit.mode == Mode::VisualLine => edit.pending = Some('g'),
                _ => {}
            }
            continue;
        }
        if let Some(prefix) = edit.pending.take() {
            match (prefix, key.code) {
                ('g', KeyCode::Char('.')) => {
                    if edit.dirty {
                        status = "Save or discard changes before toggling hidden entries".into();
                    } else if let Err(e) = browser.toggle_hidden() {
                        status = e.to_string();
                    } else {
                        edit.load(browser);
                        original = edit.rows.clone();
                        status.clear();
                    }
                }
                ('g', KeyCode::Char('x')) => {
                    if let Some(path) = edit.selected_path(browser.path()) {
                        let choices = open_config.choices(&path);
                        if !choices.is_empty() {
                            menu = Some((path, choices, 0));
                        }
                    }
                }
                ('g', KeyCode::Char('c')) => match archive_selection(&edit, false) {
                    Ok(paths) => {
                        let name = archive::suggested_name(&paths);
                        archive_input = Some((paths, name));
                    }
                    Err(message) => status = message,
                },
                ('g', KeyCode::Char('g')) => {
                    edit.row = 0;
                    edit.col = 0;
                }
                ('d', KeyCode::Char('d')) => {
                    if edit.dirty {
                        status = "Save or discard changes before deleting".into();
                    } else {
                        pending_delete = edit.selected().and_then(|row| row.original.clone());
                    }
                }
                ('x', KeyCode::Char('x')) => {
                    pending_x_since = None;
                    if edit.dirty {
                        status = "Save or discard changes before cutting".into();
                    } else {
                        edit.cut_line();
                        status = "Marked for move; paste with p/P and save with :w".into();
                    }
                }
                ('x', KeyCode::Esc) => {
                    pending_x_since = None;
                }
                ('x', _) => {
                    pending_x_since = None;
                    edit.delete_char();
                    queued_key = Some(key);
                }
                ('d', KeyCode::Char('w')) => edit.delete_word(false),
                ('c', KeyCode::Char('w')) => edit.delete_word(true),
                ('c', KeyCode::Char('c')) => edit.change_line(),
                ('y', KeyCode::Char('y')) => edit.yank_line(),
                ('r', KeyCode::Char(c)) => edit.replace_char(c),
                (',', KeyCode::Char(c @ ('a' | 'A' | 'm' | 'M' | 's' | 'S'))) => {
                    let (by, reverse) = match c {
                        'a' => (SortBy::Name, false),
                        'A' => (SortBy::Name, true),
                        'm' => (SortBy::Mtime, false),
                        'M' => (SortBy::Mtime, true),
                        's' => (SortBy::Size, false),
                        'S' => (SortBy::Size, true),
                        _ => unreachable!(),
                    };
                    let spec = SortSpec {
                        by,
                        reverse,
                        directories_first: browser.sort().directories_first,
                    };
                    status = match apply_sort(browser, &mut edit, &mut original, spec) {
                        Ok(()) => format!("Sorted by {}", spec.label()),
                        Err(message) => message,
                    };
                }
                _ => {}
            }
            continue;
        }
        match key.code {
            KeyCode::Char(' ') => {
                edit.toggle_mark_current();
                status.clear();
            }
            KeyCode::Char('/') => search = Some((String::new(), edit.row, edit.col)),
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                edit.start_visual(Mode::VisualBlock)
            }
            KeyCode::Char('v') => edit.start_visual(Mode::Visual),
            KeyCode::Char('V') => edit.start_visual(Mode::VisualLine),
            KeyCode::Char('?') => {
                help_open = true;
                help_scroll = 0;
            }
            KeyCode::Char(':') => command = Some(String::new()),
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => edit.redo(),
            KeyCode::Char('g' | 'd' | 'c' | 'y' | 'r' | 'x' | ',') => {
                if key.code == KeyCode::Char('x') {
                    pending_x_since = Some(Instant::now());
                }
                edit.pending = match key.code {
                    KeyCode::Char(c) => Some(c),
                    _ => None,
                }
            }
            KeyCode::Char('j') | KeyCode::Down => edit.down(),
            KeyCode::Char('k') | KeyCode::Up => edit.up(),
            KeyCode::Char('h') | KeyCode::Left => edit.left(),
            KeyCode::Char('l') | KeyCode::Right => edit.right(),
            KeyCode::Char('w') => edit.word_forward(),
            KeyCode::Char('b') => edit.word_back(),
            KeyCode::Char('e') => edit.word_end(),
            KeyCode::Char('0') => edit.col = 0,
            KeyCode::Char('$') => edit.end(),
            KeyCode::Char('G') => {
                edit.row = edit.rows.len().saturating_sub(1);
                edit.col = 0;
            }
            KeyCode::Char('i') => edit.enter_insert(false, false),
            KeyCode::Char('a') => edit.enter_insert(true, false),
            KeyCode::Char('A') => edit.enter_insert(false, true),
            KeyCode::Char('I') => {
                edit.col = 0;
                edit.enter_insert(false, false);
            }
            KeyCode::Char('o') => edit.open_line(false),
            KeyCode::Char('O') => edit.open_line(true),
            KeyCode::Char('s') => {
                edit.delete_char();
                edit.enter_insert(false, false);
            }
            KeyCode::Char('X') => edit.backspace(),
            KeyCode::Char('D') => edit.delete_to_end(false),
            KeyCode::Char('C') => edit.delete_to_end(true),
            KeyCode::Char('S') => edit.change_line(),
            KeyCode::Char('p') => edit.paste(false, browser.path()),
            KeyCode::Char('P') => edit.paste(true, browser.path()),
            KeyCode::Char('u') => edit.undo(),
            KeyCode::Char('q') => {
                if edit.dirty {
                    status = "Unsaved changes; use :w, :wq or :q!".into();
                } else {
                    break;
                }
            }
            KeyCode::Char('H' | '-') | KeyCode::Backspace => {
                if edit.dirty {
                    status = "Save or discard changes before navigating".into();
                } else if let Err(e) = browser.parent() {
                    status = e.to_string();
                } else {
                    edit.load(browser);
                    original = edit.rows.clone();
                    status.clear();
                }
            }
            KeyCode::Char('L') | KeyCode::Enter => {
                if edit.dirty {
                    status = "Save or discard changes before navigating".into();
                    continue;
                }
                browser.select(edit.row);
                if let Some(path) = browser.selected_path().map(Path::to_path_buf) {
                    let result = if path.is_dir() {
                        browser.enter()
                    } else {
                        open::default_plan(&path)
                            .and_then(|plan| launch_plan(terminal, plan, &launch_errors))
                    };
                    match result {
                        Ok(()) => {
                            edit.load(browser);
                            original = edit.rows.clone();
                            status.clear();
                        }
                        Err(e) => status = e.to_string(),
                    }
                }
            }
            KeyCode::Char('z') => {
                if edit.dirty {
                    status = "Save or discard changes before navigating".into();
                    continue;
                }
                match with_terminal_suspended(terminal, zoxide::choose_directory) {
                    Ok(Some(path)) => match browser.navigate_to(&path) {
                        Ok(()) => {
                            edit.load(browser);
                            original = edit.rows.clone();
                            status.clear();
                        }
                        Err(e) => status = e.to_string(),
                    },
                    Ok(None) => {}
                    Err(e) => status = e.to_string(),
                }
            }
            KeyCode::Char('Z') => {
                if edit.dirty {
                    status = "Save or discard changes before navigating".into();
                    continue;
                }
                let root = browser.path().to_path_buf();
                let show_hidden = browser.shows_hidden();
                match with_terminal_suspended(terminal, || {
                    local_search::choose_path(&root, show_hidden)
                }) {
                    Ok(Some(path)) => {
                        let result = if path.is_dir() {
                            browser.navigate_to(&path)
                        } else {
                            browser.reveal(&path)
                        };
                        match result {
                            Ok(()) => {
                                edit.load(browser);
                                original = edit.rows.clone();
                                status.clear();
                            }
                            Err(e) => status = e.to_string(),
                        }
                    }
                    Ok(None) => {}
                    Err(e) => status = e.to_string(),
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn search_match(rows: &[edit::Row], query: &str, origin: usize) -> Option<usize> {
    let query = query.to_lowercase();
    (0..rows.len())
        .map(|offset| (origin + offset + 1) % rows.len())
        .find(|&index| rows[index].name.to_lowercase().contains(&query))
}

fn archive_selection(edit: &Buffer, visual_lines: bool) -> Result<Vec<PathBuf>, String> {
    let (start, end) = if visual_lines {
        let (anchor, _) = edit.visual_anchor.ok_or("Nothing selected")?;
        (anchor.min(edit.row), anchor.max(edit.row))
    } else {
        (edit.row, edit.row)
    };
    archive_rows(edit, &(start..=end).collect::<Vec<_>>())
}

fn archive_rows(edit: &Buffer, indices: &[usize]) -> Result<Vec<PathBuf>, String> {
    if edit.dirty {
        return Err("Save or discard changes before compressing".into());
    }
    let paths: Vec<_> = indices
        .iter()
        .copied()
        .map(|index| {
            edit.rows
                .get(index)
                .and_then(|row| row.original.clone())
                .ok_or_else(|| "Selection contains an unsaved entry".to_string())
        })
        .collect::<Result<_, _>>()?;
    if paths.is_empty() {
        Err("No files to compress".into())
    } else {
        Ok(paths)
    }
}

fn launch_choice(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    choice: &Choice,
    path: &Path,
    errors: &mpsc::Sender<String>,
) -> io::Result<()> {
    let plan = open::OpenPlan {
        command: choice.opener.command(path),
        block: choice.opener.block,
    };
    launch_plan(terminal, plan, errors)
}

fn launch_plan(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    plan: open::OpenPlan,
    errors: &mpsc::Sender<String>,
) -> io::Result<()> {
    let mut command = plan.command;
    if !plan.block {
        return open::launch_background(command, errors.clone());
    }
    let status = with_terminal_suspended(terminal, || command.status())?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "Application exited with {status}"
        )));
    }
    Ok(())
}

fn with_terminal_suspended<T>(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>,
    task: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    let result = task();
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    result
}

#[cfg(test)]
mod search_tests {
    use super::*;
    #[test]
    fn search_wraps_and_ignores_case() {
        let rows = ["alpha", "Beta", "gamma"].map(|name| edit::Row {
            original: None,
            copy_from: None,
            name: name.into(),
            kind: EntryKind::File,
        });
        assert_eq!(search_match(&rows, "BE", 0), Some(1));
        assert_eq!(search_match(&rows, "AL", 2), Some(0));
        assert_eq!(search_match(&rows, "none", 0), None);
        assert_eq!(search_match(&[], "x", 0), None);
    }

    #[test]
    fn archive_uses_all_visual_line_rows() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b", "c"] {
            std::fs::write(root.path().join(name), name).unwrap();
        }
        let browser = Browser::new(root.path().to_path_buf()).unwrap();
        let mut edit = Buffer::from_browser(&browser);
        edit.start_visual(Mode::VisualLine);
        edit.down();
        assert_eq!(archive_selection(&edit, true).unwrap().len(), 2);
        assert_eq!(archive_selection(&edit, false).unwrap().len(), 1);
        let separate = archive_rows(&edit, &[0, 2]).unwrap();
        assert_eq!(
            separate
                .iter()
                .map(|path| path.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
    }

    #[test]
    fn sort_preserves_cursor_and_separate_marks() {
        let root = tempfile::tempdir().unwrap();
        for name in ["alpha", "beta", "gamma"] {
            std::fs::write(root.path().join(name), "").unwrap();
        }
        let mut browser = Browser::new(root.path().to_path_buf()).unwrap();
        let mut edit = Buffer::from_browser(&browser);
        edit.row = 0;
        edit.marked.extend([0, 2]);
        let mut original = edit.rows.clone();
        apply_sort(
            &mut browser,
            &mut edit,
            &mut original,
            SortSpec {
                by: SortBy::Name,
                reverse: true,
                directories_first: true,
            },
        )
        .unwrap();
        assert_eq!(edit.selected().unwrap().name, "alpha");
        let marked: Vec<_> = edit
            .marked
            .iter()
            .map(|index| edit.rows[*index].name.as_str())
            .collect();
        assert_eq!(marked, ["gamma", "alpha"]);
    }
}

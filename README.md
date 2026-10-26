# Salt

Salt is a local terminal file manager built in Rust. It treats the current directory as an editable list: rename, create, copy, and move entries in the list, then save the changes with `:w`.

The name follows [vim-vinegar](https://github.com/tpope/vim-vinegar) and [oil.nvim](https://github.com/stevearc/oil.nvim): vinegar, oil, salt. Salt is an independent application inspired by their directory browsing and editing ideas.

Salt currently targets macOS and Linux. It operates on local files only.

## Install

You need a recent stable Rust toolchain. Clone the repository and run:

```sh
cargo build --release --locked
install -m 755 target/release/salt "$HOME/.local/bin/salt"
```

Make sure `~/.local/bin` is on your `PATH`, then start Salt in the current directory with `salt`. Pass a directory to start elsewhere: `salt ~/Downloads`. The path argument must be a directory.

### Return to the last directory

A program cannot change its parent shell's working directory. To make `salt` leave your zsh shell in the last directory you visited, add this function to `~/.zshrc`:

```zsh
salt() {
  local cwd_file final_dir result
  cwd_file=$(mktemp "${TMPDIR:-/tmp}/salt-cwd.XXXXXX") || return
  command salt --cwd-file "$cwd_file" "$@"
  result=$?
  if (( result == 0 )); then
    IFS= read -r -d '' final_dir < "$cwd_file"
    if [[ -n $final_dir && -d $final_dir ]]; then
      builtin cd -- "$final_dir" || result=$?
    fi
  fi
  rm -f -- "$cwd_file"
  return "$result"
}
```

Restart the shell or source `~/.zshrc`. The `--cwd-file` option writes a NUL-terminated final directory only after Salt exits successfully; the wrapper reads it and changes the shell directory. Calling the binary without the function leaves the shell directory unchanged.

Optional tools:

| Tool | Used for |
| --- | --- |
| Nerd Font | Distinct icons for common file types and familiar folders. Icons are on by default; use `--icons never` without one. Unknown files use a generic icon. |
| `fzf` | Local recursive picker (`Z`) and zoxide picker (`z`). |
| `zoxide` | Directory history for `z`. |
| `zip` | Creating `.zip` archives with `gc`. |
| `$VISUAL` or `$EDITOR` | Opening text and source files with Enter. |
| `xdg-open` on Linux | Opening other files with the system default application. macOS uses `open`. |

For `--icons auto`, set `SALT_NERD_FONT=1` to enable icons. Font detection is not reliable in a terminal, so auto mode uses that explicit hint.

Salt uses the terminal's default foreground and background. ANSI red (color 1) is the interface accent for the path, directories, cursor, and panels; ANSI green (color 2) is the background of search matches. The terminal theme supplies the actual colors. For example, Ghostty's Rose Pine Dawn maps them to `#b4637a` and `#286983`. Salt has no separate color theme.

## Keys

Press `?` in Salt for the full, scrollable key reference. When you press the first key of a sequence (`g`, `d`, `c`, `y`, `r`, `x`, or `,`), a small panel in the bottom-right corner shows the available next keys. Visual selection modes (`v`, `V`, and `Ctrl+v`) show their available actions in the same corner while the selection is active. In `V`, press `g` to see `gc` for compressing the selected rows. Press `Space` to mark separate rows; the selection panel then shows the available actions and count. `Esc` cancels a sequence or selection. Single-key actions run immediately.

### Navigation and opening

| Keys | Action |
| --- | --- |
| `j` / `k`, `↑` / `↓` | Move between rows. |
| `h` / `l`, `←` / `→` | Move the cursor within a name. |
| `w` / `b` / `e`, `0` / `$` | Move within the current name. |
| `gg` / `G` | Go to first / last row. |
| `L` or `Enter` | Enter a directory or open a file. |
| `H`, `-`, or `Backspace` | Go to the parent directory. |
| `g.` | Toggle hidden entries. |
| `/` | Search names in the current directory; matching text is highlighted as you type. Enter keeps the highlights; Esc restores the previous position. |
| `n` / `N` | Jump to the next / previous search match, wrapping around the directory. |
| `:noh` | Clear search highlighting. |
| `z` | Choose a visited directory through zoxide and fzf. |
| `Z` | Find a file or directory below the current directory through fzf. Directories open; files are selected. |
| `gx` | Choose a configured application for the current file or directory. |
| `,a` / `,A` | Sort by name A–Z / Z–A. |
| `,m` / `,M` | Sort by modification time, oldest / newest first. |
| `,s` / `,S` | Sort by size, smallest / largest first. |

Enter uses `$VISUAL`, then `$EDITOR`, for text and common source files. Salt waits until the editor exits. Other files use `open` on macOS or `xdg-open` on Linux. `gx` only offers applications matched by the opener configuration; with no matching rule, it does nothing.

Press `,` to see the sorting choices. Manual sorting applies to the current directory until you leave it. The cursor and Space marks follow their entries when you sort. Sorting requires saving or discarding pending buffer edits first. Directory size is not calculated recursively; directories without a file size sort after files in size mode unless `directories_first = true`.

### Editing names and entries

| Keys | Action |
| --- | --- |
| `i` / `a`, `I` / `A` | Insert before / after the cursor, or at the start / end of a name. |
| `o` / `O` | Add a new row below / above. |
| `x` / `X` | Delete current / previous character. A lone `x` acts after a short sequence timeout. |
| `s` / `r`, `C` / `D` | Substitute / replace a character, or change / delete to end of name. |
| `cc` / `cw` / `dw` | Change a row / word, or delete a word. |
| `u` / `Ctrl+r` | Undo / redo edits in the directory buffer. |
| `v` / `V` / `Ctrl+v` | Select characters / whole rows / a rectangular block. `d`, `c`, and `y` act on the selection. |
| `Space` | Mark or unmark the current row, then move down. Repeat to select nonadjacent files and directories. |
| `yy` | Copy the current row into Salt's register. |
| `xx` | Mark the current entry for a move; the source stays in place until you paste and save. |
| `p` | Paste into the current directory's sort order. The cursor moves to the first pasted entry. |
| `dd` | Ask for confirmation, then permanently delete the current file or directory immediately. |
| `gc` | Create a `.zip` archive of the current entry. In `V` row selection, archive all selected rows together. |

A new row ending in `/` creates a directory. A name such as `test/test1.txt` creates missing parent directories when saved. A relative name such as `../other/name` moves an entry to another local directory. Save or discard pending edits before navigating, deleting, or compressing.

New entries briefly glow with the terminal accent color after pasting, finishing a new buffer row, saving newly created files or directories, or creating a ZIP. The highlight then returns to the normal row colors.

`gc` prompts for a `.zip` filename. It requires the `zip` program, creates the archive in the current directory, and refuses to overwrite an existing path. Multi-item archives can use contiguous `V` rows or separate Space marks.

With Space marks, `d` or `x` removes all marked rows from the buffer, `y` copies them to Salt's register, and `gc` compresses them into one ZIP. Marked names use the accent background; the selection panel shows the count. Removal takes effect on disk only after `:w` and its deletion confirmation. `c` or `s` changes one marked row; changing several complete rows is not supported yet. `Esc` clears all marks. Marks are cleared when the buffer changes or the directory is reloaded, but are preserved when you change the sort order.

### Saving and leaving

| Keys | Action |
| --- | --- |
| `:w` | Review all staged filesystem changes, then apply with Enter or `y`. |
| `:wq` | Review and save, then quit. |
| `q` or `:q` | Quit only when no edits are pending. |
| `:q!` | Quit without saving staged edits. |
| `:e!` | Discard staged edits and reload the directory. |
| `Esc` | Return from insert or visual mode to normal mode. |

The `:w` review lists every planned move, creation, copy, and deletion. Scroll it with `j`/`k` and cancel with Esc or `n`; cancelling leaves the buffer intact. A save with no filesystem changes does not open the review. Deleting with `dd` is immediate after its separate confirmation. Cutting with `xx` is deferred: if you quit or replace the cut register without pasting, the original entry stays untouched. Salt checks for name collisions before saving and refuses to overwrite unrelated existing paths.

## Configuration

Copy [salt.toml.example](salt.toml.example) to `~/.config/salt/salt.toml` (or `$XDG_CONFIG_HOME/salt/salt.toml`) and edit it. Restart Salt after changing the file. A missing configuration is valid; a malformed file is reported at startup.

```toml
[opener]
nvim = [{ command = ["nvim", "{file}"], desc = "Neovim", block = true }]

[[open.rules]]
glob = "*.{rs,md,txt}"
use = ["nvim"]
```

Rules are checked in order; the first matching rule supplies the `gx` menu. Rules match files unless `kind = "directory"` is set. `glob` matches a filename without case sensitivity, and `mime` matches a MIME type inferred from the extension. Both must match when both are present. An opener may have separate variants for `for = "macos"`, `"linux"`, or `"unix"`. The `for` field makes that choice available only on the named platform; omit it for all supported platforms. It does not install or locate the command. Set `block = true` for terminal programs that should run while Salt's TUI is suspended. Commands are argument arrays; `{file}` must be a separate argument and is passed without shell interpretation.

For example, a `gx` choice for PDFs can use the operating system's opener on each platform:

```toml
[opener]
system = [
  { command = ["open", "{file}"], desc = "System app", for = "macos" },
  { command = ["xdg-open", "{file}"], desc = "System app", for = "linux" },
]

[[open.rules]]
glob = "*.pdf"
use = ["system"]
```

The example includes optional Finder, Neovim, VS Code, TextEdit, Preview, and ImageOptim choices. Salt does not require those applications.

The same file controls the default sort and exact directory rules. This example makes Downloads show the newest files and folders first, mixed by modification time:

```toml
[sort]
by = "name"
directories_first = true

[[sort.rules]]
path = "~/Downloads"
by = "mtime"
reverse = true
directories_first = false
```

Valid `by` values are `name`, `mtime`, and `size`. `reverse` defaults to `false`; `directories_first` defaults to `true`. A directory rule inherits values omitted from `[sort]`. Paths must be absolute or start with `~/`, and match the opened directory itself; the rule does not apply to subdirectories. Set `directories_first = false` to mix files and directories by date or size. Configured sorting is restored when you revisit a directory; the `,` shortcuts only change the current visit.

## Development and scope

Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --locked`, and `cargo build --release --locked` before publishing changes. Tests that create ZIP archives require `zip` on `PATH`.

See [Architecture](docs/architecture.md) for the buffer model, filesystem planning, and next structural improvements.

Salt is an evolving file manager, not a complete Neovim implementation. It has no SSH backend, trash integration, or full Vim command and register support yet. `V` selects contiguous rows; Space marks nonadjacent rows. Filesystem changes are local; keep backups of important data when testing destructive editing commands.

## Acknowledgments

- [vim-vinegar](https://github.com/tpope/vim-vinegar) by Tim Pope, for the Vim file browser lineage.
- [oil.nvim](https://github.com/stevearc/oil.nvim) by stevearc, for the editable directory model.
- [Neovim](https://neovim.io/), for the editing vocabulary.
- [Yazi](https://github.com/sxyazi/yazi), for ideas around configurable openers and navigation.
- [Superfile](https://github.com/yorukot/superfile), for file manager interaction ideas and the [file icon mapping](https://github.com/yorukot/superfile/blob/main/src/config/icon/icon.go) that inspired Salt's Nerd Font glyphs. Salt uses terminal ANSI colors instead of Superfile's fixed icon colors.
- [which-key.nvim](https://github.com/folke/which-key.nvim) by folke, for showing available keys during a sequence.
- [fzf](https://github.com/junegunn/fzf) and [zoxide](https://github.com/ajeetdsouza/zoxide), which power Salt's jump pickers when installed.

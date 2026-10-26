# Architecture

Salt keeps filesystem state, editable rows, external tools, and terminal rendering separate:

| Module | Responsibility |
| --- | --- |
| `model.rs` | Read and sort a directory; keep the current path and cursor position. |
| `sort.rs`, `config.rs` | Parse global and per-directory sort rules, and load the shared TOML configuration. |
| `edit.rs` | Keep editable rows, undo history, registers, marks, and the filesystem change plan. |
| `open.rs` | Parse opener rules and launch applications. |
| `archive.rs` | Create ZIP archives without overwriting an existing destination. |
| `zoxide.rs`, `local_search.rs` | Run the two optional fzf pickers. |
| `main.rs` | Handle terminal lifecycle, render the interface, and dispatch keys. |

An editable row keeps its original path separately from its displayed name. A rename changes the name in memory; `:w` compares the current rows with the original snapshot and builds a `Plan` of renames, creates, copies, and deletions. The plan rejects duplicate destinations and collisions before applying changes. Deletions require confirmation. `xx` only stores a move intention in the register; nothing moves until it is pasted and saved.

`V` represents a contiguous range of rows. Space marks individual row indices, which allows nonadjacent selection. Both use the same row operator for copying and removing whole rows. Marks are cleared on buffer edits and directory reloads so an index cannot silently refer to a different file. Manual sorting saves the marked rows' original paths, reloads the buffer in the new order, and restores the marks by path. A future buffer model with stable row identities could preserve marks across more kinds of edit and reload.

The current event loop and rendering code still share `main.rs`. This keeps the small application easy to navigate today, but it is the next place to split as commands grow: an application state object, a shared keymap description for dispatch and hints, and a separate rendering module would remove duplicated knowledge of key sequences. The core editing and filesystem modules can remain independent of the terminal UI. This follows the separation of core editing, view state, terminal UI, and commands described in [Helix's architecture](https://github.com/helix-editor/helix/blob/master/docs/architecture.md).

Filesystem operations are checked before they run, and many rename failures can be rolled back. A group of filesystem operations is not a fully atomic transaction, especially once permanent deletion begins. Tests use temporary directories to verify operations and undo behavior. Salt targets local macOS and Linux filesystems.

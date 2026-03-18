# Technical Documentation

## Architecture

ctxmenu is a single Rust binary that operates in two modes:

- **GUI mode** (default, no arguments) — an iced application with async registry access
- **CLI mode** (subcommands: `list`, `disable`, `enable`) — synchronous operations with table output

The `#![windows_subsystem = "windows"]` attribute suppresses the console window in GUI mode. When a CLI subcommand is detected, `AttachConsole(ATTACH_PARENT_PROCESS)` reattaches stdout to the parent terminal.

## Modules

### `registry.rs`

Core module. All Windows Registry interaction lives here.

**Data types:**

| Type | Description |
|---|---|
| `MenuEntry` | One context menu item: name, registry path, type, location, status, command |
| `EntryType` | `Shell` (verb-based, has `command` subkey) or `ShellEx` (COM handler, has CLSID default value) |
| `Location` | `Files`, `Folders`, or `Background` |
| `Status` | `Enabled` or `Disabled` (presence of `LegacyDisable` value) |

**Registry scan targets:**

All paths are under `HKEY_CLASSES_ROOT`:

| Path | Type | Location |
|---|---|---|
| `*\shell` | Shell | Files |
| `*\shellex\ContextMenuHandlers` | ShellEx | Files |
| `Directory\shell` | Shell | Folders |
| `Directory\shellex\ContextMenuHandlers` | ShellEx | Folders |
| `Directory\Background\shell` | Shell | Background |
| `Directory\Background\shellex\ContextMenuHandlers` | ShellEx | Background |

**Key functions:**

- `scan_entries(filter)` — Enumerates all 6 paths (or a filtered subset), reads each subkey's name, command/CLSID, and `LegacyDisable` status. Returns sorted `Vec<MenuEntry>`.
- `toggle_entry(registry_path)` — Checks current disabled state, then flips it. Used by the GUI.
- `disable_entry(name)` / `enable_entry(name)` — Name-based lookup across all paths. Used by the CLI.
- `open_writable(registry_path)` — Tries HKCR with write access; falls back to HKCU (see below).
- `is_disabled(registry_path)` — Checks both HKCR and HKCU for `LegacyDisable`.

### `ui.rs`

Iced GUI using the Elm architecture (state → message → update → view).

**State (`App` struct):**
- `entries` — loaded `MenuEntry` list
- `location_filter` — current filter selection
- `search_query` — text filter (matches name and command)
- `log_content` — `text_editor::Content` for the diagnostics panel (read-only, selectable)

**Async pattern:**
Registry calls are blocking. They're wrapped in `tokio::task::spawn_blocking()` and dispatched via `Task::perform()`. Results arrive as messages (`EntriesLoaded`, `ToggleResult`) processed in `update()`.

**View hierarchy:**
```
Container
├── Toolbar: pick_list (filter) + text_input (search) + button (refresh)
├── horizontal_rule
├── Body: scrollable
│   ├── Header row (column labels)
│   └── Entry rows (name, type, location, command, toggler)
├── horizontal_rule
├── Status bar: entry count + logs button + error banner
└── Log panel (conditionally shown): read-only text_editor
```

### `display.rs`

CLI-only. Converts `Vec<MenuEntry>` into a formatted table using the `tabled` crate. Truncates long command strings to 60 characters.

### `main.rs`

Entry point. Parses CLI args with clap. `None` subcommand → GUI. `Some(subcommand)` → attaches console and dispatches to registry + display functions.

## Registry Write Strategy

`HKEY_CLASSES_ROOT` is a merged view of:
- `HKEY_LOCAL_MACHINE\SOFTWARE\Classes` — system-wide, often owned by TrustedInstaller
- `HKEY_CURRENT_USER\SOFTWARE\Classes` — per-user, always writable

Many context menu entries are installed system-wide under HKLM. Writing directly to their HKCR path fails with access denied, even as admin, because TrustedInstaller owns the key.

**Solution (`open_writable`):**

1. Try opening the key under HKCR with `KEY_READ | KEY_WRITE`
2. If that fails, create/open the equivalent key under `HKCU\SOFTWARE\Classes\<path>`
3. Write `LegacyDisable` there

Windows merges HKCU on top of HKLM in the HKCR view. The `LegacyDisable` value written to HKCU is visible through HKCR and the context menu system respects it.

**Reading disabled state (`is_disabled`):**

Checks both HKCR (merged view) and HKCU directly, to handle cases where the merge view hasn't updated.

## Disable Mechanism

The `LegacyDisable` value is a standard Windows mechanism:
- **Type:** `REG_SZ` (empty string)
- **Effect:** Windows shell hides the context menu entry
- **Scope:** Per-key (each `shell\<verb>` or `shellex\ContextMenuHandlers\<name>` key)
- **Reversibility:** Delete the value to restore the entry

No registry keys are created or deleted (except the HKCU override key when needed). No values other than `LegacyDisable` are modified.

## Dependencies

| Crate | Purpose |
|---|---|
| `clap` (derive) | CLI argument parsing |
| `winreg` | Windows Registry access (safe wrapper over RegKey APIs) |
| `tabled` | Table formatting for CLI output |
| `anyhow` | Error handling with context |
| `iced` (tokio) | GUI framework |
| `tokio` (rt) | Async runtime for blocking registry tasks |
| `windows-sys` (Windows only) | `AttachConsole` for CLI-in-GUI-binary |

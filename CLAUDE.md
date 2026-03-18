# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

ctxmenu is a Windows context menu manager written in Rust. It provides both a GUI (iced) and CLI interface to list, disable, and re-enable Windows right-click context menu entries via the Windows Registry.

## Build & Run

```bash
# Build (must target Windows)
cargo build --release
cargo check --target x86_64-pc-windows-msvc   # cross-check from WSL

# Run GUI (no args)
ctxmenu.exe

# Run CLI
ctxmenu.exe list [--location <files|folders|background|all>]
ctxmenu.exe disable <name>
ctxmenu.exe enable <name>
```

No test suite exists yet. Verification is manual: run the binary on Windows and check registry state.

## Architecture

**Dual-mode binary:** No subcommand launches the GUI (`ui::run()`). Subcommands (`list`, `disable`, `enable`) run in CLI mode. The `#![windows_subsystem = "windows"]` attribute hides the console; CLI mode reattaches via `AttachConsole`.

**Module roles:**
- `main.rs` — clap CLI dispatch, dual-mode entry point
- `registry.rs` — all Windows Registry logic, core data types (`MenuEntry`, `EntryType`, `Location`, `Status`)
- `source.rs` — source program resolution (extracts exe paths from commands, resolves CLSIDs to DLLs, reads PE version info via `GetFileVersionInfoW`/`VerQueryValueW`)
- `ui.rs` — iced GUI (App state, messages, view, async task dispatch)
- `display.rs` — CLI-only table formatting via `tabled`

**Registry write strategy (HKCR/HKCU fallback):** The critical pattern in `registry.rs`. HKCR keys are often owned by TrustedInstaller. `open_writable()` tries HKCR first, then falls back to creating the key under `HKCU\SOFTWARE\Classes\` which is always user-writable. Windows merges HKCU into the HKCR view. `is_disabled()` checks both locations. This means disable/enable works without admin.

**Disable mechanism:** Sets a `LegacyDisable` REG_SZ value on the entry's key. Non-destructive and reversible.

**Async pattern in GUI:** Registry calls are blocking. The UI wraps them in `tokio::task::spawn_blocking()` via `Task::perform()` to avoid freezing.

## Workflow

**Do not commit until the user has tested and confirmed the changes.** After making code changes, build and let the user verify manually. Only commit when the user explicitly asks.

**After completing a plan:** Once all implementation and testing from a plan is finished, automatically stage the changes and prepare the commit (write a commit message), but **always ask the user before actually committing**. Do not wait for the user to ask — proactively prep the git commit and present it for approval.

## Registry Paths Scanned

Twenty paths under `HKEY_CLASSES_ROOT` (shell + shellex for each): `*`, `SystemFileAssociations\*`, `AllFilesystemObjects`, `Directory`, `Folder`, `Directory\Background`, `DesktopBackground`, `SystemFileAssociations\image`, `SystemFileAssociations\audio`, `SystemFileAssociations\video`. Additionally, per-extension `SystemFileAssociations\.<ext>` and ProgID paths are scanned dynamically.

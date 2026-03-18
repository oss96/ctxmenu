# ctxmenu

A Windows context menu manager. View, disable, and re-enable right-click menu entries without deleting anything from the registry.

## Features

- **List** all context menu entries across files, folders, and desktop background
- **Disable/Enable** entries safely using the `LegacyDisable` registry mechanism (non-destructive, always reversible)
- **GUI mode** — run `ctxmenu` with no arguments to open a windowed interface with search, filtering, and toggle switches
- **CLI mode** — scriptable commands for automation
- **No admin required** — writes to HKCU when system keys are protected

## Installation

Requires [Rust](https://rustup.rs/) (2024 edition).

```bash
cargo build --release
```

The binary will be at `target\release\ctxmenu.exe`.

## Usage

### GUI

Double-click `ctxmenu.exe` or run it with no arguments. The GUI provides:

- Location filter dropdown (Files / Folders / Background / All)
- Search box to filter by name or command
- Toggle switch per entry to enable/disable
- Log panel (click "Logs" in the status bar) for diagnostics

### CLI

```bash
# List all context menu entries
ctxmenu list

# Filter by location
ctxmenu list --location folders

# Disable an entry
ctxmenu disable "Open with Code"

# Re-enable it
ctxmenu enable "Open with Code"
```

You may need to restart Explorer or sign out/in for changes to appear in the right-click menu.

## How It Works

Windows context menu entries live in the registry under `HKEY_CLASSES_ROOT`. ctxmenu scans six paths covering files, folders, and background menus for both `shell` (verb-based) and `shellex` (COM handler) entries.

To disable an entry, ctxmenu sets a `LegacyDisable` string value on the entry's registry key — the standard Windows mechanism for hiding context menu items. Nothing is deleted. To re-enable, it removes that value.

Some registry keys are owned by TrustedInstaller and can't be written to directly. In those cases, ctxmenu writes to the equivalent key under `HKEY_CURRENT_USER\SOFTWARE\Classes`, which Windows merges into the HKCR view.

## License

[MIT](LICENSE)

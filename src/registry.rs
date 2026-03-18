use anyhow::{bail, Context, Result};
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone, PartialEq)]
pub struct MenuEntry {
    pub name: String,
    pub registry_path: String,
    pub entry_type: EntryType,
    pub location: Location,
    pub status: Status,
    pub command: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryType {
    Shell,
    ShellEx,
    Modern,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Location {
    Files,
    Folders,
    Background,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Enabled,
    Disabled,
}

impl std::fmt::Display for EntryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryType::Shell => write!(f, "shell"),
            EntryType::ShellEx => write!(f, "shellex"),
            EntryType::Modern => write!(f, "modern"),
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::Files => write!(f, "Files"),
            Location::Folders => write!(f, "Folders"),
            Location::Background => write!(f, "Background"),
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Enabled => write!(f, "Enabled"),
            Status::Disabled => write!(f, "Disabled"),
        }
    }
}

struct ScanTarget {
    path: &'static str,
    entry_type: EntryType,
    location: Location,
}

const SCAN_TARGETS: &[fn() -> ScanTarget] = &[
    // *  (all file types)
    || ScanTarget {
        path: r"*\shell",
        entry_type: EntryType::Shell,
        location: Location::Files,
    },
    || ScanTarget {
        path: r"*\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Files,
    },
    // SystemFileAssociations\*  (modern entries like "Edit in Notepad")
    || ScanTarget {
        path: r"SystemFileAssociations\*\shell",
        entry_type: EntryType::Shell,
        location: Location::Files,
    },
    || ScanTarget {
        path: r"SystemFileAssociations\*\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Files,
    },
    // AllFilesystemObjects  (applies to files and folders)
    || ScanTarget {
        path: r"AllFilesystemObjects\shell",
        entry_type: EntryType::Shell,
        location: Location::Files,
    },
    || ScanTarget {
        path: r"AllFilesystemObjects\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Files,
    },
    // Directory
    || ScanTarget {
        path: r"Directory\shell",
        entry_type: EntryType::Shell,
        location: Location::Folders,
    },
    || ScanTarget {
        path: r"Directory\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Folders,
    },
    // Folder  (subtly different from Directory — includes virtual folders)
    || ScanTarget {
        path: r"Folder\shell",
        entry_type: EntryType::Shell,
        location: Location::Folders,
    },
    || ScanTarget {
        path: r"Folder\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Folders,
    },
    // Directory\Background
    || ScanTarget {
        path: r"Directory\Background\shell",
        entry_type: EntryType::Shell,
        location: Location::Background,
    },
    || ScanTarget {
        path: r"Directory\Background\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Background,
    },
    // DesktopBackground
    || ScanTarget {
        path: r"DesktopBackground\shell",
        entry_type: EntryType::Shell,
        location: Location::Background,
    },
    || ScanTarget {
        path: r"DesktopBackground\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Background,
    },
];

// Windows 11 modern context menu entries (IExplorerCommand / PackagedCom).
// These are disabled via Shell Extensions\Blocked, not LegacyDisable.
// The registry_path for these entries is stored as "blocked:{CLSID}".
const MODERN_ENTRIES: &[(&str, &str, Location)] = &[
    ("Edit with Notepad", "{CA6CC9F1-867A-481E-951E-A28C5E4F01EA}", Location::Files),
    ("Edit with Paint", "{2430F218-B743-4FD6-97BF-5C76541B4AE9}", Location::Files),
    ("Edit with Clipchamp", "{8BCF599D-B158-450F-B4C2-430932F2AF2F}", Location::Files),
    ("Open in Terminal", "{9F156763-7844-4DC4-B2B1-901F640F5155}", Location::Background),
    ("Open in Terminal Preview", "{02DB545A-3E20-46DE-83A5-1329B1E88B6B}", Location::Background),
    ("Copy as path", "{f3d06e7c-1e45-4a26-847e-f9fcdee59be0}", Location::Files),
    ("Ask Copilot", "{CB3B0003-8088-4EDE-8769-8B354AB2FF8C}", Location::Files),
    ("Photos", "{CD349BB6-A2BC-47ED-874F-7185ABA53BD4}", Location::Files),
    ("Photos (Share)", "{BFE0E2A4-C70C-4AD7-AC3D-10D1ECEBB5B4}", Location::Files),
];

const BLOCKED_KEY_PATH: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Shell Extensions\Blocked";

fn is_modern_blocked(clsid: &str) -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(BLOCKED_KEY_PATH, KEY_READ) {
        return key.get_value::<String, _>(clsid).is_ok();
    }
    false
}

fn scan_modern_entries(filter: Option<&Location>, entries: &mut Vec<MenuEntry>) {
    // Only include entries whose PackagedCom CLSID actually exists on this system
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    for &(name, clsid, ref location) in MODERN_ENTRIES {
        if let Some(f) = filter {
            if *location != *f {
                continue;
            }
        }

        // Check if this handler is actually registered on this machine
        let class_path = format!(r"PackagedCom\ClassIndex\{clsid}");
        if hkcr.open_subkey_with_flags(&class_path, KEY_READ).is_err() {
            continue;
        }

        let blocked = is_modern_blocked(clsid);
        entries.push(MenuEntry {
            name: name.to_string(),
            registry_path: format!("blocked:{clsid}"),
            entry_type: EntryType::Modern,
            location: location.clone(),
            status: if blocked { Status::Disabled } else { Status::Enabled },
            command: Some(clsid.to_string()),
        });
    }
}

pub fn scan_entries(filter: Option<&Location>) -> Result<Vec<MenuEntry>> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let mut entries = Vec::new();

    for target_fn in SCAN_TARGETS {
        let target = target_fn();
        if let Some(f) = filter {
            if target.location != *f {
                continue;
            }
        }

        let parent = match hkcr.open_subkey_with_flags(target.path, KEY_READ) {
            Ok(key) => key,
            Err(_) => continue,
        };

        for name in parent.enum_keys().filter_map(|k| k.ok()) {
            let subkey = match parent.open_subkey_with_flags(&name, KEY_READ) {
                Ok(key) => key,
                Err(_) => continue,
            };

            let registry_path = format!("{}\\{}", target.path, name);
            let disabled = is_disabled(&registry_path);

            let command = match target.entry_type {
                EntryType::Shell => subkey
                    .open_subkey_with_flags("command", KEY_READ)
                    .ok()
                    .and_then(|cmd_key| cmd_key.get_value::<String, _>("").ok()),
                EntryType::ShellEx | EntryType::Modern => {
                    subkey.get_value::<String, _>("").ok()
                }
            };

            entries.push(MenuEntry {
                name,
                registry_path,
                entry_type: target.entry_type.clone(),
                location: target.location.clone(),
                status: if disabled {
                    Status::Disabled
                } else {
                    Status::Enabled
                },
                command,
            });
        }
    }

    // Scan Windows 11 modern context menu entries
    scan_modern_entries(filter, &mut entries);

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

fn find_entry_key(name: &str, writable: bool) -> Result<(RegKey, String)> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let flags = if writable {
        KEY_READ | KEY_WRITE
    } else {
        KEY_READ
    };

    let all_paths: &[&str] = &[
        r"*\shell",
        r"*\shellex\ContextMenuHandlers",
        r"SystemFileAssociations\*\shell",
        r"SystemFileAssociations\*\shellex\ContextMenuHandlers",
        r"AllFilesystemObjects\shell",
        r"AllFilesystemObjects\shellex\ContextMenuHandlers",
        r"Directory\shell",
        r"Directory\shellex\ContextMenuHandlers",
        r"Folder\shell",
        r"Folder\shellex\ContextMenuHandlers",
        r"Directory\Background\shell",
        r"Directory\Background\shellex\ContextMenuHandlers",
        r"DesktopBackground\shell",
        r"DesktopBackground\shellex\ContextMenuHandlers",
    ];

    for parent_path in all_paths {
        let parent = match hkcr.open_subkey_with_flags(parent_path, flags) {
            Ok(key) => key,
            Err(_) => continue,
        };

        for key_name in parent.enum_keys().filter_map(|k| k.ok()) {
            if key_name.eq_ignore_ascii_case(name) {
                let subkey = parent
                    .open_subkey_with_flags(&key_name, flags)
                    .with_context(|| format!("Failed to open key '{key_name}' under {parent_path}"))?;
                return Ok((subkey, format!("{parent_path}\\{key_name}")));
            }
        }
    }

    bail!("Entry '{name}' not found in any context menu registry location");
}

pub fn disable_entry(name: &str) -> Result<()> {
    let (key, path) = find_entry_key(name, true)?;

    if key.get_value::<String, _>("LegacyDisable").is_ok() {
        bail!("Entry '{name}' is already disabled");
    }

    key.set_value("LegacyDisable", &"")
        .with_context(|| format!("Failed to set LegacyDisable on {path}"))?;

    Ok(())
}

pub fn enable_entry(name: &str) -> Result<()> {
    let (key, path) = find_entry_key(name, true)?;

    if key.get_value::<String, _>("LegacyDisable").is_err() {
        bail!("Entry '{name}' is already enabled");
    }

    key.delete_value("LegacyDisable")
        .with_context(|| format!("Failed to remove LegacyDisable from {path}"))?;

    Ok(())
}

pub fn require_admin() -> Result<()> {
    if !is_admin() {
        bail!(
            "This operation requires administrator privileges.\n\
             Please run this command in an elevated terminal (Run as Administrator)."
        );
    }
    Ok(())
}

pub fn is_admin() -> bool {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    hkcr.open_subkey_with_flags(r"*\shell", KEY_READ | KEY_WRITE)
        .is_ok()
}

/// Open a registry key for writing, trying HKCR first then falling back to
/// HKCU\SOFTWARE\Classes (which is always writable by the current user).
fn open_writable(registry_path: &str) -> Result<RegKey> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    if let Ok(key) = hkcr.open_subkey_with_flags(registry_path, KEY_READ | KEY_WRITE) {
        return Ok(key);
    }

    // HKCR key is likely HKLM-backed (TrustedInstaller). Write to HKCU override instead.
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hkcu_path = format!(r"SOFTWARE\Classes\{registry_path}");
    let (key, _) = hkcu
        .create_subkey_with_flags(&hkcu_path, KEY_READ | KEY_WRITE)
        .with_context(|| format!("Failed to open or create '{hkcu_path}' under HKCU"))?;
    Ok(key)
}

/// Check if LegacyDisable is set, checking both HKCR and the HKCU override.
fn is_disabled(registry_path: &str) -> bool {
    // Check HKCR merged view first
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    if let Ok(key) = hkcr.open_subkey_with_flags(registry_path, KEY_READ) {
        if key.get_value::<String, _>("LegacyDisable").is_ok() {
            return true;
        }
    }

    // Also check HKCU override directly (in case HKCR merge doesn't reflect it yet)
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hkcu_path = format!(r"SOFTWARE\Classes\{registry_path}");
    if let Ok(key) = hkcu.open_subkey_with_flags(&hkcu_path, KEY_READ) {
        if key.get_value::<String, _>("LegacyDisable").is_ok() {
            return true;
        }
    }

    false
}

pub fn toggle_entry(registry_path: &str) -> Result<Status> {
    // Modern entries use "blocked:{CLSID}" as their registry_path
    if let Some(clsid) = registry_path.strip_prefix("blocked:") {
        return toggle_modern_entry(clsid);
    }

    let currently_disabled = is_disabled(registry_path);

    if currently_disabled {
        remove_legacy_disable(registry_path)?;
        Ok(Status::Enabled)
    } else {
        let key = open_writable(registry_path)?;
        key.set_value("LegacyDisable", &"")
            .with_context(|| format!("Failed to set LegacyDisable on {registry_path}"))?;
        Ok(Status::Disabled)
    }
}

fn toggle_modern_entry(clsid: &str) -> Result<Status> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (blocked_key, _) = hkcu
        .create_subkey_with_flags(BLOCKED_KEY_PATH, KEY_READ | KEY_WRITE)
        .with_context(|| format!("Failed to open {BLOCKED_KEY_PATH}"))?;

    if is_modern_blocked(clsid) {
        blocked_key
            .delete_value(clsid)
            .with_context(|| format!("Failed to unblock {clsid}"))?;
        Ok(Status::Enabled)
    } else {
        blocked_key
            .set_value(clsid, &"")
            .with_context(|| format!("Failed to block {clsid}"))?;
        Ok(Status::Disabled)
    }
}

/// Remove LegacyDisable from wherever it exists.
/// Tries HKCR (direct), then HKCU override. If the value only exists in
/// HKLM (system-protected), it cannot be removed without TrustedInstaller.
fn remove_legacy_disable(registry_path: &str) -> Result<()> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hkcu_path = format!(r"SOFTWARE\Classes\{registry_path}");

    let mut removed = false;

    // Try removing from HKCR directly (works if key is user/admin-writable)
    if let Ok(key) = hkcr.open_subkey_with_flags(registry_path, KEY_READ | KEY_WRITE) {
        if key.delete_value("LegacyDisable").is_ok() {
            removed = true;
        }
    }

    // Try removing from HKCU override
    if let Ok(key) = hkcu.open_subkey_with_flags(&hkcu_path, KEY_READ | KEY_WRITE) {
        if key.delete_value("LegacyDisable").is_ok() {
            removed = true;
        }
    }

    if removed {
        return Ok(());
    }

    // Value exists but is in HKLM and protected
    if is_disabled(registry_path) {
        bail!(
            "'{registry_path}' is disabled by Windows (system-protected). \
             Cannot re-enable without TrustedInstaller permissions."
        );
    }

    Ok(())
}

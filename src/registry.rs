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
];

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
                EntryType::ShellEx => subkey.get_value::<String, _>("").ok(),
            };

            entries.push(MenuEntry {
                name,
                registry_path,
                entry_type: match target.entry_type {
                    EntryType::Shell => EntryType::Shell,
                    EntryType::ShellEx => EntryType::ShellEx,
                },
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
        r"Directory\shell",
        r"Directory\shellex\ContextMenuHandlers",
        r"Directory\Background\shell",
        r"Directory\Background\shellex\ContextMenuHandlers",
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
    let currently_disabled = is_disabled(registry_path);
    let key = open_writable(registry_path)?;

    if currently_disabled {
        key.delete_value("LegacyDisable")
            .with_context(|| format!("Failed to remove LegacyDisable from {registry_path}"))?;
        Ok(Status::Enabled)
    } else {
        key.set_value("LegacyDisable", &"")
            .with_context(|| format!("Failed to set LegacyDisable on {registry_path}"))?;
        Ok(Status::Disabled)
    }
}

use std::collections::{HashMap, HashSet};

use anyhow::{bail, Context, Result};
use winreg::enums::*;
use winreg::RegKey;

use crate::source;

#[derive(Debug, Clone, PartialEq)]
pub struct MenuEntry {
    pub name: String,
    pub registry_path: String,
    pub entry_type: EntryType,
    pub location: Location,
    pub status: Status,
    pub command: Option<String>,
    pub source: String,
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
    // SystemFileAssociations\image  (all image files by perceived type)
    || ScanTarget {
        path: r"SystemFileAssociations\image\shell",
        entry_type: EntryType::Shell,
        location: Location::Files,
    },
    || ScanTarget {
        path: r"SystemFileAssociations\image\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Files,
    },
    // SystemFileAssociations\audio
    || ScanTarget {
        path: r"SystemFileAssociations\audio\shell",
        entry_type: EntryType::Shell,
        location: Location::Files,
    },
    || ScanTarget {
        path: r"SystemFileAssociations\audio\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Files,
    },
    // SystemFileAssociations\video
    || ScanTarget {
        path: r"SystemFileAssociations\video\shell",
        entry_type: EntryType::Shell,
        location: Location::Files,
    },
    || ScanTarget {
        path: r"SystemFileAssociations\video\shellex\ContextMenuHandlers",
        entry_type: EntryType::ShellEx,
        location: Location::Files,
    },
];

/// Well-known ShellEx handler key names → actual menu display text.
/// ShellEx handlers generate their display names at runtime via COM,
/// so we maintain this table for common Windows handlers.
const SHELLEX_DISPLAY_NAMES: &[(&str, &str)] = &[
    ("EPP", "Scan with Microsoft Defender..."),
    ("Sharing", "Give access to"),
    ("ModernSharing", "Share"),
    ("CopyAsPathMenu", "Copy as path"),
    ("SendTo", "Send to"),
    ("PintoStartScreen", "Pin to Start"),
    ("PlayTo", "Cast to Device"),
    ("ShellImagePreview", "Image Preview"),
];

/// Well-known ShellEx CLSIDs → actual menu display text.
/// For entries whose registry key name IS a CLSID.
const SHELLEX_CLSID_NAMES: &[(&str, &str)] = &[
    ("{596AB062-B4D2-4215-9F74-E9109B0A8153}", "Restore previous versions"),
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
    ("Edit with Photos", "{7A53B94A-4E6E-4826-B48E-535020B264E5}", Location::Files),
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
            source: "Windows".to_string(),
        });
    }
}

/// Dynamically discover modern context menu entries from PackagedCom registrations.
/// This catches third-party IExplorerCommand handlers (e.g. "Open with Zed") that
/// are not in the hardcoded MODERN_ENTRIES list.
fn scan_dynamic_modern_entries(
    filter: Option<&Location>,
    entries: &mut Vec<MenuEntry>,
    known_clsids: &HashSet<String>,
) {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);

    let packages_key = match hkcr.open_subkey_with_flags(r"PackagedCom\Package", KEY_READ) {
        Ok(key) => key,
        Err(_) => return,
    };

    for package_name in packages_key.enum_keys().filter_map(|k| k.ok()) {
        let class_path = format!(r"PackagedCom\Package\{}\Class", package_name);
        let class_key = match hkcr.open_subkey_with_flags(&class_path, KEY_READ) {
            Ok(key) => key,
            Err(_) => continue,
        };

        for clsid in class_key.enum_keys().filter_map(|k| k.ok()) {
            if known_clsids.contains(&clsid.to_lowercase()) {
                continue;
            }

            // Skip OLE document/automation classes (they have ProgID or TypeLib subkeys)
            let clsid_reg_path = format!(r"CLSID\{}", clsid);
            if let Ok(clsid_key) = hkcr.open_subkey_with_flags(&clsid_reg_path, KEY_READ)
            {
                if clsid_key
                    .open_subkey_with_flags("ProgID", KEY_READ)
                    .is_ok()
                    || clsid_key
                        .open_subkey_with_flags("TypeLib", KEY_READ)
                        .is_ok()
                    || clsid_key
                        .open_subkey_with_flags("Insertable", KEY_READ)
                        .is_ok()
                {
                    continue;
                }
            }

            // Only include CLSIDs with a human-readable display name
            let display_name = match source::clsid_display_name(&clsid) {
                Some(name) => name,
                None => continue,
            };

            let location = Location::Files;
            if let Some(f) = filter {
                if location != *f {
                    continue;
                }
            }

            let blocked = is_modern_blocked(&clsid);
            let source_name = derive_source_from_package(&package_name);

            entries.push(MenuEntry {
                name: display_name,
                registry_path: format!("blocked:{clsid}"),
                entry_type: EntryType::Modern,
                location,
                status: if blocked {
                    Status::Disabled
                } else {
                    Status::Enabled
                },
                command: Some(clsid.clone()),
                source: source_name,
            });
        }
    }
}

/// Extract a readable app name from a package full name.
/// e.g. "ZedIndustries.Zed_1.2.3_x64__hash" → "Zed"
fn derive_source_from_package(package_name: &str) -> String {
    let family = package_name.split('_').next().unwrap_or(package_name);
    family.rsplit('.').next().unwrap_or(family).to_string()
}

/// Try to resolve a human-readable display name for a context menu entry
/// by reading MUIVerb, the default value, or falling back to the key name.
fn resolve_entry_display_name(subkey: &RegKey, entry_type: &EntryType, key_name: &str) -> String {
    match entry_type {
        EntryType::Shell => {
            // 1. Try MUIVerb (explicit display name, may be a MUI string)
            if let Ok(mui_verb) = subkey.get_value::<String, _>("MUIVerb") {
                if let Some(resolved) = source::resolve_display_name(&mui_verb) {
                    return resolved;
                }
            }
            // 2. Try the default value of the verb key itself
            if let Ok(default_val) = subkey.get_value::<String, _>("") {
                let trimmed = default_val.trim().to_string();
                if !trimmed.is_empty() {
                    if let Some(resolved) = source::resolve_display_name(&trimmed) {
                        return resolved;
                    }
                }
            }
            // 3. Fallback to the registry key name
            key_name.to_string()
        }
        EntryType::ShellEx => {
            // 1. Check hardcoded display name table
            if let Some(&(_, display)) = SHELLEX_DISPLAY_NAMES
                .iter()
                .find(|&&(name, _)| name.eq_ignore_ascii_case(key_name))
            {
                return display.to_string();
            }
            // 2. For CLSID key names, check CLSID tables then registry
            if key_name.starts_with('{') {
                if let Some(&(_, display)) = SHELLEX_CLSID_NAMES
                    .iter()
                    .find(|&&(clsid, _)| clsid.eq_ignore_ascii_case(key_name))
                {
                    return display.to_string();
                }
                if let Some(display) = source::clsid_display_name(key_name) {
                    return display;
                }
            }
            key_name.to_string()
        }
        EntryType::Modern => key_name.to_string(),
    }
}

/// Scan ProgID and file-extension-specific context menu entries.
/// These capture entries like "Edit in Notepad" (under txtfile\shell\edit)
/// or app-specific entries registered under their ProgID.
fn scan_progid_entries(
    filter: Option<&Location>,
    entries: &mut Vec<MenuEntry>,
    seen: &mut HashSet<String>,
    source_cache: &mut HashMap<String, String>,
) {
    // ProgID entries only apply to files
    if let Some(f) = filter {
        if *f != Location::Files {
            return;
        }
    }

    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);

    // Collect ProgIDs and file extensions from HKCR
    let mut progids: HashSet<String> = HashSet::new();
    let mut extensions: Vec<String> = Vec::new();
    if let Ok(hkcr_key) = hkcr.open_subkey_with_flags("", KEY_READ) {
        for ext_name in hkcr_key.enum_keys().filter_map(|k| k.ok()) {
            if !ext_name.starts_with('.') {
                continue;
            }
            extensions.push(ext_name.clone());
            // Read the default value to get the ProgID
            if let Ok(ext_key) = hkcr.open_subkey_with_flags(&ext_name, KEY_READ) {
                if let Ok(progid) = ext_key.get_value::<String, _>("") {
                    let progid = progid.trim().to_string();
                    if !progid.is_empty() {
                        progids.insert(progid);
                    }
                }
            }
        }
    }

    // Also add "Applications\*" entries
    if let Ok(apps_key) = hkcr.open_subkey_with_flags("Applications", KEY_READ) {
        for app_name in apps_key.enum_keys().filter_map(|k| k.ok()) {
            progids.insert(format!("Applications\\{app_name}"));
        }
    }

    // Scan ProgIDs and SystemFileAssociations\.<ext> paths
    // Combine both into a single set of paths to scan
    let mut scan_roots: Vec<String> = progids.into_iter().collect();
    for ext in &extensions {
        scan_roots.push(format!("SystemFileAssociations\\{ext}"));
    }

    for root in &scan_roots {
        for (suffix, entry_type) in [
            ("shell", EntryType::Shell),
            ("shellex\\ContextMenuHandlers", EntryType::ShellEx),
        ] {
            let path = format!("{}\\{}", root, suffix);
            let parent = match hkcr.open_subkey_with_flags(&path, KEY_READ) {
                Ok(key) => key,
                Err(_) => continue,
            };

            for name in parent.enum_keys().filter_map(|k| k.ok()) {
                // Dedup by lowercase name + entry type
                let dedup_key = format!("{}:{}", name.to_lowercase(), entry_type);
                if seen.contains(&dedup_key) {
                    continue;
                }

                let subkey = match parent.open_subkey_with_flags(&name, KEY_READ) {
                    Ok(key) => key,
                    Err(_) => continue,
                };

                let registry_path = format!("{}\\{}", path, name);
                let disabled = is_disabled(&registry_path);

                let command = match entry_type {
                    EntryType::Shell => subkey
                        .open_subkey_with_flags("command", KEY_READ)
                        .ok()
                        .and_then(|cmd_key| cmd_key.get_value::<String, _>("").ok())
                        .or_else(|| {
                            subkey
                                .get_value::<String, _>("DelegateExecute")
                                .ok()
                                .map(|clsid| format!("delegate:{clsid}"))
                        }),
                    EntryType::ShellEx | EntryType::Modern => {
                        subkey.get_value::<String, _>("").ok()
                    }
                };

                let source_name = source::resolve_source(
                    &entry_type,
                    &name,
                    &command,
                    source_cache,
                );

                seen.insert(dedup_key);

                let display_name =
                    resolve_entry_display_name(&subkey, &entry_type, &name);

                entries.push(MenuEntry {
                    name: display_name,
                    registry_path,
                    entry_type: entry_type.clone(),
                    location: Location::Files,
                    status: if disabled {
                        Status::Disabled
                    } else {
                        Status::Enabled
                    },
                    command,
                    source: source_name,
                });
            }
        }
    }
}

pub fn scan_entries(filter: Option<&Location>) -> Result<Vec<MenuEntry>> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let mut entries = Vec::new();
    let mut source_cache: HashMap<String, String> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();

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
                    .and_then(|cmd_key| cmd_key.get_value::<String, _>("").ok())
                    .or_else(|| {
                        // Fallback: some shell verbs use DelegateExecute instead of command
                        subkey
                            .get_value::<String, _>("DelegateExecute")
                            .ok()
                            .map(|clsid| format!("delegate:{clsid}"))
                    }),
                EntryType::ShellEx | EntryType::Modern => {
                    subkey.get_value::<String, _>("").ok()
                }
            };

            let source = source::resolve_source(
                &target.entry_type,
                &name,
                &command,
                &mut source_cache,
            );

            // Track for deduplication with ProgID scan
            let dedup_key = format!("{}:{}", name.to_lowercase(), target.entry_type);
            seen.insert(dedup_key);

            let display_name =
                resolve_entry_display_name(&subkey, &target.entry_type, &name);

            entries.push(MenuEntry {
                name: display_name,
                registry_path,
                entry_type: target.entry_type.clone(),
                location: target.location.clone(),
                status: if disabled {
                    Status::Disabled
                } else {
                    Status::Enabled
                },
                command,
                source,
            });
        }
    }

    // Scan ProgID and file-extension-specific entries
    scan_progid_entries(filter, &mut entries, &mut seen, &mut source_cache);

    // Scan Windows 11 modern context menu entries (hardcoded well-known)
    scan_modern_entries(filter, &mut entries);

    // Dynamically discover additional modern entries from PackagedCom
    let mut known_clsids: HashSet<String> = MODERN_ENTRIES
        .iter()
        .map(|(_, clsid, _)| clsid.to_lowercase())
        .collect();
    // Also exclude CLSIDs already captured as ShellEx entries
    for entry in &entries {
        if let Some(ref cmd) = entry.command {
            let cmd_trimmed = cmd.trim();
            if cmd_trimmed.starts_with('{') {
                known_clsids.insert(cmd_trimmed.to_lowercase());
            }
        }
    }
    scan_dynamic_modern_entries(filter, &mut entries, &known_clsids);

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

/// Extract the last path segment (registry key name) from a registry path.
fn registry_key_name(registry_path: &str) -> &str {
    registry_path
        .strip_prefix("blocked:")
        .unwrap_or(registry_path)
        .rsplit('\\')
        .next()
        .unwrap_or(registry_path)
}

pub fn disable_entry(name: &str) -> Result<()> {
    let entries = scan_entries(None)?;
    let entry = entries
        .iter()
        .find(|e| {
            e.name.eq_ignore_ascii_case(name)
                || registry_key_name(&e.registry_path).eq_ignore_ascii_case(name)
        })
        .with_context(|| format!("Entry '{name}' not found"))?;

    if entry.status == Status::Disabled {
        bail!("Entry '{}' is already disabled", entry.name);
    }

    toggle_entry(&entry.registry_path)?;
    Ok(())
}

pub fn enable_entry(name: &str) -> Result<()> {
    let entries = scan_entries(None)?;
    let entry = entries
        .iter()
        .find(|e| {
            e.name.eq_ignore_ascii_case(name)
                || registry_key_name(&e.registry_path).eq_ignore_ascii_case(name)
        })
        .with_context(|| format!("Entry '{name}' not found"))?;

    if entry.status == Status::Enabled {
        bail!("Entry '{}' is already enabled", entry.name);
    }

    toggle_entry(&entry.registry_path)?;
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

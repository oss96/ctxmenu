use std::collections::HashMap;
use std::path::Path;

use winreg::enums::*;
use winreg::RegKey;

use crate::registry::EntryType;

/// Resolve the source program name for a context menu entry.
pub fn resolve_source(
    entry_type: &EntryType,
    entry_name: &str,
    command: &Option<String>,
    cache: &mut HashMap<String, String>,
) -> String {
    if *entry_type == EntryType::Modern {
        return "Windows".to_string();
    }

    match entry_type {
        EntryType::Shell => {
            let Some(cmd) = command else {
                // Shell entries with no command are built-in Windows shell verbs
                return "Windows".to_string();
            };
            if let Some(exe_path) = extract_exe_path(cmd) {
                if let Some(cached) = cache.get(&exe_path.to_lowercase()) {
                    return cached.clone();
                }
                let name = get_product_name(&exe_path)
                    .unwrap_or_else(|| filename_stem(&exe_path));
                cache.insert(exe_path.to_lowercase(), name.clone());
                name
            } else {
                // cmd.exe wrappers and other unparseable commands are likely Windows
                "Windows".to_string()
            }
        }
        EntryType::ShellEx => {
            // The CLSID may be in the command (default value) or the entry name itself
            let clsid = command
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| s.starts_with('{'))
                .or_else(|| {
                    if entry_name.starts_with('{') { Some(entry_name) } else { None }
                });

            let Some(clsid) = clsid else {
                return "Unknown".to_string();
            };

            if let Some(dll_path) = resolve_clsid_to_path(clsid) {
                let key = dll_path.to_lowercase();
                if let Some(cached) = cache.get(&key) {
                    return cached.clone();
                }
                let name = get_product_name(&dll_path)
                    .unwrap_or_else(|| clsid_display_name(clsid)
                        .unwrap_or_else(|| filename_stem(&dll_path)));
                cache.insert(key, name.clone());
                name
            } else {
                clsid_display_name(clsid).unwrap_or_else(|| "Unknown".to_string())
            }
        }
        EntryType::Modern => unreachable!(),
    }
}

/// Extract an executable/DLL path from a shell command string.
fn extract_exe_path(command: &str) -> Option<String> {
    let expanded = expand_env_vars(command);
    let trimmed = expanded.trim();

    if trimmed.is_empty() {
        return None;
    }

    let raw_path = if trimmed.starts_with('"') {
        // Quoted path: extract between first pair of quotes
        let end = trimmed[1..].find('"')?;
        &trimmed[1..1 + end]
    } else {
        // Unquoted: take everything up to first space
        trimmed.split_whitespace().next()?
    };

    let path = raw_path.trim();
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Special-case rundll32: the real binary is the first argument
    if stem == "rundll32" {
        return extract_rundll32_target(&expanded);
    }

    // Skip cmd wrappers — too ambiguous
    if stem == "cmd" {
        return None;
    }

    if path.contains('\\') || path.contains('/') || path.contains(':') {
        Some(path.to_string())
    } else {
        Some(path.to_string())
    }
}

/// For `rundll32.exe some.dll,Function`, extract `some.dll` path.
fn extract_rundll32_target(command: &str) -> Option<String> {
    let lower = command.to_lowercase();
    let idx = lower.find("rundll32")?;
    let after = command[idx..].split_whitespace().skip(1).next()?;
    // The argument is `path.dll,FunctionName` — strip the function part
    let dll = after.split(',').next()?;
    let expanded = expand_env_vars(dll.trim_matches('"'));
    if expanded.trim().is_empty() {
        None
    } else {
        Some(expanded)
    }
}

/// Resolve a CLSID to the backing DLL or EXE path via the registry.
fn resolve_clsid_to_path(clsid: &str) -> Option<String> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);

    for subkey in &["InprocServer32", "LocalServer32"] {
        let path = format!(r"CLSID\{}\{}", clsid, subkey);
        if let Ok(key) = hkcr.open_subkey_with_flags(&path, KEY_READ) {
            if let Ok(val) = key.get_value::<String, _>("") {
                let val = val.trim().to_string();
                if val.is_empty() {
                    continue;
                }
                let expanded = expand_env_vars(&val);
                // Strip quotes and arguments
                let clean = strip_path_args(&expanded);
                if !clean.is_empty() {
                    return Some(clean);
                }
            }
        }
    }
    None
}

/// Read the human-readable display name from `HKCR\CLSID\{clsid}` default value.
fn clsid_display_name(clsid: &str) -> Option<String> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let path = format!(r"CLSID\{}", clsid);
    let key = hkcr.open_subkey_with_flags(&path, KEY_READ).ok()?;
    let val: String = key.get_value("").ok()?;
    let trimmed = val.trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

/// Get the ProductName (or FileDescription) from a PE binary's version resource.
fn get_product_name(file_path: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let wide_path: Vec<u16> = file_path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut handle: u32 = 0;
        let size = GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide_path.as_ptr(), 0, size, buffer.as_mut_ptr() as *mut _) == 0 {
            return None;
        }

        // Query translation table
        let mut trans_ptr: *mut u8 = std::ptr::null_mut();
        let mut trans_len: u32 = 0;
        let translation_key: Vec<u16> = "\\VarFileInfo\\Translation\0"
            .encode_utf16()
            .collect();

        if VerQueryValueW(
            buffer.as_ptr() as *const _,
            translation_key.as_ptr(),
            &mut trans_ptr as *mut *mut u8 as *mut *mut _,
            &mut trans_len,
        ) == 0
            || trans_len < 4
        {
            // No translation table — try neutral fallback
            return query_string_value(&buffer, 0x0409, 0x04B0, "ProductName")
                .or_else(|| query_string_value(&buffer, 0x0409, 0x04B0, "FileDescription"))
                .or_else(|| query_string_value(&buffer, 0x0000, 0x04B0, "ProductName"));
        }

        // Each translation entry is 4 bytes: u16 lang_id + u16 code_page
        let translations = std::slice::from_raw_parts(
            trans_ptr as *const [u16; 2],
            trans_len as usize / 4,
        );

        for &[lang_id, code_page] in translations {
            if let Some(name) = query_string_value(&buffer, lang_id, code_page, "ProductName") {
                return Some(name);
            }
            if let Some(name) = query_string_value(&buffer, lang_id, code_page, "FileDescription") {
                return Some(name);
            }
        }

        None
    }
}

/// Query a single string value from the version info buffer.
fn query_string_value(
    buffer: &[u8],
    lang_id: u16,
    code_page: u16,
    key_name: &str,
) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::VerQueryValueW;

    let sub_block = format!("\\StringFileInfo\\{:04x}{:04x}\\{}\0", lang_id, code_page, key_name);
    let wide_sub: Vec<u16> = sub_block.encode_utf16().collect();

    let mut value_ptr: *mut u8 = std::ptr::null_mut();
    let mut value_len: u32 = 0;

    if unsafe {
        VerQueryValueW(
            buffer.as_ptr() as *const _,
            wide_sub.as_ptr(),
            &mut value_ptr as *mut *mut u8 as *mut *mut _,
            &mut value_len,
        )
    } == 0
        || value_len == 0
    {
        return None;
    }

    let slice = unsafe { std::slice::from_raw_parts(value_ptr as *const u16, value_len as usize) };
    let s = String::from_utf16_lossy(slice)
        .trim_end_matches('\0')
        .trim()
        .to_string();

    if s.is_empty() { None } else { Some(s) }
}

/// Expand `%VAR%` patterns in a string using environment variables.
fn expand_env_vars(s: &str) -> String {
    let mut result = s.to_string();
    while let Some(start) = result.find('%') {
        if let Some(end) = result[start + 1..].find('%') {
            let var_name = &result[start + 1..start + 1 + end];
            if var_name.is_empty() {
                break;
            }
            let replacement = std::env::var(var_name).unwrap_or_default();
            result = format!("{}{}{}", &result[..start], replacement, &result[start + 2 + end..]);
        } else {
            break;
        }
    }
    result
}

/// Extract the file path from a string that may include arguments.
fn strip_path_args(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with('"') {
        if let Some(end) = trimmed[1..].find('"') {
            return trimmed[1..1 + end].to_string();
        }
    }
    // For unquoted paths, heuristic: find the last segment that looks like a file extension
    // then take everything up to and including that
    if let Some(ext_pos) = trimmed.to_lowercase().find(".dll") {
        return trimmed[..ext_pos + 4].to_string();
    }
    if let Some(ext_pos) = trimmed.to_lowercase().find(".exe") {
        return trimmed[..ext_pos + 4].to_string();
    }
    trimmed.split_whitespace().next().unwrap_or("").to_string()
}

/// Get the filename stem from a path.
fn filename_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

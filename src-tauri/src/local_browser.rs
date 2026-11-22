//! Local filesystem listing for the dual-pane SFTP browser.
//!
//! The WebView receives metadata only. Directory enumeration is performed off
//! the async runtime's worker threads, and symlinks are reported but never followed.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LocalFileEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: Option<u64>,
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalDirectoryListing {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub roots: Vec<String>,
    pub entries: Vec<LocalFileEntry>,
}

#[tauri::command]
pub async fn local_list_directory(path: Option<String>) -> Result<LocalDirectoryListing, String> {
    tokio::task::spawn_blocking(move || list_directory(path))
        .await
        .map_err(|error| format!("local_list_join: {error}"))?
}


#[derive(Debug, Clone, Serialize)]
pub struct LocalDeleteFailure {
    pub path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalDeleteResult {
    pub deleted: Vec<String>,
    pub failed: Vec<LocalDeleteFailure>,
}

fn validate_local_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.trim() != name
        || name.ends_with('.')
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
        || name.chars().any(|ch| matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err("local_bad_name: Use a single file or folder name without reserved characters.".to_string());
    }
    let device = name.split('.').next().unwrap_or_default().trim_end_matches(' ').to_ascii_uppercase();
    if matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && matches!(device.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("local_bad_name: This name is reserved by Windows.".to_string());
    }
    Ok(())
}

fn canonical_local_parent(path: &Path) -> Result<PathBuf, String> {
    let parent = std::fs::canonicalize(path)
        .map_err(|error| format!("local_parent_invalid: {}: {error}", path.display()))?;
    let metadata = std::fs::metadata(&parent)
        .map_err(|error| format!("local_parent_metadata_failed: {}: {error}", parent.display()))?;
    if !metadata.is_dir() {
        return Err(format!("local_parent_not_directory: {}", parent.display()));
    }
    Ok(parent)
}

fn local_child_path(parent: &Path, name: &str) -> Result<PathBuf, String> {
    validate_local_name(name)?;
    Ok(parent.join(name))
}

fn resolve_local_entry(path: &str) -> Result<(PathBuf, std::fs::Metadata), String> {
    let requested = PathBuf::from(path);
    let name = requested.file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("local_entry_invalid: {path}"))?;
    validate_local_name(name)?;
    let parent = requested.parent()
        .ok_or_else(|| format!("local_entry_parent_invalid: {path}"))?;
    let canonical_parent = canonical_local_parent(parent)?;
    let target = local_child_path(&canonical_parent, name)?;
    let metadata = std::fs::symlink_metadata(&target)
        .map_err(|error| format!("local_entry_metadata_failed: {}: {error}", target.display()))?;
    Ok((target, metadata))
}

fn create_local_directory(parent_path: &str, name: &str) -> Result<(), String> {
    let parent = canonical_local_parent(Path::new(parent_path))?;
    let target = local_child_path(&parent, name)?;
    match std::fs::symlink_metadata(&target) {
        Ok(_) => return Err(format!("local_already_exists: {}", target.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("local_entry_metadata_failed: {}: {error}", target.display())),
    }
    std::fs::create_dir(&target)
        .map_err(|error| format!("local_create_directory_failed: {}: {error}", target.display()))
}

fn rename_local_entry(path: &str, new_name: &str) -> Result<(), String> {
    let (source, metadata) = resolve_local_entry(path)?;
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err("local_rename_unsupported: Symbolic links and special files cannot be renamed here.".to_string());
    }
    let parent = source.parent().ok_or_else(|| "local_entry_parent_invalid".to_string())?;
    let target = local_child_path(parent, new_name)?;
    if target == source {
        return Ok(());
    }
    match std::fs::symlink_metadata(&target) {
        Ok(_) => return Err(format!("local_already_exists: {}", target.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("local_entry_metadata_failed: {}: {error}", target.display())),
    }
    std::fs::rename(&source, &target)
        .map_err(|error| format!("local_rename_failed: {}: {error}", source.display()))
}

fn delete_local_entries(paths: Vec<String>) -> LocalDeleteResult {
    let mut result = LocalDeleteResult { deleted: Vec::new(), failed: Vec::new() };
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        let outcome = (|| {
            let (target, metadata) = resolve_local_entry(&path)?;
            if metadata.file_type().is_symlink() {
                return Err("local_delete_symlink: Symbolic links are not deleted from this browser.".to_string());
            }
            if metadata.is_file() {
                std::fs::remove_file(&target)
                    .map_err(|error| format!("local_delete_failed: {}: {error}", target.display()))
            } else if metadata.is_dir() {
                std::fs::remove_dir(&target)
                    .map_err(|error| format!("local_delete_failed: {}: {error}", target.display()))
            } else {
                Err("local_delete_unsupported: This file type cannot be deleted safely.".to_string())
            }
        })();
        match outcome {
            Ok(()) => result.deleted.push(path),
            Err(detail) => result.failed.push(LocalDeleteFailure { path, detail }),
        }
    }
    result
}

#[tauri::command]
pub async fn local_create_directory(parent_path: String, name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || create_local_directory(&parent_path, &name))
        .await
        .map_err(|error| format!("local_create_directory_join: {error}"))?
}

#[tauri::command]
pub async fn local_rename_entry(path: String, new_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || rename_local_entry(&path, &new_name))
        .await
        .map_err(|error| format!("local_rename_join: {error}"))?
}

#[tauri::command]
pub async fn local_delete_many(paths: Vec<String>) -> Result<LocalDeleteResult, String> {
    if paths.len() > 10_000 {
        return Err("local_delete_limit: Select no more than 10,000 items at once.".to_string());
    }
    tokio::task::spawn_blocking(move || delete_local_entries(paths))
        .await
        .map_err(|error| format!("local_delete_join: {error}"))?
}

fn default_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("USERPROFILE") {
            return PathBuf::from(path);
        }
        if let (Some(drive), Some(path)) = (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
            return PathBuf::from(drive).join(path);
        }
    }
    #[cfg(not(windows))]
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path);
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(windows)]
fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        return format!("\\\\{rest}");
    }
    value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
}

#[cfg(not(windows))]
fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn local_roots() -> Vec<String> {
    #[cfg(windows)]
    {
        return (b'A'..=b'Z')
            .filter_map(|letter| {
                let root = format!("{}:\\", letter as char);
                Path::new(&root).is_dir().then_some(root)
            })
            .collect();
    }
    #[cfg(not(windows))]
    {
        vec!["/".to_string()]
    }
}

fn is_root(path: &Path) -> bool {
    #[cfg(windows)]
    {
        path.components().count() <= 2
    }
    #[cfg(not(windows))]
    {
        path == Path::new("/")
    }
}

fn list_directory(path: Option<String>) -> Result<LocalDirectoryListing, String> {
    let requested = path.filter(|value| !value.trim().is_empty()).map(PathBuf::from).unwrap_or_else(default_path);
    let current = std::fs::canonicalize(&requested)
        .map_err(|error| format!("local_path_invalid: {}: {error}", requested.display()))?;
    let metadata = std::fs::metadata(&current)
        .map_err(|error| format!("local_metadata_failed: {}: {error}", current.display()))?;
    if !metadata.is_dir() {
        return Err(format!("local_not_directory: {}", current.display()));
    }

    let mut entries = Vec::new();
    let directory = std::fs::read_dir(&current)
        .map_err(|error| format!("local_read_directory_failed: {}: {error}", current.display()))?;
    for item in directory {
        let item = item.map_err(|error| format!("local_read_entry_failed: {}: {error}", current.display()))?;
        let entry_path = item.path();
        let metadata = std::fs::symlink_metadata(&entry_path)
            .map_err(|error| format!("local_entry_metadata_failed: {}: {error}", entry_path.display()))?;
        let file_type = metadata.file_type();
        let kind = if file_type.is_symlink() {
            "symlink"
        } else if file_type.is_dir() {
            "directory"
        } else if file_type.is_file() {
            "file"
        } else {
            "other"
        };
        let modified = metadata.modified().ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        entries.push(LocalFileEntry {
            name: item.file_name().to_string_lossy().into_owned(),
            path: display_path(&entry_path),
            kind: kind.to_string(),
            size: (kind == "file").then_some(metadata.len()),
            modified,
        });
    }

    let parent_path = if is_root(&current) {
        None
    } else {
        current.parent()
            .filter(|parent| *parent != current && !parent.as_os_str().is_empty())
            .map(|parent| parent.to_string_lossy().into_owned())
    };
    Ok(LocalDirectoryListing {
        current_path: display_path(&current),
        parent_path,
        roots: local_roots(),
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_listing_returns_metadata_and_parent_without_reading_file_contents() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("folder")).unwrap();
        std::fs::write(root.path().join("sample.txt"), b"contents are not returned").unwrap();

        let listing = list_directory(Some(root.path().to_string_lossy().into_owned())).unwrap();
        assert_eq!(listing.current_path, display_path(&std::fs::canonicalize(root.path()).unwrap()));
        assert!(listing.parent_path.is_some());
        let folder = listing.entries.iter().find(|entry| entry.name == "folder").unwrap();
        assert_eq!(folder.kind, "directory");
        assert_eq!(folder.size, None);
        let file = listing.entries.iter().find(|entry| entry.name == "sample.txt").unwrap();
        assert_eq!(file.kind, "file");
        assert_eq!(file.size, Some(b"contents are not returned".len() as u64));
        assert!(file.modified.is_some());
    }


    #[test]
    fn local_create_rename_and_multi_delete_supports_files_and_empty_folders() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().to_string_lossy().into_owned();

        create_local_directory(&parent, "created").unwrap();
        assert!(root.path().join("created").is_dir());

        let source = root.path().join("before.txt");
        std::fs::write(&source, b"data").unwrap();
        rename_local_entry(&source.to_string_lossy(), "after.txt").unwrap();
        assert!(!source.exists());
        assert!(root.path().join("after.txt").is_file());

        let folder = root.path().join("created").to_string_lossy().into_owned();
        let file = root.path().join("after.txt").to_string_lossy().into_owned();
        let result = delete_local_entries(vec![folder.clone(), file.clone()]);
        assert_eq!(result.deleted, vec![folder, file]);
        assert!(result.failed.is_empty());
        assert!(!root.path().join("created").exists());
        assert!(!root.path().join("after.txt").exists());
    }

    #[test]
    fn local_multi_delete_keeps_non_empty_directories_and_reports_partial_failures() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("not-empty");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("inside.txt"), b"keep").unwrap();
        let file = root.path().join("remove.txt");
        std::fs::write(&file, b"remove").unwrap();

        let result = delete_local_entries(vec![
            folder.to_string_lossy().into_owned(),
            file.to_string_lossy().into_owned(),
        ]);
        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert!(folder.join("inside.txt").exists());
        assert!(!file.exists());
    }

    #[test]
    fn local_roots_include_the_platform_root() {
        let roots = local_roots();
        assert!(!roots.is_empty());
        #[cfg(windows)]
        assert!(roots.iter().all(|root| root.ends_with("\\")));
        #[cfg(not(windows))]
        assert_eq!(roots, vec!["/"]);
    }
}

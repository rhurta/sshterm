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
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\\") {
        return format!("\\\\{rest}");
    }
    value.strip_prefix(r"\\?\\").unwrap_or(&value).to_string()
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
    fn local_roots_include_the_platform_root() {
        let roots = local_roots();
        assert!(!roots.is_empty());
        #[cfg(windows)]
        assert!(roots.iter().all(|root| root.ends_with("\\")));
        #[cfg(not(windows))]
        assert_eq!(roots, vec!["/"]);
    }
}

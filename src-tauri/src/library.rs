use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::persist::Store;

pub fn list(store: &Store) -> Vec<String> {
    store.snapshot().library_roots
}

pub fn add(store: &Store, path: String) -> AppResult<(Vec<String>, String)> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err(AppError::msg(format!("missing path: {}", path.display())));
    }
    if !path.is_dir() {
        return Err(AppError::msg("Add a music folder"));
    }
    let key = normalize(&path);
    store.update(|data| {
        if !data.library_roots.iter().any(|root| same_path(root, &key)) {
            data.library_roots.push(key.clone());
        }
        data.last_root = Some(key.clone());
    });
    Ok((list(store), key))
}

/// Point a library folder at a moved copy with the same name. A missing
/// folder stays in the list so it can be located.
pub fn prune_missing(store: &Store) -> bool {
    crate::relink::retarget_library(store)
}

pub fn remove(store: &Store, path: String) -> AppResult<Vec<String>> {
    store.update(|data| {
        data.library_roots.retain(|root| !same_path(root, &path));
        if data
            .last_root
            .as_ref()
            .is_some_and(|root| same_path(root, &path))
        {
            data.last_root = data.library_roots.first().cloned();
        }
    });
    Ok(list(store))
}

fn normalize(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn same_path(left: &str, right: &str) -> bool {
    Path::new(left) == Path::new(right) || normalize(Path::new(left)) == normalize(Path::new(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::Store;

    #[test]
    fn add_and_remove_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let folder = dir.path().join("Music");
        std::fs::create_dir_all(&folder).unwrap();
        let (roots, added) = add(&store, folder.to_string_lossy().to_string()).unwrap();
        assert_eq!(roots.len(), 1);
        assert!(!added.is_empty());
        let (again, _) = add(&store, folder.to_string_lossy().to_string()).unwrap();
        assert_eq!(again.len(), 1);
        let empty = remove(&store, again[0].clone()).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn prune_drops_deleted_folder_and_keeps_offline_root() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let folder = dir.path().join("Music");
        std::fs::create_dir_all(&folder).unwrap();
        add(&store, folder.to_string_lossy().to_string()).unwrap();
        std::fs::remove_dir(&folder).unwrap();
        assert!(!prune_missing(&store) || list(&store).len() == 1);
        assert_eq!(list(&store).len(), 1);
        store.update(|data| {
            data.library_roots.push("/volume-offline/Music".into());
            data.last_root = Some("/volume-offline/Music".into());
        });
        assert!(!prune_missing(&store));
        assert!(list(&store)
            .iter()
            .any(|root| root.contains("volume-offline")));
    }

    #[test]
    fn rejects_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let file = dir.path().join("song.mp3");
        std::fs::write(&file, []).unwrap();
        assert!(add(&store, file.to_string_lossy().to_string()).is_err());
    }
}

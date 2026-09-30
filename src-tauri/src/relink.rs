use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::error::{AppError, AppResult};
use crate::persist::{Playlist, PlaylistItem, Store};
use crate::player::scan::{hidden, is_audio_path};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingItem {
    pub scope: String,
    pub id: String,
    pub path: String,
    pub kind: String,
    pub label: String,
}

pub fn list_missing(store: &Store) -> Vec<MissingItem> {
    let data = store.snapshot();
    let mut missing = Vec::new();
    for playlist in &data.playlists {
        for item in &playlist.items {
            if item.kind == "exclude" {
                continue;
            }
            let path = Path::new(&item.path);
            if item.kind == "dir" {
                if dir_gone(path) {
                    missing.push(missing_item("playlist", &playlist.id, &item.path, "dir"));
                }
            } else if file_gone(path) {
                missing.push(missing_item("playlist", &playlist.id, &item.path, "file"));
            }
        }
    }
    for root in &data.library_roots {
        if dir_gone(Path::new(root)) {
            missing.push(missing_item("library", root, root, "dir"));
        }
    }
    missing
}

pub fn retarget_playlists(playlists: &mut [Playlist], library_roots: &[String]) {
    if !playlists
        .iter()
        .any(|playlist| playlist.items.iter().any(item_missing))
    {
        return;
    }
    let (files, dirs) = index_places(&search_places(library_roots, playlists));
    for playlist in playlists {
        for item in &mut playlist.items {
            retarget_item(item, &files, &dirs);
        }
    }
}

pub fn retarget_library(store: &Store) -> bool {
    let data = store.snapshot();
    let missing: Vec<String> = data
        .library_roots
        .iter()
        .filter(|root| dir_gone(Path::new(root)))
        .cloned()
        .collect();
    if missing.is_empty() {
        return false;
    }
    let places = search_places(&data.library_roots, &data.playlists);
    let (_, dirs) = index_places(&places);
    let mut roots = data.library_roots.clone();
    let mut last = data.last_root.clone();
    let mut changed = false;
    for root in &mut roots {
        if !dir_gone(Path::new(root)) {
            continue;
        }
        let Some(name) = Path::new(root).file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(found) = unique(dirs.get(&name.to_lowercase())) else {
            continue;
        };
        if found == *root {
            continue;
        }
        if last.as_deref() == Some(root.as_str()) {
            last = Some(found.clone());
        }
        *root = found;
        changed = true;
    }
    if !changed {
        return false;
    }
    store.update(|saved| {
        saved.library_roots = roots;
        saved.last_root = last;
    });
    true
}

pub fn relink(store: &Store, scope: &str, id: &str, from: &str, to: &str) -> AppResult<()> {
    let dest = PathBuf::from(to);
    if !dest.exists() {
        return Err(AppError::msg("that path was not found"));
    }
    match scope {
        "library" => relink_library(store, from, &dest),
        "playlist" => relink_playlist(store, id, from, &dest),
        _ => Err(AppError::msg("nothing to update")),
    }
}

fn relink_library(store: &Store, from: &str, dest: &Path) -> AppResult<()> {
    if !dest.is_dir() {
        return Err(AppError::msg("Choose the album folder"));
    }
    let next = dest.to_string_lossy().to_string();
    let mut found = false;
    store.update(|data| {
        for root in &mut data.library_roots {
            if root == from {
                *root = next.clone();
                found = true;
            }
        }
        if data.last_root.as_deref() == Some(from) {
            data.last_root = Some(next);
        }
    });
    if !found {
        return Err(AppError::msg("that library folder is gone"));
    }
    Ok(())
}

fn relink_playlist(store: &Store, id: &str, from: &str, dest: &Path) -> AppResult<()> {
    let files = if dest.is_dir() {
        crate::player::scan::audio_paths(dest).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut found = false;
    let mut updated = false;
    store.update(|data| {
        let Some(playlist) = data.playlists.iter_mut().find(|playlist| playlist.id == id) else {
            return;
        };
        found = true;
        if dest.is_dir() {
            for item in &mut playlist.items {
                if item.kind == "dir" && item.path == from {
                    item.path = dest.to_string_lossy().to_string();
                    updated = true;
                }
            }
            for item in &mut playlist.items {
                if item.kind == "exclude" || item.kind == "dir" || Path::new(&item.path).is_file() {
                    continue;
                }
                let Some(name) = Path::new(&item.path).file_name() else {
                    continue;
                };
                if let Some(match_path) = files.iter().find(|file| file.file_name() == Some(name)) {
                    item.path = match_path.to_string_lossy().to_string();
                    item.kind = "file".into();
                    updated = true;
                }
            }
            return;
        }
        for item in &mut playlist.items {
            if item.path == from && item.kind != "exclude" {
                item.path = dest.to_string_lossy().to_string();
                item.kind = "file".into();
                updated = true;
            }
        }
    });
    if !found {
        return Err(AppError::msg("playlist not found"));
    }
    if !updated {
        return Err(AppError::msg("nothing matched that location"));
    }
    Ok(())
}

fn retarget_item(
    item: &mut PlaylistItem,
    files: &HashMap<String, Vec<String>>,
    dirs: &HashMap<String, Vec<String>>,
) {
    if item.kind == "exclude" || !item_missing(item) {
        return;
    }
    let Some(name) = Path::new(&item.path)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return;
    };
    let key = name.to_lowercase();
    let found = if item.kind == "dir" {
        unique(dirs.get(&key))
    } else {
        unique(files.get(&key))
    };
    if let Some(found) = found {
        if found != item.path {
            item.path = found;
        }
    }
}

fn item_missing(item: &PlaylistItem) -> bool {
    let path = Path::new(&item.path);
    if item.kind == "exclude" {
        return false;
    }
    if item.kind == "dir" {
        dir_gone(path)
    } else {
        file_gone(path)
    }
}

fn file_gone(path: &Path) -> bool {
    !path.is_file() && path.parent().is_some_and(|parent| parent.is_dir())
}

fn dir_gone(path: &Path) -> bool {
    !path.is_dir() && path.parent().is_some_and(|parent| parent.is_dir())
}

fn missing_item(scope: &str, id: &str, path: &str, kind: &str) -> MissingItem {
    let label = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string();
    MissingItem {
        scope: scope.into(),
        id: id.into(),
        path: path.into(),
        kind: kind.into(),
        label,
    }
}

fn search_places(library_roots: &[String], playlists: &[Playlist]) -> Vec<PathBuf> {
    let mut places = Vec::new();
    for root in library_roots {
        push_place(&mut places, PathBuf::from(root));
    }
    for playlist in playlists {
        for item in &playlist.items {
            if item.kind == "dir" {
                push_place(&mut places, PathBuf::from(&item.path));
            }
        }
    }
    places
}

fn push_place(places: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !places.iter().any(|item| item == &path) {
        places.push(path);
    }
}

fn index_places(
    places: &[PathBuf],
) -> (HashMap<String, Vec<String>>, HashMap<String, Vec<String>>) {
    let mut files: HashMap<String, Vec<String>> = HashMap::new();
    let mut dirs: HashMap<String, Vec<String>> = HashMap::new();
    for place in places {
        for entry in WalkDir::new(place).max_depth(5).follow_links(false) {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            let nested = path.strip_prefix(place).is_ok_and(|relative| {
                relative
                    .components()
                    .any(|component| hidden(Path::new(component.as_os_str())))
            });
            if nested {
                continue;
            }
            if entry.file_type().is_dir() {
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    push_named(&mut dirs, name, path);
                }
                continue;
            }
            if is_audio_path(path) {
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    push_named(&mut files, name, path);
                }
            }
        }
    }
    (files, dirs)
}

fn push_named(index: &mut HashMap<String, Vec<String>>, name: &str, path: &Path) {
    let key = name.to_lowercase();
    let text = path.to_string_lossy().to_string();
    let bucket = index.entry(key).or_default();
    if !bucket.iter().any(|item| item == &text) {
        bucket.push(text);
    }
}

fn unique(matches: Option<&Vec<String>>) -> Option<String> {
    let matches = matches?;
    if matches.len() == 1 {
        matches.first().cloned()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::Store;
    use crate::playlists::{self, flatten_paths};

    #[test]
    fn moved_song_is_found_by_its_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let from = dir.path().join("old");
        let into = dir.path().join("library");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&into).unwrap();
        let song = from.join("kept.mp3");
        std::fs::write(&song, []).unwrap();
        let playlists = playlists::create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        store.update(|data| {
            data.library_roots.push(into.to_string_lossy().to_string());
            data.playlists[0].items = vec![PlaylistItem {
                path: song.to_string_lossy().into(),
                kind: "file".into(),
            }];
        });
        let moved = into.join("kept.mp3");
        std::fs::rename(&song, &moved).unwrap();
        assert!(playlists::sync(&store));
        let songs = flatten_paths(&store.snapshot().playlists[0]);
        assert_eq!(songs.len(), 1);
        assert!(songs[0].ends_with("kept.mp3"));
        assert!(songs[0].contains("library"));
        assert!(list_missing(&store).iter().all(|item| item.id != id));
    }

    #[test]
    fn two_copies_are_left_for_the_user() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let from = dir.path().join("old");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let song = from.join("kept.mp3");
        std::fs::write(&song, []).unwrap();
        std::fs::write(a.join("kept.mp3"), []).unwrap();
        std::fs::write(b.join("kept.mp3"), []).unwrap();
        playlists::create(&store, "Late".into()).unwrap();
        store.update(|data| {
            data.library_roots.push(a.to_string_lossy().to_string());
            data.library_roots.push(b.to_string_lossy().to_string());
            data.playlists[0].items = vec![PlaylistItem {
                path: song.to_string_lossy().into(),
                kind: "file".into(),
            }];
        });
        std::fs::remove_file(&song).unwrap();
        let _ = playlists::sync(&store);
        let missing = list_missing(&store);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].kind, "file");
        assert!(relink(
            &store,
            "playlist",
            &store.snapshot().playlists[0].id,
            &song.to_string_lossy(),
            &a.join("kept.mp3").to_string_lossy(),
        )
        .is_ok());
        assert!(list_missing(&store).is_empty());
    }

    #[test]
    fn a_library_root_does_not_search_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let library = dir.path().join("library");
        let sibling = dir.path().join("Videos").join("music");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let lost = library.join("gone.mp3");
        std::fs::write(&lost, []).unwrap();
        playlists::create(&store, "Late".into()).unwrap();
        store.update(|data| {
            data.library_roots.push(library.to_string_lossy().into());
            data.playlists[0].items = vec![PlaylistItem {
                path: lost.to_string_lossy().into(),
                kind: "file".into(),
            }];
        });
        std::fs::remove_file(&lost).unwrap();
        std::fs::write(sibling.join("gone.mp3"), []).unwrap();
        let _ = playlists::sync(&store);
        let missing = list_missing(&store);
        assert_eq!(
            missing.len(),
            1,
            "must not steal a song from outside the library"
        );
        assert!(missing[0].path.contains("gone.mp3"));
    }
}

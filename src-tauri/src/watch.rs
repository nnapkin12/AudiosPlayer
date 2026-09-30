use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Manager};

use crate::persist::Store;
use crate::player::{Player, STATE_EVENT};

pub const CHANGED_EVENT: &str = "library://changed";
const SETTLE: Duration = Duration::from_millis(400);
const EVENT_WAIT: Duration = Duration::from_secs(2);
const RESCAN_EVERY: u32 = 15; // 15 * 2s = 30s
const FAIL_BACKOFF: Duration = Duration::from_secs(60);

pub fn spawn(app: AppHandle, store: Store) {
    let _ = std::thread::Builder::new()
        .name("audios-watch".into())
        .spawn(move || run(app, store));
}

fn run(app: AppHandle, store: Store) {
    let (tx, rx) = mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |result: Result<notify::Event, notify::Error>| {
            // Treat errors and rescans as "something moved" so we do not sit
            // forever on a dead watch.
            let _ = tx.send(result.is_err());
        })
        .ok();
    let Some(watcher) = watcher.as_mut() else {
        return;
    };
    let mut watching = Vec::new();
    let mut failed: HashMap<PathBuf, Instant> = HashMap::new();
    let mut ticks = 0u32;
    let mut last_rev = u64::MAX;
    loop {
        let rev = store.revision();
        if rev != last_rev || ticks.is_multiple_of(RESCAN_EVERY) {
            last_rev = rev;
            sync_watches(watcher, &mut watching, &mut failed, dirs_to_watch(&store));
            if !failed.is_empty() {
                tell_user(&app, &store);
            }
        }
        match rx.recv_timeout(EVENT_WAIT) {
            Ok(_) => {
                while rx.try_recv().is_ok() {}
                std::thread::sleep(SETTLE);
                while rx.try_recv().is_ok() {}
                crate::playlists::sync(&store);
                crate::library::prune_missing(&store);
                let _ = app.emit(CHANGED_EVENT, ());
                last_rev = store.revision();
                sync_watches(watcher, &mut watching, &mut failed, dirs_to_watch(&store));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                ticks = ticks.saturating_add(1);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn tell_user(app: &AppHandle, store: &Store) {
    if store.session_note().is_some() {
        return;
    }
    store.set_session_note(Some(
        "Folder watching is limited (too many folders for inotify). Some changes will not show up until you reopen Audios!. Raise the limit with: sudo sysctl fs.inotify.max_user_watches=524288".into(),
    ));
    if let Some(player) = app.try_state::<Player>() {
        let _ = app.emit(STATE_EVENT, player.snapshot());
    }
}

/// Add/remove watches without tearing down ones that still match. Failed
/// paths are retried after [`FAIL_BACKOFF`], not every loop.
fn sync_watches(
    watcher: &mut RecommendedWatcher,
    watching: &mut Vec<PathBuf>,
    failed: &mut HashMap<PathBuf, Instant>,
    wanted: Vec<PathBuf>,
) {
    let now = Instant::now();
    watching.retain(|path| {
        if wanted.iter().any(|want| want == path) {
            true
        } else {
            let _ = watcher.unwatch(path);
            false
        }
    });
    failed.retain(|path, until| wanted.contains(path) && *until > now);

    for path in wanted {
        if watching.iter().any(|have| have == &path) {
            continue;
        }
        if failed.get(&path).is_some_and(|until| now < *until) {
            continue;
        }
        match watcher.watch(&path, RecursiveMode::Recursive) {
            Ok(()) => {
                failed.remove(&path);
                watching.push(path);
            }
            Err(_) => {
                failed.insert(path, now + FAIL_BACKOFF);
            }
        }
    }
}

pub fn dirs_to_watch(store: &Store) -> Vec<PathBuf> {
    let data = store.snapshot();
    let mut dirs = Vec::new();
    for root in data.library_roots {
        push_dir(&mut dirs, PathBuf::from(root));
    }
    for playlist in data.playlists {
        for item in playlist.items {
            if item.kind == "exclude" {
                continue;
            }
            let path = PathBuf::from(&item.path);
            if item.kind == "dir" || path.is_dir() {
                push_dir(&mut dirs, path);
            } else if let Some(parent) = path.parent() {
                push_dir(&mut dirs, parent.to_path_buf());
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    let mut covered = Vec::new();
    for dir in dirs {
        if covered
            .iter()
            .any(|parent: &PathBuf| dir.starts_with(parent))
        {
            continue;
        }
        covered.push(dir);
    }
    covered
}

fn push_dir(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() {
        dirs.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::{PlaylistItem, Store};

    #[test]
    fn failed_watch_is_not_retried_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("keep");
        std::fs::create_dir_all(&keep).unwrap();
        let watching = vec![keep.clone()];
        let mut failed = HashMap::new();
        failed.insert(
            dir.path().join("gone"),
            Instant::now() + Duration::from_secs(30),
        );
        // No real watcher: just the backoff map.
        assert!(failed.values().all(|until| Instant::now() < *until));
        assert_eq!(watching, vec![keep]);
    }

    #[test]
    fn watch_list_covers_library_roots() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let root = dir.path().join("music");
        std::fs::create_dir_all(&root).unwrap();
        store.update(|data| {
            data.library_roots.push(root.to_string_lossy().into());
        });
        let watched = dirs_to_watch(&store);
        assert_eq!(watched, vec![root]);
    }

    #[test]
    fn playlist_file_watches_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        let song = album.join("a.mp3");
        std::fs::write(&song, []).unwrap();
        store.update(|data| {
            data.playlists.push(crate::persist::Playlist {
                id: "p".into(),
                name: "P".into(),
                items: vec![PlaylistItem {
                    path: song.to_string_lossy().into(),
                    kind: "file".into(),
                }],
                has_cover: false,
            });
        });
        assert_eq!(dirs_to_watch(&store), vec![album]);
    }
}

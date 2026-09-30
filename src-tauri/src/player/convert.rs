use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::{AppError, AppResult};
use crate::search::OutputWithin;

/// Upper bound for `~/.cache/audios/playback`. Oldest transcodes go first.
pub const PLAYBACK_CACHE_CAP_BYTES: u64 = 512 * 1024 * 1024;

// Some MP4/AAC files make Symphonia panic inside Decoder::new. Play a cached
// transcode of those files. The library file is never modified or removed.
pub fn for_playback(path: &Path) -> AppResult<PathBuf> {
    if let Some(cached) = fresh_cache(path) {
        return Ok(cached);
    }
    match super::engine::probe_audio(path) {
        Ok(()) => Ok(path.to_path_buf()),
        Err(error) if !path.is_file() => Err(error),
        Err(error) => transcode(path).map_err(|convert_error| {
            let detail = convert_error.to_string();
            if detail.to_lowercase().contains("ffmpeg") {
                AppError::msg(format!("{error}. {detail}"))
            } else {
                error
            }
        }),
    }
}

fn fresh_cache(path: &Path) -> Option<PathBuf> {
    let (mp3, wav) = cache_paths(path)?;
    [mp3, wav].into_iter().find(|candidate| {
        candidate.is_file()
            && candidate
                .metadata()
                .map(|meta| meta.len() > 0)
                .unwrap_or(false)
    })
}

fn cache_paths(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let meta = path.metadata().ok()?;
    let stamp = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .hash(&mut hasher);
    stamp.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    let id = format!("{:016x}", hasher.finish());
    let dir = playback_cache_dir();
    Some((dir.join(format!("{id}.mp3")), dir.join(format!("{id}.wav"))))
}

fn playback_cache_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = CACHE_OVERRIDE.with(|slot| slot.borrow().clone()) {
        let _ = std::fs::create_dir_all(&dir);
        return dir;
    }
    let dirs = directories::ProjectDirs::from("com", "audios", "Audios")
        .expect("a home directory is required");
    let dir = dirs.cache_dir().join("playback");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn transcode(path: &Path) -> AppResult<PathBuf> {
    if !path.is_file() {
        return Err(AppError::msg(format!("missing path: {}", path.display())));
    }
    let ffmpeg = crate::search::find_tool("ffmpeg").ok_or_else(|| {
        AppError::msg(
            "ffmpeg is required to play this file. Install it with: sudo apt install ffmpeg",
        )
    })?;
    let (mp3, wav) = cache_paths(path).ok_or_else(|| AppError::msg("could not cache playback"))?;
    std::fs::create_dir_all(playback_cache_dir())?;
    let ready = run_ffmpeg(
        &ffmpeg,
        path,
        &mp3,
        &["-codec:a", "libmp3lame", "-q:a", "4"],
    )
    .or_else(|| {
        run_ffmpeg(
            &ffmpeg,
            path,
            &wav,
            &["-acodec", "pcm_s16le", "-ar", "44100", "-ac", "2"],
        )
    });
    match ready {
        Some(ready) => {
            prune_playback_cache(PLAYBACK_CACHE_CAP_BYTES, Some(&ready));
            Ok(ready)
        }
        None => {
            let _ = std::fs::remove_file(&mp3);
            let _ = std::fs::remove_file(&wav);
            Err(AppError::msg("could not convert it for playback"))
        }
    }
}

/// ffmpeg writes to a `.part` sibling; only a finished file gets the real
/// name, so a transcode cut short by a crash is never mistaken for a cache hit.
fn run_ffmpeg(ffmpeg: &Path, source: &Path, dest: &Path, extra: &[&str]) -> Option<PathBuf> {
    let part = part_path(dest);
    let _ = std::fs::remove_file(&part);
    let output = crate::search::spawn_tool(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(source)
        .args(["-vn"])
        .args(extra)
        // ffmpeg picks the muxer from the extension, so name it explicitly.
        .args([
            "-f",
            if dest.extension().is_some_and(|e| e == "wav") {
                "wav"
            } else {
                "mp3"
            },
        ])
        .arg(&part)
        .output_within(crate::search::TOOL_TRANSCODE_TIMEOUT)
        .ok()?;
    let done = output.status.success()
        && part.is_file()
        && part.metadata().map(|meta| meta.len() > 0).unwrap_or(false);
    if done && std::fs::rename(&part, dest).is_ok() {
        return Some(dest.to_path_buf());
    }
    let _ = std::fs::remove_file(&part);
    None
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".part");
    dest.with_file_name(name)
}

/// Delete leftover `.part` files and then the oldest transcodes until the
/// cache is under `cap_bytes`. `keep` is never removed.
pub fn prune_playback_cache(cap_bytes: u64, keep: Option<&Path>) {
    prune_dir(&playback_cache_dir(), cap_bytes, keep);
}

fn prune_dir(dir: &Path, cap_bytes: u64, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if path.extension().is_some_and(|ext| ext == "part") {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        files.push((modified, meta.len(), path));
    }
    let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    if total <= cap_bytes {
        return;
    }
    files.sort_by_key(|(modified, _, _)| *modified);
    for (_, len, path) in files {
        if total <= cap_bytes {
            break;
        }
        if keep.is_some_and(|k| k == path) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

#[cfg(test)]
use std::cell::RefCell;

#[cfg(test)]
thread_local! {
    static CACHE_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_does_not_modify_the_library_file() {
        let dir = tempfile::tempdir().unwrap();
        CACHE_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(dir.path().join("cache")));
        let source = dir.path().join("song.m4a");
        std::fs::write(&source, b"not a real m4a").unwrap();
        let _ = for_playback(&source);
        assert_eq!(std::fs::read(&source).unwrap(), b"not a real m4a");
        CACHE_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    }

    fn touch(dir: &Path, name: &str, len: usize, age_secs: u64) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, vec![0u8; len]).unwrap();
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(when).unwrap();
        path
    }

    #[test]
    fn prune_drops_parts_and_oldest_files_first() {
        let dir = tempfile::tempdir().unwrap();
        let old = touch(dir.path(), "old.mp3", 100, 300);
        let mid = touch(dir.path(), "mid.mp3", 100, 200);
        let new = touch(dir.path(), "new.mp3", 100, 100);
        let part = touch(dir.path(), "half.mp3.part", 100, 10);
        prune_dir(dir.path(), 250, Some(&old));
        assert!(!part.exists(), ".part files always go");
        assert!(old.exists(), "the file being played is kept");
        assert!(!mid.exists(), "the oldest removable file goes first");
        assert!(new.exists());
    }

    #[test]
    fn prune_leaves_a_small_cache_alone() {
        let dir = tempfile::tempdir().unwrap();
        let a = touch(dir.path(), "a.mp3", 10, 10);
        prune_dir(dir.path(), 1_000, None);
        assert!(a.exists());
    }

    #[test]
    fn a_leftover_part_file_is_not_a_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        CACHE_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(dir.path().join("cache")));
        let source = dir.path().join("song.m4a");
        std::fs::write(&source, b"x").unwrap();
        let (mp3, _) = cache_paths(&source).unwrap();
        std::fs::create_dir_all(mp3.parent().unwrap()).unwrap();
        std::fs::write(part_path(&mp3), b"half").unwrap();
        assert!(fresh_cache(&source).is_none());
        CACHE_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    }
}

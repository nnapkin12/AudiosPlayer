use std::path::{Path, PathBuf};

use lofty::prelude::*;
use lofty::tag::ItemKey;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::AppResult;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "m4a", "mp4", "aac", "wav", "aiff", "aif", "wma", "wv", "ape",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track: Option<u32>,
    pub disc: Option<u32>,
    pub duration_ms: u64,
    pub folder: String,
    pub replaygain_track: Option<f64>,
    pub replaygain_album: Option<f64>,
    /// Sample peak, linear. Missing on older saved queues.
    #[serde(default)]
    pub replaygain_track_peak: Option<f64>,
    #[serde(default)]
    pub replaygain_album_peak: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderNode {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub children: Vec<FolderNode>,
}

pub fn is_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            AUDIO_EXTENSIONS
                .iter()
                .any(|ok| ext.eq_ignore_ascii_case(ok))
        })
        .unwrap_or(false)
}

pub fn hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with('.'))
        .unwrap_or(false)
}

pub fn audio_paths(path: &Path) -> AppResult<Vec<PathBuf>> {
    walk_audio_files(path)
}

fn walk_audio_files(path: &Path) -> AppResult<Vec<PathBuf>> {
    if path.is_file() {
        if !is_audio_path(path) {
            return Err(crate::error::AppError::msg(
                "That isn't a song Audios! can play",
            ));
        }
        return Ok(vec![path.to_path_buf()]);
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(path).follow_links(true) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let file_path = entry.path();
        if file_path
            .strip_prefix(path)
            .map(|relative| {
                relative
                    .components()
                    .any(|component| hidden(Path::new(component.as_os_str())))
            })
            .unwrap_or(false)
        {
            // A tag write that died mid-way leaves a hidden copy of the song.
            if crate::tags::is_stale_staging(file_path) {
                let _ = std::fs::remove_file(file_path);
            }
            continue;
        }
        if is_audio_path(file_path) {
            files.push(file_path.to_path_buf());
        }
    }
    Ok(files)
}

pub fn collect_tracks(path: &Path) -> AppResult<Vec<Track>> {
    let mut tracks: Vec<Track> = walk_audio_files(path)?
        .into_iter()
        .map(|file| track_from_path(&file))
        .collect();
    sort_tracks(&mut tracks);
    Ok(tracks)
}

pub fn collect_tracks_fast(path: &Path) -> AppResult<Vec<Track>> {
    let mut tracks: Vec<Track> = walk_audio_files(path)?
        .into_iter()
        .map(|file| track_stub(&file))
        .collect();
    sort_tracks(&mut tracks);
    Ok(tracks)
}

pub fn collect_many(paths: &[String], fast: bool) -> Vec<Track> {
    let mut tracks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        let found = if fast {
            collect_tracks_fast(Path::new(path))
        } else {
            collect_tracks(Path::new(path))
        };
        if let Ok(found) = found {
            for track in found {
                if seen.insert(track.path.clone()) {
                    tracks.push(track);
                }
            }
        }
    }
    sort_tracks(&mut tracks);
    tracks
}

pub fn sort_tracks(tracks: &mut [Track]) {
    tracks.sort_by(|a, b| {
        a.folder
            .cmp(&b.folder)
            .then(a.disc.unwrap_or(1).cmp(&b.disc.unwrap_or(1)))
            .then(
                a.track
                    .unwrap_or(u32::MAX)
                    .cmp(&b.track.unwrap_or(u32::MAX)),
            )
            .then(a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });
}

pub fn track_stub(path: &Path) -> Track {
    let folder = path
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_default();
    let fallback_title = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    Track {
        path: path.to_string_lossy().to_string(),
        title: fallback_title,
        artist: String::new(),
        album: String::new(),
        album_artist: String::new(),
        track: None,
        disc: None,
        duration_ms: 0,
        folder,
        replaygain_track: None,
        replaygain_album: None,
        replaygain_track_peak: None,
        replaygain_album_peak: None,
    }
}

pub fn track_from_path(path: &Path) -> Track {
    let mut track = track_stub(path);

    if let Ok(tagged) = crate::mp4_recover::open(path) {
        track.duration_ms = tagged.properties().duration().as_millis() as u64;
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(title) = tag.title() {
                if !title.trim().is_empty() {
                    track.title = title.to_string();
                }
            }
            if let Some(artist) = tag.artist() {
                track.artist = artist.to_string();
            }
            if let Some(album) = tag.album() {
                track.album = album.to_string();
            }
            track.album_artist = tag
                .get_string(&ItemKey::AlbumArtist)
                .unwrap_or(&track.artist)
                .to_string();
            track.track = tag.track();
            track.disc = tag.disk();
            track.replaygain_track = tag
                .get_string(&ItemKey::ReplayGainTrackGain)
                .and_then(parse_replaygain_db);
            track.replaygain_album = tag
                .get_string(&ItemKey::ReplayGainAlbumGain)
                .and_then(parse_replaygain_db);
            track.replaygain_track_peak = tag
                .get_string(&ItemKey::ReplayGainTrackPeak)
                .and_then(parse_replaygain_peak);
            track.replaygain_album_peak = tag
                .get_string(&ItemKey::ReplayGainAlbumPeak)
                .and_then(parse_replaygain_peak);
        }
    }

    track
}

#[allow(dead_code)]
pub fn build_tree(root: &Path, tracks: &[Track]) -> FolderNode {
    let root_string = root.to_string_lossy().to_string();
    let mut node = FolderNode {
        name: root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| root_string.clone()),
        path: root_string.clone(),
        kind: if root.is_file() { "file" } else { "dir" }.into(),
        children: Vec::new(),
    };

    if root.is_file() {
        return node;
    }

    let mut dirs: Vec<PathBuf> = tracks
        .iter()
        .filter_map(|track| PathBuf::from(&track.path).parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();

    for dir in dirs {
        if !dir.starts_with(root) {
            continue;
        }
        insert_dir(&mut node, root, &dir, tracks);
    }
    node
}

fn insert_dir(root_node: &mut FolderNode, root: &Path, dir: &Path, tracks: &[Track]) {
    let relative = match dir.strip_prefix(root) {
        Ok(relative) if relative.as_os_str().is_empty() => {
            attach_files(root_node, dir, tracks);
            return;
        }
        Ok(relative) => relative.to_path_buf(),
        Err(_) => return,
    };

    let mut current = root_node;
    let mut prefix = root.to_path_buf();
    for part in relative.components() {
        prefix.push(part);
        let name = part.as_os_str().to_string_lossy().to_string();
        let path = prefix.to_string_lossy().to_string();
        if let Some(index) = current.children.iter().position(|child| child.path == path) {
            current = &mut current.children[index];
        } else {
            current.children.push(FolderNode {
                name,
                path,
                kind: "dir".into(),
                children: Vec::new(),
            });
            current = current.children.last_mut().unwrap();
        }
    }
    attach_files(current, dir, tracks);
}

fn attach_files(node: &mut FolderNode, dir: &Path, tracks: &[Track]) {
    let dir_string = dir.to_string_lossy();
    for track in tracks {
        if track.folder != dir_string {
            continue;
        }
        if node.children.iter().any(|child| child.path == track.path) {
            continue;
        }
        node.children.push(FolderNode {
            name: Path::new(&track.path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| track.title.clone()),
            path: track.path.clone(),
            kind: "file".into(),
            children: Vec::new(),
        });
    }
}

pub fn parse_replaygain_db(value: &str) -> Option<f64> {
    let cleaned = value.replace("dB", "").replace("db", "");
    cleaned.trim().parse::<f64>().ok()
}

pub fn parse_replaygain_peak(value: &str) -> Option<f64> {
    let peak = value.trim().parse::<f64>().ok()?;
    if peak.is_finite() && peak > 0.0 {
        Some(peak)
    } else {
        None
    }
}

fn leveled_gain(db: Option<f64>, peak: Option<f64>) -> f64 {
    let mut gain = db.map(|db| 10f64.powf(db / 20.0)).unwrap_or(1.0);
    if let Some(peak) = peak.filter(|peak| peak.is_finite() && *peak > 0.0) {
        if gain * peak > 1.0 {
            gain = 1.0 / peak;
        }
    }
    if !gain.is_finite() {
        return 1.0;
    }
    gain.clamp(0.05, 3.0)
}

pub fn replaygain_multiplier(track: &Track) -> f64 {
    leveled_gain(
        track.replaygain_track.or(track.replaygain_album),
        track.replaygain_track_peak.or(track.replaygain_album_peak),
    )
}

/// Album gain when the queue is one album; falls back to track gain.
pub fn replaygain_album_multiplier(track: &Track) -> f64 {
    leveled_gain(
        track.replaygain_album.or(track.replaygain_track),
        track.replaygain_album_peak.or(track.replaygain_track_peak),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_extensions_are_detected() {
        assert!(is_audio_path(Path::new("/x/Song.FLAC")));
        assert!(is_audio_path(Path::new("/x/a.mp3")));
        assert!(!is_audio_path(Path::new("/x/notes.txt")));
    }

    #[test]
    fn replaygain_parses_db_suffix() {
        assert_eq!(parse_replaygain_db("-6.50 dB"), Some(-6.5));
        assert_eq!(parse_replaygain_db("3.0"), Some(3.0));
        assert_eq!(parse_replaygain_db("nope"), None);
    }

    #[test]
    fn replaygain_peak_pulls_a_hot_boost_back_to_full_scale() {
        let mut track = track_stub(Path::new("/a.flac"));
        track.replaygain_track = Some(6.0);
        track.replaygain_track_peak = Some(0.9);
        let unlimited = 10f64.powf(6.0 / 20.0);
        assert!(unlimited * 0.9 > 1.0);
        let gain = replaygain_multiplier(&track);
        assert!((gain - (1.0 / 0.9)).abs() < 1e-9);
        assert!(gain * 0.9 <= 1.0 + 1e-9);
    }

    #[test]
    fn album_multiplier_prefers_album_gain() {
        let mut track = track_stub(Path::new("/a.flac"));
        track.replaygain_track = Some(-6.0);
        track.replaygain_album = Some(0.0);
        let track_gain = replaygain_multiplier(&track);
        let album_gain = replaygain_album_multiplier(&track);
        assert!((album_gain - 1.0).abs() < 1e-9);
        assert!(track_gain < 0.6);
    }

    #[test]
    fn sort_uses_disc_then_track() {
        let mut tracks = vec![
            Track {
                path: "/a/02.mp3".into(),
                title: "B".into(),
                artist: String::new(),
                album: String::new(),
                album_artist: String::new(),
                track: Some(2),
                disc: Some(1),
                duration_ms: 0,
                folder: "/a".into(),
                replaygain_track: None,
                replaygain_album: None,
                replaygain_track_peak: None,
                replaygain_album_peak: None,
            },
            Track {
                path: "/a/01.mp3".into(),
                title: "A".into(),
                artist: String::new(),
                album: String::new(),
                album_artist: String::new(),
                track: Some(1),
                disc: Some(1),
                duration_ms: 0,
                folder: "/a".into(),
                replaygain_track: None,
                replaygain_album: None,
                replaygain_track_peak: None,
                replaygain_album_peak: None,
            },
        ];
        sort_tracks(&mut tracks);
        assert_eq!(tracks[0].track, Some(1));
        assert_eq!(tracks[1].track, Some(2));
    }

    #[test]
    fn collect_sees_files_added_and_removed_later() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("a.mp3"), []).unwrap();
        assert_eq!(collect_tracks(dir.path()).unwrap().len(), 1);
        std::fs::write(album.join("b.flac"), []).unwrap();
        assert_eq!(collect_tracks_fast(dir.path()).unwrap().len(), 2);
        std::fs::remove_file(album.join("a.mp3")).unwrap();
        let tracks = collect_tracks(dir.path()).unwrap();
        assert_eq!(tracks.len(), 1);
        assert!(tracks[0].path.ends_with("b.flac"));
    }

    #[test]
    fn collect_nested_folders() {
        let dir = tempfile::tempdir().unwrap();
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("01.mp3"), []).unwrap();
        std::fs::write(album.join("skip.txt"), []).unwrap();
        let tracks = collect_tracks(dir.path()).unwrap();
        assert_eq!(tracks.len(), 1);
        assert!(tracks[0].path.ends_with("01.mp3"));
        let fast = collect_tracks_fast(dir.path()).unwrap();
        assert_eq!(fast.len(), 1);
    }
}

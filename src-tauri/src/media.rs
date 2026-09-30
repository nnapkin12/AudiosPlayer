//! Linux desktop media controls through MPRIS.
//!
//! Souvlaki 0.8.3 is the crates.io release. `use_zbus` is that crate's published
//! feature and selects the zbus 3 dependency it ships with.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use base64::Engine;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};

use crate::player::Player;

pub fn start(player: Player) {
    let (tx, rx) = mpsc::sync_channel(1);
    player.bind_media(tx);
    std::thread::Builder::new()
        .name("audios-mpris".into())
        .spawn(move || {
            if let Err(error) = serve(player, rx) {
                eprintln!("Audios! media controls are unavailable: {error}");
            }
        })
        .ok();
}

fn serve(player: Player, rx: mpsc::Receiver<()>) -> Result<(), String> {
    let hwnd = None;
    let mut controls = MediaControls::new(PlatformConfig {
        dbus_name: "audios",
        display_name: "Audios!",
        hwnd,
    })
    .map_err(|error| format!("{error:?}"))?;

    let events = player.clone();
    let (event_tx, event_rx) = mpsc::channel::<MediaControlEvent>();
    std::thread::Builder::new()
        .name("audios-mpris-events".into())
        .spawn(move || {
            while let Ok(event) = event_rx.recv() {
                dispatch(event, &events);
            }
        })
        .map_err(|error| format!("{error:?}"))?;
    controls
        .attach(move |event| {
            let _ = event_tx.send(event);
        })
        .map_err(|error| format!("{error:?}"))?;

    let mut shown = Shown::default();
    publish(&mut controls, &player, &mut shown);
    loop {
        let wait = if shown.playing {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(30)
        };
        match rx.recv_timeout(wait) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {
                publish(&mut controls, &player, &mut shown);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn dispatch(event: MediaControlEvent, player: &Player) {
    match event {
        MediaControlEvent::Play => {
            let _ = player.play();
        }
        MediaControlEvent::Pause => {
            let _ = player.pause();
        }
        MediaControlEvent::Toggle => {
            let _ = player.toggle();
        }
        MediaControlEvent::Next => {
            let _ = player.next();
        }
        MediaControlEvent::Previous => {
            let _ = player.previous();
        }
        MediaControlEvent::Stop => {
            let _ = player.stop();
        }
        MediaControlEvent::Seek(direction) => {
            nudge(player, direction, Duration::from_secs(10));
        }
        MediaControlEvent::SeekBy(direction, delta) => nudge(player, direction, delta),
        MediaControlEvent::SetPosition(MediaPosition(position)) => {
            seek_to(player, position.as_millis() as u64);
        }
        MediaControlEvent::SetVolume(volume) => {
            let volume = volume.clamp(0.0, 1.0);
            if volume <= 0.0 {
                let _ = player.set_muted(true);
            } else {
                let _ = player.set_muted(false);
                let _ = player.set_volume(volume);
            }
        }
        MediaControlEvent::OpenUri(uri) => {
            if let Some(path) = file_path_from_uri(&uri) {
                let _ = player.play_path(&path);
            }
        }
        MediaControlEvent::Raise => player.raise_window(),
        MediaControlEvent::Quit => player.quit_window(),
    }
}

fn nudge(player: &Player, direction: SeekDirection, delta: Duration) {
    let snap = player.snapshot();
    let step = delta.as_millis() as u64;
    let next = match direction {
        SeekDirection::Forward => snap.position_ms.saturating_add(step),
        SeekDirection::Backward => snap.position_ms.saturating_sub(step),
    };
    seek_to(player, next);
}

fn seek_to(player: &Player, position_ms: u64) {
    let duration = player.snapshot().duration_ms;
    let position_ms = if duration > 0 {
        position_ms.min(duration.saturating_sub(1))
    } else {
        position_ms
    };
    let _ = player.seek(position_ms);
}

#[derive(Default)]
struct Shown {
    path: String,
    title: String,
    artist: String,
    album: String,
    duration_ms: u64,
    cover: Option<String>,
    playing: bool,
    position_ms: u64,
    volume: f64,
    ready: bool,
}

fn publish(controls: &mut MediaControls, player: &Player, shown: &mut Shown) {
    let snap = player.snapshot();
    let volume = if snap.muted { 0.0 } else { snap.volume };
    let Some(track) = snap.current.as_ref() else {
        if !shown.ready || !shown.path.is_empty() || shown.playing {
            let _ = controls.set_metadata(MediaMetadata::default());
            let _ = controls.set_playback(MediaPlayback::Stopped);
            shown.path.clear();
            shown.playing = false;
            shown.ready = true;
        }
        if (shown.volume - volume).abs() > 0.001 {
            let _ = controls.set_volume(volume);
            shown.volume = volume;
        }
        return;
    };

    let title = display_title(&track.title, &track.path);
    let artist = display_artist(&track.artist, &track.album_artist);
    let album = track.album.trim().to_string();
    let metadata_changed = shown.path != track.path
        || shown.title != title
        || shown.artist != artist
        || shown.album != album
        || shown.duration_ms != snap.duration_ms;

    if metadata_changed {
        let cover = if shown.path == track.path {
            shown.cover.clone()
        } else {
            cover_uri(&track.path)
        };
        let _ = controls.set_metadata(MediaMetadata {
            title: Some(title.as_str()),
            artist: Some(artist.as_str()),
            album: if album.is_empty() {
                None
            } else {
                Some(album.as_str())
            },
            cover_url: cover.as_deref(),
            duration: if snap.duration_ms > 0 {
                Some(Duration::from_millis(snap.duration_ms))
            } else {
                None
            },
        });
        shown.path = track.path.clone();
        shown.title = title;
        shown.artist = artist;
        shown.album = album;
        shown.duration_ms = snap.duration_ms;
        shown.cover = cover;
        shown.ready = true;
    }

    let moved = shown.position_ms.abs_diff(snap.position_ms) > 400;
    if shown.playing != snap.playing || moved || metadata_changed {
        let progress = Some(MediaPosition(Duration::from_millis(snap.position_ms)));
        let playback = if snap.playing {
            MediaPlayback::Playing { progress }
        } else {
            MediaPlayback::Paused { progress }
        };
        let _ = controls.set_playback(playback);
        shown.playing = snap.playing;
        shown.position_ms = snap.position_ms;
    }

    if (shown.volume - volume).abs() > 0.001 {
        let _ = controls.set_volume(volume);
        shown.volume = volume;
    }
}

fn display_title(title: &str, path: &str) -> String {
    if !title.trim().is_empty() {
        return title.to_string();
    }
    Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown title")
        .to_string()
}

fn display_artist(artist: &str, album_artist: &str) -> String {
    if !artist.trim().is_empty() {
        artist.to_string()
    } else if !album_artist.trim().is_empty() {
        album_artist.to_string()
    } else {
        "Unknown artist".to_string()
    }
}

fn cover_uri(track_path: &str) -> Option<String> {
    let art = crate::tags::cover_for(track_path).ok()??;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(art.data_base64.as_bytes())
        .ok()?;
    let path = write_cover(&bytes)?;
    Some(file_uri(&path))
}

fn write_cover(bytes: &[u8]) -> Option<PathBuf> {
    let dir = directories::ProjectDirs::from("com", "audios", "Audios")?
        .cache_dir()
        .join("mpris");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("cover-{:x}.jpg", hash_bytes(bytes)));
    if !path.is_file() {
        let tmp = dir.join(format!("cover-{:x}.jpg.part", hash_bytes(bytes)));
        std::fs::write(&tmp, bytes).ok()?;
        std::fs::rename(&tmp, &path).ok()?;
    }
    sweep_old_covers(&dir, &path);
    Some(path)
}

/// Keep the current file (and anything written in the last 30 s) so the
/// shell can still load a just-replaced cover.
fn sweep_old_covers(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let candidate = entry.path();
        if candidate == keep {
            continue;
        }
        let name = candidate
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !name.starts_with("cover-") {
            continue;
        }
        if !(name.ends_with(".jpg") || name.ends_with(".part")) {
            continue;
        }
        let recent = candidate
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age < Duration::from_secs(30));
        if recent {
            continue;
        }
        let _ = std::fs::remove_file(candidate);
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn file_path_from_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let path = rest.split(['?', '#']).next().unwrap_or(rest);
    percent_decode(path)
}

fn percent_decode(input: &str) -> Option<String> {
    let raw = input.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%' && index + 2 < raw.len() {
            let hex = std::str::from_utf8(&raw[index + 1..index + 3]).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::{file_path_from_uri, file_uri};
    use std::path::Path;

    #[test]
    fn file_uri_round_trip_keeps_spaces() {
        let path = Path::new("/home/listener/My Music/01 - Song.flac");
        let uri = file_uri(path);
        assert!(uri.starts_with("file:///home/listener/My%20Music/"));
        assert_eq!(
            file_path_from_uri(&uri).as_deref(),
            Some(path.to_str().unwrap())
        );
    }
}

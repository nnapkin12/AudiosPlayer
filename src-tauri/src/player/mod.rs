mod convert;
pub mod engine;
pub mod queue;
pub mod scan;

use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::eq::{self, EqPersist, EqUpdate, EqUserPreset};
use crate::error::{AppError, AppResult};
use crate::persist::Store;

use self::engine::{PlayerEngine, RodioEngine};
use self::queue::{Queue, RepeatMode};
use self::scan::{collect_tracks, replaygain_multiplier, track_from_path, FolderNode, Track};

pub const STATE_EVENT: &str = "player://state";
pub const TICK_EVENT: &str = "player://tick";
pub const SPEED_MIN: f64 = 0.5;
pub const SPEED_MAX: f64 = 2.0;

pub fn clamp_speed(value: f64) -> f64 {
    if !value.is_finite() {
        return 1.0;
    }
    let hundredths = (value * 100.0).round() / 100.0;
    hundredths.clamp(SPEED_MIN, SPEED_MAX)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub current: Option<Track>,
    pub index: usize,
    pub queue: Vec<Track>,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f64,
    pub muted: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub replaygain: bool,
    pub gapless: bool,
    pub speed: f64,
    /// Sample rate of the playing file. The equalizer curve uses this, not a fixed 48 kHz.
    pub sample_rate: u32,
    pub eq: eq::EqState,
    pub root: Option<String>,
    pub tree: Option<FolderNode>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tick {
    pub position_ms: u64,
    pub duration_ms: u64,
}

/// Now-playing fields for a remote. Not sent to the desktop webview.
#[derive(Debug, Clone)]
pub struct Transport {
    pub title: String,
    pub artist: String,
    pub album_artist: String,
    pub album: String,
    pub path: String,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f64,
    pub muted: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub speed: f64,
}

struct Logic {
    queue: Queue,
    volume: f64,
    muted: bool,
    replaygain: bool,
    gapless: bool,
    speed: f64,
    root: Option<PathBuf>,
    error: Option<String>,
    pending_gapless: bool,
    want_playing: bool,
    eq: EqPersist,
}

#[derive(Clone)]
pub struct Player {
    engine: Arc<RodioEngine>,
    logic: Arc<Mutex<Logic>>,
    persist: Store,
    app: AppHandle,
    media_ping: Arc<Mutex<Option<SyncSender<()>>>>,
}

impl Player {
    pub fn new(app: AppHandle, persist: Store) -> Self {
        let saved = persist.snapshot();
        let speed = clamp_speed(saved.speed);
        let player = Self {
            engine: Arc::new(RodioEngine::new()),
            logic: Arc::new(Mutex::new(Logic {
                queue: Queue::with_modes(saved.repeat, saved.shuffle),
                volume: saved.volume,
                muted: saved.muted,
                replaygain: saved.replaygain,
                gapless: saved.gapless,
                speed,
                root: None,
                error: None,
                pending_gapless: false,
                want_playing: false,
                eq: saved.eq.clone(),
            })),
            persist,
            app,
            media_ping: Arc::new(Mutex::new(None)),
        };
        crate::search::clear_temps();
        let _ = player.engine.set_eq(eq::EqParams::from_persist(&saved.eq));
        let _ = player.engine.set_speed(speed);

        let weak = player.clone();
        std::thread::Builder::new()
            .name("audios-ticker".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(250));
                weak.tick();
            })
            .expect("player ticker");

        player
    }

    pub fn bind_media(&self, tx: SyncSender<()>) {
        *self.media_ping.lock().expect("media ping") = Some(tx);
    }

    pub fn raise_window(&self) {
        let Some(window) = self.app.get_webview_window("main") else {
            return;
        };
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }

    pub fn quit_window(&self) {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.close();
        }
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot_eq(false)
    }

    pub fn snapshot_ui(&self) -> PlayerSnapshot {
        self.snapshot_eq(true)
    }

    fn snapshot_eq(&self, eq_catalog: bool) -> PlayerSnapshot {
        let logic = self.logic.lock().expect("player lock");
        PlayerSnapshot {
            current: logic.queue.current().cloned(),
            index: logic.queue.index,
            // Webview only needs the current track. Skip the queue, folder tree,
            // and EQ catalog on volume/seek ticks so IPC stays small.
            queue: Vec::new(),
            playing: self.engine.is_playing(),
            position_ms: self.engine.position_ms(),
            duration_ms: self.current_duration(&logic),
            volume: logic.volume,
            muted: logic.muted,
            repeat: logic.queue.repeat,
            shuffle: logic.queue.shuffle,
            replaygain: logic.replaygain,
            gapless: logic.gapless,
            speed: logic.speed,
            sample_rate: self.engine.sample_rate(),
            eq: if eq_catalog {
                eq::state_with_catalog(&logic.eq)
            } else {
                eq::state_from(&logic.eq)
            },
            root: logic
                .root
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            tree: None,
            error: logic.error.clone(),
        }
    }

    pub fn open_path(&self, path: &str) -> AppResult<PlayerSnapshot> {
        self.open_path_inner(path, true)?;
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn play(&self) -> AppResult<PlayerSnapshot> {
        self.engine.play()?;
        self.logic.lock().expect("player lock").want_playing = true;
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn pause(&self) -> AppResult<PlayerSnapshot> {
        self.engine.pause()?;
        self.logic.lock().expect("player lock").want_playing = false;
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn toggle(&self) -> AppResult<PlayerSnapshot> {
        if self.engine.is_playing() {
            self.pause()
        } else {
            if self
                .logic
                .lock()
                .expect("player lock")
                .queue
                .current()
                .is_none()
            {
                return Err(AppError::msg("Nothing playing"));
            }
            self.play()
        }
    }

    pub fn stop(&self) -> AppResult<PlayerSnapshot> {
        self.forget_current();
        self.engine.stop()?;
        self.logic.lock().expect("player lock").want_playing = false;
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn next(&self) -> AppResult<PlayerSnapshot> {
        self.forget_current();
        let next_path = {
            let mut logic = self.logic.lock().expect("player lock");
            logic.pending_gapless = false;
            logic.queue.advance().map(|track| track.path.clone())
        };
        match next_path {
            Some(path) => self.load_and_maybe_play(&path, true)?,
            None => {
                self.engine.pause()?;
                let _ = self.engine.seek(0);
                self.logic.lock().expect("player lock").want_playing = false;
            }
        }
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn previous(&self) -> AppResult<PlayerSnapshot> {
        if self.engine.position_ms() > 3000 {
            self.engine.seek(0)?;
            self.emit_state();
            return Ok(self.snapshot());
        }
        self.forget_current();
        let previous = {
            let mut logic = self.logic.lock().expect("player lock");
            logic.pending_gapless = false;
            logic.queue.retreat().map(|track| track.path.clone())
        };
        if let Some(path) = previous {
            self.load_and_maybe_play(&path, true)?;
        }
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn seek(&self, position_ms: u64) -> AppResult<PlayerSnapshot> {
        self.engine.seek(position_ms)?;
        self.emit_tick();
        self.ping_media();
        Ok(self.snapshot())
    }

    pub fn play_index(&self, index: usize) -> AppResult<PlayerSnapshot> {
        self.forget_current();
        let path = {
            let mut logic = self.logic.lock().expect("player lock");
            logic.pending_gapless = false;
            logic
                .queue
                .play_index(index)
                .map(|track| track.path.clone())
                .ok_or_else(|| AppError::msg("that track is not in the queue"))?
        };
        self.load_and_maybe_play(&path, true)?;
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn play_path(&self, path: &str) -> AppResult<PlayerSnapshot> {
        self.forget_current();
        let index = {
            let logic = self.logic.lock().expect("player lock");
            logic
                .queue
                .tracks
                .iter()
                .position(|track| track.path == path)
        };
        if let Some(index) = index {
            return self.play_index(index);
        }
        if Path::new(path).is_dir() {
            let jumped = {
                let mut logic = self.logic.lock().expect("player lock");
                logic
                    .queue
                    .jump_to_folder(path)
                    .map(|track| track.path.clone())
            };
            if let Some(track_path) = jumped {
                self.load_and_maybe_play(&track_path, true)?;
                self.emit_state();
                return Ok(self.snapshot());
            }
        }
        self.open_path(path)
    }

    /// Play a library file. If it is already queued, keep that queue.
    /// Otherwise queue the other songs in the same folder, capped so a
    /// huge directory does not become the whole queue.
    pub fn play_folder_file(&self, path: &str) -> AppResult<PlayerSnapshot> {
        if !Path::new(path).is_file() {
            return Err(AppError::msg("That song is missing"));
        }
        let queued = {
            let logic = self.logic.lock().expect("player lock");
            logic.queue.tracks.iter().any(|track| track.path == path)
        };
        if queued {
            return self.play_path(path);
        }
        let tracks = folder_tracks(Path::new(path));
        let tracks = if tracks.iter().any(|track| track.path == path) {
            tracks
        } else {
            vec![track_from_path(Path::new(path))]
        };
        self.play_tracks(tracks, Some(path.to_string()))
    }

    /// Fields the web remote renders. Skips the queue, folder tree, and EQ.
    pub fn transport(&self) -> Transport {
        let logic = self.logic.lock().expect("player lock");
        let current = logic.queue.current();
        Transport {
            title: current.map(|track| track.title.clone()).unwrap_or_default(),
            artist: current
                .map(|track| track.artist.clone())
                .unwrap_or_default(),
            album_artist: current
                .map(|track| track.album_artist.clone())
                .unwrap_or_default(),
            album: current.map(|track| track.album.clone()).unwrap_or_default(),
            path: current.map(|track| track.path.clone()).unwrap_or_default(),
            playing: self.engine.is_playing(),
            position_ms: self.engine.position_ms(),
            duration_ms: self.current_duration(&logic),
            volume: logic.volume,
            muted: logic.muted,
            repeat: logic.queue.repeat,
            shuffle: logic.queue.shuffle,
            speed: logic.speed,
        }
    }

    pub fn play_queue_paths(
        &self,
        paths: Vec<String>,
        start_path: Option<String>,
    ) -> AppResult<PlayerSnapshot> {
        self.forget_current();
        let mut tracks = Vec::new();
        for path in &paths {
            if let Ok(found) = collect_tracks(Path::new(path)) {
                tracks.extend(found);
            }
        }
        if tracks.is_empty() {
            return Err(AppError::msg("This playlist is empty"));
        }
        let start = start_path
            .as_deref()
            .and_then(|wanted| tracks.iter().position(|track| track.path == wanted))
            .unwrap_or(0);
        let first = {
            let mut logic = self.logic.lock().expect("player lock");
            logic.error = None;
            logic.pending_gapless = false;
            logic.queue.replace(tracks, start);
            logic.queue.current().map(|track| track.path.clone())
        };
        if let Some(first) = first {
            self.load_and_maybe_play(&first, true)?;
        }
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn play_tracks(
        &self,
        tracks: Vec<Track>,
        start_path: Option<String>,
    ) -> AppResult<PlayerSnapshot> {
        self.forget_current();
        if tracks.is_empty() {
            return Err(AppError::msg("Nothing to play"));
        }
        let start = start_path
            .as_deref()
            .and_then(|wanted| tracks.iter().position(|track| track.path == wanted))
            .unwrap_or(0);
        let first = {
            let mut logic = self.logic.lock().expect("player lock");
            logic.error = None;
            logic.pending_gapless = false;
            logic.queue.replace(tracks, start);
            logic.queue.current().map(|track| track.path.clone())
        };
        if let Some(first) = first {
            self.load_and_maybe_play(&first, true)?;
        }
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn set_volume(&self, volume: f64) -> AppResult<PlayerSnapshot> {
        {
            let mut logic = self.logic.lock().expect("player lock");
            logic.volume = volume.clamp(0.0, 1.0);
        }
        self.apply_volume();
        self.persist.update(|data| {
            data.volume = volume.clamp(0.0, 1.0);
        });
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn set_muted(&self, muted: bool) -> AppResult<PlayerSnapshot> {
        self.logic.lock().expect("player lock").muted = muted;
        self.apply_volume();
        self.persist.update(|data| data.muted = muted);
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn set_repeat(&self, repeat: RepeatMode) -> AppResult<PlayerSnapshot> {
        self.logic
            .lock()
            .expect("player lock")
            .queue
            .set_repeat(repeat);
        self.persist.update(|data| data.repeat = repeat);
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn set_shuffle(&self, shuffle: bool) -> AppResult<PlayerSnapshot> {
        self.logic
            .lock()
            .expect("player lock")
            .queue
            .set_shuffle(shuffle);
        self.persist.update(|data| data.shuffle = shuffle);
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn set_replaygain(&self, enabled: bool) -> AppResult<PlayerSnapshot> {
        self.logic.lock().expect("player lock").replaygain = enabled;
        self.persist.update(|data| data.replaygain = enabled);
        self.apply_volume();
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn set_gapless(&self, enabled: bool) -> AppResult<PlayerSnapshot> {
        self.logic.lock().expect("player lock").gapless = enabled;
        self.persist.update(|data| data.gapless = enabled);
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn set_speed(&self, speed: f64) -> AppResult<PlayerSnapshot> {
        let speed = clamp_speed(speed);
        self.engine.set_speed(speed)?;
        self.logic.lock().expect("player lock").speed = speed;
        self.persist.update(|data| data.speed = speed);
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn refresh_metadata(&self, paths: &[String]) -> Vec<Track> {
        let mut fresh = Vec::new();
        for path in paths {
            if !Path::new(path).is_file() {
                continue;
            }
            let track = track_from_path(Path::new(path));
            crate::tags::forget_thumb(path);
            fresh.push(track);
        }
        if fresh.is_empty() {
            return fresh;
        }
        {
            let mut logic = self.logic.lock().expect("player lock");
            for track in &mut logic.queue.tracks {
                if let Some(next) = fresh.iter().find(|item| item.path == track.path) {
                    *track = next.clone();
                }
            }
        }
        self.emit_state();
        fresh
    }

    pub fn set_eq(&self, update: EqUpdate) -> AppResult<PlayerSnapshot> {
        let params = eq::EqParams::from_update(&update);
        {
            let mut logic = self.logic.lock().expect("player lock");
            eq::apply_update(&mut logic.eq, &update);
        }
        let _ = self.engine.set_eq(params);
        self.persist
            .update(|data| eq::apply_update(&mut data.eq, &update));
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn import_parametric_eq(&self, text: String) -> AppResult<PlayerSnapshot> {
        let parsed = eq::parse_parametric_eq(&text)?;
        self.set_eq(EqUpdate {
            enabled: true,
            preset_id: eq::WORKING_PRESET_ID.into(),
            bands: parsed.bands.to_vec(),
            preamp: parsed.preamp,
            auto_preamp: true,
        })
    }

    pub fn save_custom_eq(&self, preset: EqUserPreset) -> AppResult<PlayerSnapshot> {
        let (eq, params) = {
            let mut logic = self.logic.lock().expect("player lock");
            eq::upsert_user_preset(&mut logic.eq, preset)?;
            (logic.eq.clone(), eq::EqParams::from_persist(&logic.eq))
        };
        self.persist.update(|data| data.eq = eq);
        let _ = self.engine.set_eq(params);
        self.emit_state();
        Ok(self.snapshot())
    }

    pub fn delete_custom_eq(&self, id: String) -> AppResult<PlayerSnapshot> {
        let (eq, params) = {
            let mut logic = self.logic.lock().expect("player lock");
            eq::delete_user_preset(&mut logic.eq, &id)?;
            (logic.eq.clone(), eq::EqParams::from_persist(&logic.eq))
        };
        self.persist.update(|data| data.eq = eq);
        let _ = self.engine.set_eq(params);
        self.emit_state();
        Ok(self.snapshot())
    }

    fn open_path_inner(&self, path: &str, play: bool) -> AppResult<()> {
        let path = PathBuf::from(path);
        if !path.exists() {
            return Err(AppError::msg(format!("missing path: {}", path.display())));
        }
        let tracks = collect_tracks(&path)?;
        if tracks.is_empty() {
            return Err(AppError::msg("No songs here"));
        }
        let root = if path.is_file() {
            path.parent().map(Path::to_path_buf).unwrap_or(path.clone())
        } else {
            path.clone()
        };
        let start = if path.is_file() {
            tracks
                .iter()
                .position(|track| Path::new(&track.path) == path)
                .unwrap_or(0)
        } else {
            0
        };
        let first = {
            let mut logic = self.logic.lock().expect("player lock");
            logic.root = Some(root.clone());
            logic.error = None;
            logic.pending_gapless = false;
            logic.queue.replace(tracks, start);
            logic.queue.current().map(|track| track.path.clone())
        };
        self.persist.update(|data| {
            data.last_root = Some(root.to_string_lossy().to_string());
        });
        if let Some(first) = first {
            self.load_and_maybe_play(&first, play)?;
        }
        Ok(())
    }

    fn load_and_maybe_play(&self, path: &str, play: bool) -> AppResult<()> {
        // The queue keeps the library path. The engine gets a file Symphonia can open.
        let playable = convert::for_playback(Path::new(path))?;
        self.engine.set_uri(&playable)?;
        crate::search::drop_temps_except(Some(Path::new(path)));
        self.apply_volume();
        {
            let mut logic = self.logic.lock().expect("player lock");
            logic.pending_gapless = false;
            logic.want_playing = play;
            logic.error = None;
        }
        if play {
            self.engine.play()?;
        } else {
            self.engine.pause()?;
        }
        Ok(())
    }

    fn apply_volume(&self) {
        let logic = self.logic.lock().expect("player lock");
        let mut volume = if logic.muted { 0.0 } else { logic.volume };
        if logic.replaygain {
            if let Some(track) = logic.queue.current() {
                volume *= replaygain_multiplier(track);
            }
        }
        drop(logic);
        let _ = self.engine.set_volume(volume);
    }

    fn current_duration(&self, logic: &Logic) -> u64 {
        let decoded = self.engine.duration_ms();
        let tagged = logic
            .queue
            .current()
            .map(|track| track.duration_ms)
            .unwrap_or(0);
        let header = decoded.max(tagged);
        let position = self.engine.position_ms();
        // A short header (common on some MP3s) would pin the bar at the end
        // while samples are still playing. Once playback passes that header,
        // grow the length by the overrun so the bar can move again.
        if self.engine.is_playing() && header > 0 && position > header {
            let overrun = position - header;
            header + overrun + overrun
        } else {
            header
        }
    }

    fn forget_current(&self) {
        let path = self
            .logic
            .lock()
            .expect("player lock")
            .queue
            .current()
            .map(|track| track.path.clone());
        if let Some(path) = path {
            self.persist.forget_position(&path);
        }
    }

    fn maybe_queue_gapless(&self) {
        let (gapless, pending, next) = {
            let logic = self.logic.lock().expect("player lock");
            (
                logic.gapless,
                logic.pending_gapless,
                logic.queue.peek_next_index().and_then(|index| {
                    logic
                        .queue
                        .tracks
                        .get(index)
                        .map(|track| track.path.clone())
                }),
            )
        };
        if !gapless || pending {
            return;
        }
        if let Some(path) = next {
            let Ok(playable) = convert::for_playback(Path::new(&path)) else {
                return;
            };
            if self.engine.set_gapless_next(Some(&playable)).is_ok() {
                self.logic.lock().expect("player lock").pending_gapless = true;
            }
        }
    }

    fn on_gapless_started(&self) {
        let changed = {
            let mut logic = self.logic.lock().expect("player lock");
            if !logic.pending_gapless {
                return;
            }
            logic.pending_gapless = false;
            logic.queue.advance();
            true
        };
        if changed {
            self.apply_volume();
            self.emit_state();
        }
    }

    fn on_eos(&self) {
        self.forget_current();
        let next = {
            let mut logic = self.logic.lock().expect("player lock");
            if logic.pending_gapless {
                logic.pending_gapless = false;
                logic.queue.advance().map(|track| track.path.clone())
            } else {
                logic.queue.advance().map(|track| track.path.clone())
            }
        };
        if let Some(path) = next {
            if let Err(error) = self.load_and_maybe_play(&path, true) {
                self.logic.lock().expect("player lock").error = Some(error.to_string());
            }
        } else {
            let _ = self.engine.pause();
            let _ = self.engine.seek(0);
            self.logic.lock().expect("player lock").want_playing = false;
        }
        self.emit_state();
    }

    fn tick(&self) {
        let (want_playing, duration, pending) = {
            let logic = self.logic.lock().expect("player lock");
            (
                logic.want_playing,
                self.current_duration(&logic),
                logic.pending_gapless,
            )
        };
        let position = self.engine.position_ms();
        let queued = self.engine.queued_sources();

        if want_playing && self.engine.is_empty() {
            self.on_eos();
            return;
        }

        if pending && queued <= 1 && position < 800 {
            self.on_gapless_started();
        } else if want_playing && duration > 0 && position + 1600 >= duration {
            self.maybe_queue_gapless();
        }

        self.emit_tick();
    }

    fn emit_state(&self) {
        let _ = self.app.emit(STATE_EVENT, self.snapshot());
        self.ping_media();
    }

    fn ping_media(&self) {
        let guard = self.media_ping.lock().expect("media ping");
        if let Some(tx) = guard.as_ref() {
            let _ = tx.try_send(());
        }
    }

    fn emit_tick(&self) {
        let logic = self.logic.lock().expect("player lock");
        let _ = self.app.emit(
            TICK_EVENT,
            Tick {
                position_ms: self.engine.position_ms(),
                duration_ms: self.current_duration(&logic),
            },
        );
    }
}

/// Songs sitting next to `file`. Above the cap, return just that file so
/// one enormous folder does not all land in the queue.
fn folder_tracks(file: &Path) -> Vec<Track> {
    const CAP: usize = 2_000;
    let Some(parent) = file.parent() else {
        return vec![track_from_path(file)];
    };
    let Ok(dir) = std::fs::read_dir(parent) else {
        return vec![track_from_path(file)];
    };
    let mut paths = Vec::new();
    for entry in dir.flatten() {
        let candidate = entry.path();
        if candidate.is_file() && scan::is_audio_path(&candidate) {
            paths.push(candidate);
            if paths.len() > CAP {
                return vec![track_from_path(file)];
            }
        }
    }
    if paths.is_empty() {
        return vec![track_from_path(file)];
    }
    let mut tracks: Vec<Track> = paths.iter().map(|path| track_from_path(path)).collect();
    scan::sort_tracks(&mut tracks);
    tracks
}

#[cfg(test)]
mod tests {
    use super::clamp_speed;

    #[test]
    fn speed_stays_inside_one_octave() {
        assert_eq!(clamp_speed(1.0), 1.0);
        assert_eq!(clamp_speed(0.1), 0.5);
        assert_eq!(clamp_speed(3.0), 2.0);
        assert_eq!(clamp_speed(f64::NAN), 1.0);
        assert_eq!(clamp_speed(1.256), 1.26);
    }

    #[test]
    fn folder_tracks_skip_non_audio() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.mp3"), []).unwrap();
        std::fs::write(dir.path().join("b.flac"), []).unwrap();
        std::fs::write(dir.path().join("notes.txt"), []).unwrap();
        let tracks = super::folder_tracks(&dir.path().join("b.flac"));
        assert_eq!(tracks.len(), 2);
        assert!(tracks.iter().all(|track| !track.path.ends_with(".txt")));
    }
}

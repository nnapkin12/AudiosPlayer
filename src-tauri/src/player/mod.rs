mod convert;
pub mod engine;
#[cfg(test)]
pub mod fake_engine;
pub mod host;
pub mod queue;
pub mod scan;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::eq::{self, EqPersist, EqUpdate, EqUserPreset};
use crate::error::{AppError, AppResult};
use crate::persist::Store;

use self::engine::{PlayerEngine, RodioEngine};
use self::host::{Host, TauriHost};
use self::queue::{Queue, RepeatMode};
use self::scan::{collect_tracks, replaygain_multiplier, track_from_path, FolderNode, Track};

pub const STATE_EVENT: &str = "player://state";
pub const TICK_EVENT: &str = "player://tick";
pub const VIZ_EVENT: &str = "player://viz";
pub const SPEED_MIN: f64 = 0.5;
pub const SPEED_MAX: f64 = 2.0;
/// How long before the end of a song the next one is handed to the engine.
pub const GAPLESS_PREROLL_MS: u64 = 1600;
/// Ticker period. End of track and gapless are checked at this rate.
pub const TICK_MS: u64 = 250;

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
    /// A next song has been (or is being) handed to the engine.
    pending_gapless: bool,
    /// Which song that was, to notice when the queue changes its mind.
    pending_path: Option<String>,
    /// `engine.started_sources()` last seen; a higher value is a hop.
    started_seen: u64,
    want_playing: bool,
    eq: EqPersist,
}

#[derive(Clone)]
pub struct Player {
    engine: Arc<dyn PlayerEngine>,
    logic: Arc<Mutex<Logic>>,
    /// Serializes public operations. See [`Player::begin`].
    ops: Arc<Mutex<()>>,
    persist: Store,
    host: Arc<dyn Host>,
    media_ping: Arc<Mutex<Option<SyncSender<()>>>>,
    /// Run gapless preparation on a thread. Off in tests for determinism.
    background_work: bool,
    /// Incremented on every `play_media`. A download that finishes after a
    /// newer one started is dropped instead of replacing it.
    play_generation: Arc<std::sync::atomic::AtomicU64>,
}

impl Player {
    /// The production player: rodio output, Tauri events, background ticker
    /// and visualizer threads.
    pub fn new(app: AppHandle, persist: Store) -> Self {
        crate::search::clear_temps();
        convert::prune_playback_cache(convert::PLAYBACK_CACHE_CAP_BYTES, None);
        let mut player = Self::with_parts(
            Arc::new(RodioEngine::new()),
            Arc::new(TauriHost::new(app)),
            persist,
        );
        player.background_work = true;
        player.spawn_background();
        player
    }

    /// Assemble a player from parts and do not start any threads. Tests call
    /// [`Player::tick`] themselves.
    pub fn with_parts(engine: Arc<dyn PlayerEngine>, host: Arc<dyn Host>, persist: Store) -> Self {
        let saved = persist.snapshot();
        let speed = clamp_speed(saved.speed);
        let player = Self {
            engine,
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
                pending_path: None,
                started_seen: 0,
                want_playing: false,
                eq: saved.eq.clone(),
            })),
            ops: Arc::new(Mutex::new(())),
            persist,
            host,
            media_ping: Arc::new(Mutex::new(None)),
            background_work: false,
            play_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let _ = player.engine.set_eq(eq::EqParams::from_persist(&saved.eq));
        let _ = player.engine.set_speed(speed);
        player.engine.set_viz_enabled(saved.visualizer);
        player
    }

    fn spawn_background(&self) {
        let ticker = self.clone();
        std::thread::Builder::new()
            .name("audios-ticker".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(TICK_MS));
                // A panic in one tick must not end end-of-track handling
                // for the rest of the session.
                if catch_unwind(AssertUnwindSafe(|| ticker.tick())).is_err() {
                    eprintln!("Audios! player tick panicked; continuing");
                }
            })
            .expect("player ticker");

        let viz_engine = Arc::clone(&self.engine);
        let viz_host = Arc::clone(&self.host);
        std::thread::Builder::new()
            .name("audios-viz".into())
            .spawn(move || {
                let mut last = [0u8; crate::viz::BANDS];
                loop {
                    std::thread::sleep(Duration::from_millis(33));
                    let step = catch_unwind(AssertUnwindSafe(|| {
                        if !viz_engine.viz_enabled() || !viz_engine.is_playing() {
                            last = [0; crate::viz::BANDS];
                            return;
                        }
                        let Some(bands) = viz_engine.spectrum() else {
                            return;
                        };
                        if bands == last {
                            return;
                        }
                        last = bands;
                        viz_host.emit_viz(bands);
                    }));
                    if step.is_err() {
                        eprintln!("Audios! visualizer panicked; continuing");
                    }
                }
            })
            .expect("visualizer thread");
    }

    pub fn bind_media(&self, tx: SyncSender<()>) {
        *self.media_ping.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    }

    pub fn raise_window(&self) {
        self.host.raise_window();
    }

    pub fn quit_window(&self) {
        self.host.quit_window();
    }

    /// Call at the start of a search-play. Returns the generation to check
    /// after the download; if it no longer matches, another play won.
    pub fn bump_play_generation(&self) -> u64 {
        self.play_generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1
    }

    pub fn play_generation(&self) -> u64 {
        self.play_generation
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot_eq(false)
    }

    pub fn snapshot_ui(&self) -> PlayerSnapshot {
        self.snapshot_eq(true)
    }

    fn snapshot_eq(&self, eq_catalog: bool) -> PlayerSnapshot {
        let logic = self.logic();
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
            // A playback error wins; otherwise tell the user their settings
            // file could not be read or written.
            error: logic
                .error
                .clone()
                .or_else(|| self.persist.warning())
                .or_else(|| self.persist.session_note()),
        }
    }

    // ---- locking -------------------------------------------------------

    /// Every public operation runs under this lock, so the 250 ms ticker,
    /// MPRIS, the remote, and the webview cannot interleave half-finished
    /// state changes. Reads (`snapshot`, `transport`) do not take it.
    fn begin(&self) -> MutexGuard<'_, ()> {
        let guard = self.ops.lock().unwrap_or_else(|e| e.into_inner());
        // A gapless hop the ticker has not noticed yet must land before the
        // caller reasons about "current".
        self.sync_hop();
        guard
    }

    fn logic(&self) -> MutexGuard<'_, Logic> {
        self.logic.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn finish(&self) -> AppResult<PlayerSnapshot> {
        self.emit_state();
        Ok(self.snapshot())
    }

    // ---- transport -----------------------------------------------------

    pub fn open_path(&self, path: &str) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.open_path_inner(path, true)?;
        self.finish()
    }

    pub fn play(&self) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.play_inner()?;
        self.finish()
    }

    pub fn pause(&self) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.pause_inner()?;
        self.finish()
    }

    pub fn toggle(&self) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        if self.engine.is_playing() {
            self.pause_inner()?;
        } else {
            self.play_inner()?;
        }
        self.finish()
    }

    pub fn stop(&self) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.engine.stop()?;
        {
            let mut logic = self.logic();
            logic.want_playing = false;
            logic.clear_pending();
        }
        self.finish()
    }

    pub fn next(&self) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        let next_path = {
            let mut logic = self.logic();
            logic.clear_pending();
            logic.queue.advance().map(|track| track.path.clone())
        };
        match next_path {
            Some(path) => self.load_and_maybe_play(&path, true)?,
            None => {
                self.engine.pause()?;
                self.logic().want_playing = false;
            }
        }
        self.finish()
    }

    pub fn previous(&self) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        // Past three seconds in, Previous restarts the song. An empty sink
        // (after the queue ended) still reports its last position; skip that.
        if !self.engine.is_empty() && self.engine.position_ms() > 3000 {
            self.engine.seek(0)?;
            return self.finish();
        }
        let previous = {
            let mut logic = self.logic();
            logic.clear_pending();
            logic.queue.retreat().map(|track| track.path.clone())
        };
        if let Some(path) = previous {
            self.load_and_maybe_play(&path, true)?;
        }
        self.finish()
    }

    pub fn seek(&self, position_ms: u64) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.engine.seek(position_ms)?;
        self.emit_tick();
        self.ping_media();
        Ok(self.snapshot())
    }

    pub fn play_index(&self, index: usize) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.play_index_inner(index)?;
        self.finish()
    }

    pub fn play_path(&self, path: &str) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.play_path_inner(path)?;
        self.finish()
    }

    /// Play a library file. If it is already queued, keep that queue.
    /// Otherwise queue the other songs in the same folder, capped so a
    /// huge directory does not become the whole queue.
    pub fn play_folder_file(&self, path: &str) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        if !Path::new(path).is_file() {
            return Err(AppError::msg("That song is missing"));
        }
        let queued = self
            .logic()
            .queue
            .tracks
            .iter()
            .any(|track| track.path == path);
        if queued {
            self.play_path_inner(path)?;
            return self.finish();
        }
        let tracks = folder_tracks(Path::new(path));
        let tracks = if tracks.iter().any(|track| track.path == path) {
            tracks
        } else {
            vec![track_from_path(Path::new(path))]
        };
        self.play_tracks_inner(tracks, Some(path.to_string()))?;
        self.finish()
    }

    /// Fields the web remote renders. Skips the queue, folder tree, and EQ.
    pub fn transport(&self) -> Transport {
        let logic = self.logic();
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
        let _op = self.begin();
        let mut tracks = Vec::new();
        for path in &paths {
            if let Ok(found) = collect_tracks(Path::new(path)) {
                tracks.extend(found);
            }
        }
        if tracks.is_empty() {
            return Err(AppError::msg("This playlist is empty"));
        }
        self.play_tracks_inner(tracks, start_path)?;
        self.finish()
    }

    pub fn play_tracks(
        &self,
        tracks: Vec<Track>,
        start_path: Option<String>,
    ) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.play_tracks_inner(tracks, start_path)?;
        self.finish()
    }

    /// Append a song. An empty queue starts it. A playing or paused queue
    /// keeps its place and play state.
    pub fn enqueue_path(&self, path: &str) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        if !Path::new(path).is_file() {
            return Err(AppError::msg("That song is missing"));
        }
        if self.logic().queue.is_idle() {
            self.play_tracks_inner(
                vec![track_from_path(Path::new(path))],
                Some(path.to_string()),
            )?;
            return self.finish();
        }
        self.logic().queue.enqueue(track_from_path(Path::new(path)));
        self.invalidate_gapless();
        self.finish()
    }

    // ---- settings ------------------------------------------------------

    pub fn set_volume(&self, volume: f64) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        let volume = volume.clamp(0.0, 1.0);
        self.logic().volume = volume;
        self.apply_volume();
        // Sliders fire many times a second; the ticker writes the file.
        self.persist.update_soon(|data| data.volume = volume);
        self.finish()
    }

    pub fn set_muted(&self, muted: bool) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.logic().muted = muted;
        self.apply_volume();
        self.persist.update_soon(|data| data.muted = muted);
        self.finish()
    }

    pub fn set_repeat(&self, repeat: RepeatMode) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.logic().queue.set_repeat(repeat);
        self.persist.update(|data| data.repeat = repeat);
        self.invalidate_gapless();
        self.finish()
    }

    pub fn set_shuffle(&self, shuffle: bool) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.logic().queue.set_shuffle(shuffle);
        self.persist.update(|data| data.shuffle = shuffle);
        self.invalidate_gapless();
        self.finish()
    }

    pub fn set_replaygain(&self, enabled: bool) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.logic().replaygain = enabled;
        self.persist.update(|data| data.replaygain = enabled);
        self.apply_volume();
        self.finish()
    }

    pub fn set_gapless(&self, enabled: bool) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.logic().gapless = enabled;
        self.persist.update(|data| data.gapless = enabled);
        if !enabled {
            self.cancel_pending_gapless();
        }
        self.finish()
    }

    pub fn set_viz_enabled(&self, enabled: bool) {
        self.engine.set_viz_enabled(enabled);
    }

    pub fn set_speed(&self, speed: f64) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        let speed = clamp_speed(speed);
        self.engine.set_speed(speed)?;
        self.logic().speed = speed;
        self.persist.update(|data| data.speed = speed);
        self.finish()
    }

    pub fn refresh_metadata(&self, paths: &[String]) -> Vec<Track> {
        let _op = self.begin();
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
        self.logic().queue.refresh_tracks(&fresh);
        self.emit_state();
        fresh
    }

    pub fn set_eq(&self, update: EqUpdate) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        self.set_eq_inner(update);
        self.finish()
    }

    pub fn import_parametric_eq(&self, text: String) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        let parsed = eq::parse_parametric_eq(&text)?;
        self.set_eq_inner(EqUpdate {
            enabled: true,
            preset_id: eq::WORKING_PRESET_ID.into(),
            bands: parsed.bands.to_vec(),
            preamp: parsed.preamp,
            auto_preamp: true,
        });
        self.finish()
    }

    pub fn save_custom_eq(&self, preset: EqUserPreset) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        let (eq, params) = {
            let mut logic = self.logic();
            eq::upsert_user_preset(&mut logic.eq, preset)?;
            (logic.eq.clone(), eq::EqParams::from_persist(&logic.eq))
        };
        self.persist.update(|data| data.eq = eq);
        let _ = self.engine.set_eq(params);
        self.finish()
    }

    pub fn delete_custom_eq(&self, id: String) -> AppResult<PlayerSnapshot> {
        let _op = self.begin();
        let (eq, params) = {
            let mut logic = self.logic();
            eq::delete_user_preset(&mut logic.eq, &id)?;
            (logic.eq.clone(), eq::EqParams::from_persist(&logic.eq))
        };
        self.persist.update(|data| data.eq = eq);
        let _ = self.engine.set_eq(params);
        self.finish()
    }

    // ---- inner operations (caller holds `ops`) --------------------------

    fn play_inner(&self) -> AppResult<()> {
        let current = self.logic().queue.current().map(|track| track.path.clone());
        let Some(current) = current else {
            return Err(AppError::msg("Nothing playing"));
        };
        if self.engine.is_empty() {
            // After Stop, or after the queue ran out: put the song back.
            return self.load_and_maybe_play(&current, true);
        }
        self.engine.play()?;
        self.logic().want_playing = true;
        Ok(())
    }

    fn pause_inner(&self) -> AppResult<()> {
        self.engine.pause()?;
        self.logic().want_playing = false;
        Ok(())
    }

    fn play_index_inner(&self, index: usize) -> AppResult<()> {
        let path = {
            let mut logic = self.logic();
            logic.clear_pending();
            logic
                .queue
                .play_index(index)
                .map(|track| track.path.clone())
                .ok_or_else(|| AppError::msg("that track is not in the queue"))?
        };
        self.load_and_maybe_play(&path, true)
    }

    fn play_path_inner(&self, path: &str) -> AppResult<()> {
        let index = self
            .logic()
            .queue
            .tracks
            .iter()
            .position(|track| track.path == path);
        if let Some(index) = index {
            return self.play_index_inner(index);
        }
        if Path::new(path).is_dir() {
            let jumped = {
                let mut logic = self.logic();
                logic.clear_pending();
                logic
                    .queue
                    .jump_to_folder(path)
                    .map(|track| track.path.clone())
            };
            if let Some(track_path) = jumped {
                return self.load_and_maybe_play(&track_path, true);
            }
        }
        self.open_path_inner(path, true)
    }

    fn play_tracks_inner(&self, tracks: Vec<Track>, start_path: Option<String>) -> AppResult<()> {
        if tracks.is_empty() {
            return Err(AppError::msg("Nothing to play"));
        }
        let start = start_path
            .as_deref()
            .and_then(|wanted| tracks.iter().position(|track| track.path == wanted))
            .unwrap_or(0);
        let first = {
            let mut logic = self.logic();
            logic.error = None;
            logic.clear_pending();
            logic.queue.replace(tracks, start);
            logic.queue.current().map(|track| track.path.clone())
        };
        if let Some(first) = first {
            self.load_and_maybe_play(&first, true)?;
        }
        Ok(())
    }

    fn set_eq_inner(&self, update: EqUpdate) {
        let params = eq::EqParams::from_update(&update);
        eq::apply_update(&mut self.logic().eq, &update);
        let _ = self.engine.set_eq(params);
        self.persist
            .update(|data| eq::apply_update(&mut data.eq, &update));
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
            let mut logic = self.logic();
            logic.root = Some(root.clone());
            logic.error = None;
            logic.clear_pending();
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

    /// Hand `path` to the engine. On failure the engine is stopped so the
    /// previous song does not keep playing under a queue that moved on.
    fn load_and_maybe_play(&self, path: &str, play: bool) -> AppResult<()> {
        // Invalidates an in-flight search download so it cannot replace this song.
        let _ = self.bump_play_generation();
        let result = (|| {
            // The queue keeps the library path. The engine gets a file it can open.
            let playable = self.engine.prepare(Path::new(path))?;
            self.engine.set_uri(&playable)?;
            Ok(())
        })();
        if let Err(error) = result {
            let _ = self.engine.stop();
            let mut logic = self.logic();
            logic.clear_pending();
            logic.want_playing = false;
            return Err(error);
        }
        self.host.track_loaded(Path::new(path));
        self.apply_volume();
        {
            let mut logic = self.logic();
            logic.clear_pending();
            logic.started_seen = 1;
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
        let logic = self.logic();
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

    // ---- gapless -------------------------------------------------------

    /// Near the end of a song, hand the next one to the engine so it can
    /// follow without a gap. Preparing may run ffmpeg, so it happens off
    /// the ticker; the append is committed under `ops` only if the queue
    /// still wants that song.
    fn maybe_queue_gapless(&self) {
        let next = {
            let mut logic = self.logic();
            if !logic.gapless || logic.pending_gapless {
                return;
            }
            let Some(path) = logic.queue.peek_next().map(|track| track.path.clone()) else {
                return;
            };
            logic.pending_gapless = true;
            logic.pending_path = Some(path.clone());
            path
        };
        if self.background_work {
            let player = self.clone();
            std::thread::Builder::new()
                .name("audios-gapless".into())
                .spawn(move || {
                    let prepared = player.engine.prepare(Path::new(&next));
                    let _op = player.ops.lock().unwrap_or_else(|e| e.into_inner());
                    player.commit_gapless(&next, prepared);
                })
                .ok();
        } else {
            // Tests: the caller (tick) already holds `ops`.
            let prepared = self.engine.prepare(Path::new(&next));
            self.commit_gapless(&next, prepared);
        }
    }

    /// Caller holds `ops`.
    fn commit_gapless(&self, next: &str, prepared: AppResult<PathBuf>) {
        let still_wanted = {
            let logic = self.logic();
            logic.pending_gapless && logic.pending_path.as_deref() == Some(next)
        };
        if !still_wanted {
            return;
        }
        let appended = prepared.and_then(|playable| self.engine.set_gapless_next(Some(&playable)));
        if appended.is_err() {
            self.logic().clear_pending();
        }
    }

    /// The queue changed under a pending append. If the song the engine
    /// holds is no longer what plays next, drop it; the next tick appends
    /// the right one.
    fn invalidate_gapless(&self) {
        let stale = {
            let logic = self.logic();
            logic.pending_gapless
                && logic.queue.peek_next().map(|track| track.path.as_str())
                    != logic.pending_path.as_deref()
        };
        if stale {
            self.cancel_pending_gapless();
        }
    }

    fn cancel_pending_gapless(&self) {
        let pending = self.logic().pending_gapless;
        if !pending {
            return;
        }
        let _ = self.engine.cancel_gapless_next();
        self.logic().clear_pending();
    }

    /// The engine moved on to an appended source. Move the queue with it.
    fn sync_hop(&self) {
        let hopped = {
            let mut logic = self.logic();
            let started = self.engine.started_sources();
            if started <= logic.started_seen {
                return;
            }
            logic.started_seen = started;
            if !logic.pending_gapless {
                return;
            }
            logic.clear_pending();
            logic.queue.advance();
            true
        };
        if hopped {
            self.apply_volume();
            self.emit_state();
        }
    }

    // ---- end of track --------------------------------------------------

    /// The sink ran dry. Step to the next song, skipping ones that will not
    /// open, and stop after one full pass so a broken file cannot spin.
    fn on_eos(&self) {
        let attempts = {
            let logic = self.logic();
            logic.queue.tracks.len() + logic.queue.user_queue_len() + 1
        };
        let mut skipped: Option<String> = None;
        for _ in 0..attempts {
            let next = {
                let mut logic = self.logic();
                logic.clear_pending();
                if logic.queue.repeat == RepeatMode::One && skipped.is_some() {
                    // Repeat one on a broken file: do not retry it forever.
                    None
                } else {
                    logic.queue.advance().map(|track| track.path.clone())
                }
            };
            let Some(path) = next else {
                break;
            };
            match self.load_and_maybe_play(&path, true) {
                Ok(()) => {
                    self.logic().error = skipped;
                    self.emit_state();
                    return;
                }
                Err(error) => {
                    let name = Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.clone());
                    skipped = Some(format!("Skipped {name}: {error}"));
                }
            }
        }
        let _ = self.engine.pause();
        {
            let mut logic = self.logic();
            logic.want_playing = false;
            if skipped.is_some() {
                logic.error = skipped;
            }
        }
        self.emit_state();
    }

    /// One step of end-of-track and gapless bookkeeping, then a position tick.
    /// The production ticker thread calls this every 250 ms.
    pub(crate) fn tick(&self) {
        let _op = self.begin();
        self.persist.flush_if_dirty();
        let (want_playing, duration) = {
            let logic = self.logic();
            (logic.want_playing, self.current_duration(&logic))
        };
        if want_playing && self.engine.is_empty() {
            self.on_eos();
            return;
        }
        let position = self.engine.position_ms();
        if want_playing && duration > 0 && position + GAPLESS_PREROLL_MS >= duration {
            self.maybe_queue_gapless();
        }
        self.emit_tick();
    }

    fn emit_state(&self) {
        self.host.emit_state(&self.snapshot());
        self.ping_media();
    }

    fn ping_media(&self) {
        let guard = self.media_ping.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = guard.as_ref() {
            let _ = tx.try_send(());
        }
    }

    fn emit_tick(&self) {
        let logic = self.logic();
        self.host.emit_tick(&Tick {
            position_ms: self.engine.position_ms(),
            duration_ms: self.current_duration(&logic),
        });
    }
}

impl Logic {
    fn clear_pending(&mut self) {
        self.pending_gapless = false;
        self.pending_path = None;
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
    use super::fake_engine::{Call, FakeEngine, RecordingHost};
    use super::scan::Track;
    use super::*;

    fn track(name: &str) -> Track {
        Track {
            path: format!("/music/{name}.mp3"),
            title: name.to_string(),
            artist: "Artist".into(),
            album: "Album".into(),
            album_artist: "Artist".into(),
            track: None,
            disc: None,
            duration_ms: 0,
            folder: "/music".into(),
            replaygain_track: None,
            replaygain_album: None,
        }
    }

    struct Rig {
        player: Player,
        engine: FakeEngine,
        host: RecordingHost,
        _dir: tempfile::TempDir,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        // Gapless defaults on; most transition tests want the plain path.
        store.update(|data| data.gapless = false);
        let engine = FakeEngine::new();
        let host = RecordingHost::default();
        let player = Player::with_parts(Arc::new(engine.clone()), Arc::new(host.clone()), store);
        Rig {
            player,
            engine,
            host,
            _dir: dir,
        }
    }

    fn current_path(player: &Player) -> Option<String> {
        player.snapshot().current.map(|track| track.path)
    }

    #[test]
    fn play_tracks_loads_the_start_track_and_plays() {
        let rig = rig();
        let tracks = vec![track("a"), track("b"), track("c")];
        rig.player
            .play_tracks(tracks, Some("/music/b.mp3".into()))
            .unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        assert!(rig.engine.is_playing());
        assert_eq!(
            rig.engine.current_source(),
            Some(PathBuf::from("/music/b.mp3"))
        );
        assert!(rig.host.last_state().is_some());
    }

    #[test]
    fn end_of_track_advances_to_the_next_song() {
        let rig = rig();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.engine.advance(20_000);
        rig.player.tick();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        assert!(rig.engine.is_playing());
        assert!(rig
            .engine
            .calls()
            .contains(&Call::SetUri("/music/b.mp3".into())));
    }

    #[test]
    fn end_of_queue_pauses_and_stays_on_last_song() {
        let rig = rig();
        rig.player.play_tracks(vec![track("a")], None).unwrap();
        rig.engine.advance(20_000);
        rig.player.tick();
        let snapshot = rig.player.snapshot();
        assert!(!snapshot.playing);
        assert_eq!(
            snapshot.current.map(|t| t.path).as_deref(),
            Some("/music/a.mp3")
        );
        // A second tick must not try to load anything again.
        rig.engine.clear_calls();
        rig.player.tick();
        assert!(rig.engine.calls().is_empty());
    }

    #[test]
    fn next_and_previous_walk_the_context() {
        let rig = rig();
        rig.player
            .play_tracks(vec![track("a"), track("b"), track("c")], None)
            .unwrap();
        rig.player.next().unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        rig.player.previous().unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/a.mp3"));
    }

    #[test]
    fn previous_after_three_seconds_restarts_the_song() {
        let rig = rig();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.player.next().unwrap();
        rig.engine.advance(5_000);
        rig.player.previous().unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        assert_eq!(rig.engine.position_ms(), 0);
    }

    #[test]
    fn gapless_appends_the_next_song_before_the_end() {
        let rig = rig();
        rig.player.set_gapless(true).unwrap();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.engine.advance(8_500);
        rig.player.tick();
        assert_eq!(
            rig.engine.sources(),
            vec![PathBuf::from("/music/a.mp3"), PathBuf::from("/music/b.mp3")]
        );
        // The hop: a runs out, b starts inside the sink without a reload.
        rig.engine.advance(2_000);
        rig.player.tick();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        assert!(!rig
            .engine
            .calls()
            .contains(&Call::SetUri("/music/b.mp3".into())));
    }

    #[test]
    fn short_track_duration_comes_from_the_engine() {
        let rig = rig();
        rig.engine.set_duration(Path::new("/music/a.mp3"), 3_000);
        rig.player.play_tracks(vec![track("a")], None).unwrap();
        assert_eq!(rig.player.snapshot().duration_ms, 3_000);
    }

    #[test]
    fn broken_next_track_is_skipped_with_a_note() {
        let rig = rig();
        rig.engine.mark_broken(Path::new("/music/b.mp3"));
        rig.player
            .play_tracks(vec![track("a"), track("b"), track("c")], None)
            .unwrap();
        rig.engine.advance(20_000);
        rig.player.tick();
        let snapshot = rig.player.snapshot();
        assert!(snapshot.error.unwrap().starts_with("Skipped b.mp3"));
        assert_eq!(
            snapshot.current.map(|t| t.path).as_deref(),
            Some("/music/c.mp3")
        );
        assert!(rig.engine.is_playing());
    }

    #[test]
    fn all_broken_stops_after_one_pass_and_does_not_spin() {
        let rig = rig();
        rig.player.set_repeat(RepeatMode::All).unwrap();
        rig.engine.mark_broken(Path::new("/music/b.mp3"));
        rig.engine.mark_broken(Path::new("/music/c.mp3"));
        rig.player
            .play_tracks(vec![track("a"), track("b"), track("c")], None)
            .unwrap();
        rig.engine.advance(20_000);
        rig.player.tick();
        // b, c fail, a wraps around and plays: bounded, and playing.
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/a.mp3"));
        assert!(rig.engine.is_playing());
        rig.engine.mark_broken(Path::new("/music/a.mp3"));
        rig.engine.advance(20_000);
        rig.player.tick();
        assert!(!rig.player.snapshot().playing);
        let after_stop = rig.engine.calls().len();
        // Further ticks do nothing: no ffmpeg storm, no state.json churn.
        rig.player.tick();
        rig.player.tick();
        assert_eq!(rig.engine.calls().len(), after_stop);
    }

    #[test]
    fn repeat_one_on_a_broken_file_stops_instead_of_retrying() {
        let rig = rig();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.player.set_repeat(RepeatMode::One).unwrap();
        rig.engine.mark_broken(Path::new("/music/a.mp3"));
        rig.engine.advance(20_000);
        rig.player.tick();
        assert!(!rig.player.snapshot().playing);
        assert!(rig.player.snapshot().error.is_some());
        rig.engine.clear_calls();
        rig.player.tick();
        assert!(rig.engine.calls().is_empty());
    }

    #[test]
    fn play_after_the_queue_ended_restarts_the_last_song() {
        let rig = rig();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.engine.advance(50_000);
        rig.player.tick(); // a ends, b starts
        rig.engine.advance(50_000);
        rig.player.tick(); // b ends, queue is done
        assert!(!rig.player.snapshot().playing);
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        rig.player.play().unwrap();
        assert!(rig.engine.is_playing());
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        assert_eq!(rig.engine.position_ms(), 0);
    }

    #[test]
    fn stop_then_play_resumes_the_same_song() {
        let rig = rig();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.player.stop().unwrap();
        assert!(rig.engine.is_empty());
        rig.player.play().unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/a.mp3"));
        assert!(rig.engine.is_playing());
        rig.player.stop().unwrap();
        rig.player.toggle().unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/a.mp3"));
    }

    #[test]
    fn previous_at_end_of_queue_goes_back_not_restart() {
        let rig = rig();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.engine.advance(50_000);
        rig.player.tick(); // a ends, b starts
        rig.engine.advance(50_000);
        rig.player.tick(); // b ends, queue is done
        assert!(rig.engine.is_empty());
        // The empty sink's stale end position must not turn this into seek(0).
        rig.player.previous().unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/a.mp3"));
        assert!(rig.engine.is_playing());
    }

    #[test]
    fn shuffle_toggle_drops_a_stale_gapless_append() {
        let rig = rig();
        rig.player.set_gapless(true).unwrap();
        let tracks: Vec<Track> = (0..6).map(|i| track(&format!("t{i}"))).collect();
        rig.player.play_tracks(tracks, None).unwrap();
        rig.engine.advance(8_500);
        rig.player.tick();
        assert_eq!(rig.engine.sources().len(), 2);
        let appended = rig.engine.sources()[1].clone();
        let mut changed = false;
        for _ in 0..40 {
            rig.player.set_shuffle(true).unwrap();
            let want = rig
                .player
                .logic()
                .queue
                .peek_next()
                .map(|t| PathBuf::from(&t.path));
            if want.as_ref() != Some(&appended) {
                changed = true;
                break;
            }
            rig.player.set_shuffle(false).unwrap();
        }
        assert!(changed, "could not get a differing shuffle order");
        assert!(rig.engine.calls().contains(&Call::CancelAppend));
        assert_eq!(rig.engine.sources().len(), 1, "stale append dropped");
        rig.player.tick();
        let want = rig
            .player
            .logic()
            .queue
            .peek_next()
            .map(|t| PathBuf::from(&t.path))
            .unwrap();
        assert_eq!(rig.engine.sources().get(1), Some(&want));
    }

    #[test]
    fn enqueue_during_preroll_replaces_the_gapless_append() {
        let rig = rig();
        rig.player.set_gapless(true).unwrap();
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        rig.engine.advance(8_500);
        rig.player.tick();
        assert_eq!(rig.engine.sources()[1], PathBuf::from("/music/b.mp3"));
        let extra = rig._dir.path().join("extra.mp3");
        std::fs::write(&extra, []).unwrap();
        rig.player.enqueue_path(extra.to_str().unwrap()).unwrap();
        rig.player.tick();
        assert_eq!(rig.engine.sources()[1], extra, "queued song plays next");
        rig.engine.advance(2_000);
        rig.player.tick();
        assert_eq!(
            current_path(&rig.player),
            Some(extra.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn duration_follows_the_song_after_a_gapless_hop() {
        let rig = rig();
        rig.player.set_gapless(true).unwrap();
        rig.engine.set_duration(Path::new("/music/a.mp3"), 10_000);
        rig.engine.set_duration(Path::new("/music/b.mp3"), 4_000);
        rig.player
            .play_tracks(vec![track("a"), track("b"), track("c")], None)
            .unwrap();
        rig.engine.advance(8_500);
        rig.player.tick();
        rig.engine.advance(2_000);
        rig.player.tick();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/b.mp3"));
        assert_eq!(rig.player.snapshot().duration_ms, 4_000);
        // b is short, so c is appended in time: the chain continues.
        rig.engine.advance(2_000);
        rig.player.tick();
        assert_eq!(
            rig.engine.sources().get(1),
            Some(&PathBuf::from("/music/c.mp3"))
        );
    }

    #[test]
    fn next_pressed_before_the_tick_sees_a_hop_does_not_double_skip() {
        let rig = rig();
        rig.player.set_gapless(true).unwrap();
        rig.player
            .play_tracks(vec![track("a"), track("b"), track("c")], None)
            .unwrap();
        rig.engine.advance(8_500);
        rig.player.tick();
        rig.engine.advance(2_000); // hop to b; no tick yet
        rig.player.next().unwrap();
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/c.mp3"));
    }

    #[test]
    fn failed_load_on_next_stops_the_old_song() {
        let rig = rig();
        rig.engine.mark_broken(Path::new("/music/b.mp3"));
        rig.player
            .play_tracks(vec![track("a"), track("b")], None)
            .unwrap();
        assert!(rig.player.next().is_err());
        assert!(rig.engine.is_empty(), "a must not keep playing under b");
        assert!(!rig.player.snapshot().playing);
    }

    #[test]
    fn a_library_play_invalidates_an_in_flight_search() {
        let rig = rig();
        let search = rig.player.bump_play_generation();
        rig.player.play_tracks(vec![track("a")], None).unwrap();
        assert_ne!(
            search,
            rig.player.play_generation(),
            "starting a song must retire a pending search-play"
        );
        assert_eq!(current_path(&rig.player).as_deref(), Some("/music/a.mp3"));
    }

    #[test]
    fn volume_writes_are_coalesced_by_the_ticker() {
        let rig = rig();
        let state = rig._dir.path().join("state.json");
        std::fs::remove_file(&state).ok(); // rig() wrote defaults once
        for step in 0..20 {
            rig.player.set_volume(step as f64 / 20.0).unwrap();
        }
        assert!(!state.exists(), "no write per slider event");
        rig.player.tick();
        assert!(state.exists());
    }

    #[test]
    fn unreadable_settings_show_up_in_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "not json").unwrap();
        let store = Store::load_from(path);
        let player = Player::with_parts(
            Arc::new(FakeEngine::new()),
            Arc::new(RecordingHost::default()),
            store,
        );
        assert!(player
            .snapshot()
            .error
            .unwrap()
            .contains("could not be read"));
    }

    #[test]
    fn toggle_with_nothing_loaded_is_an_error() {
        let rig = rig();
        assert!(rig.player.toggle().is_err());
    }

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

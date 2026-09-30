//! A scripted engine for driving the `Player` state machine in tests.
//!
//! It models what the rodio engine publishes: a list of queued sources, a
//! position inside the first one, and a playing flag. Tests move time with
//! [`FakeEngine::advance`] and call `Player::tick` to observe transitions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::eq::EqParams;
use crate::error::{AppError, AppResult};

use super::engine::PlayerEngine;
use super::host::Host;
use super::{PlayerSnapshot, Tick};

pub const DEFAULT_DURATION_MS: u64 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Prepare(PathBuf),
    SetUri(PathBuf),
    Append(PathBuf),
    Play,
    Pause,
    Stop,
    Seek(u64),
    CancelAppend,
}

#[derive(Default)]
struct State {
    sources: Vec<PathBuf>,
    position_ms: u64,
    playing: bool,
    /// Sources that have produced sound since the last `set_uri`.
    started: u64,
    speed: f64,
    volume: f64,
    durations: HashMap<PathBuf, u64>,
    /// Paths `prepare`/`set_uri` reject, to model an unreadable file.
    broken: Vec<PathBuf>,
    calls: Vec<Call>,
}

#[derive(Clone, Default)]
pub struct FakeEngine {
    state: Arc<Mutex<State>>,
}

impl FakeEngine {
    pub fn new() -> Self {
        let engine = Self::default();
        engine.state.lock().unwrap().speed = 1.0;
        engine
    }

    pub fn set_duration(&self, path: &Path, duration_ms: u64) {
        self.state
            .lock()
            .unwrap()
            .durations
            .insert(path.to_path_buf(), duration_ms);
    }

    pub fn mark_broken(&self, path: &Path) {
        self.state.lock().unwrap().broken.push(path.to_path_buf());
    }

    /// Move playback forward. When the first source runs out it is dropped
    /// and the position restarts inside the next one, like a rodio sink.
    pub fn advance(&self, mut ms: u64) {
        let mut state = self.state.lock().unwrap();
        if !state.playing {
            return;
        }
        if state.started == 0 && !state.sources.is_empty() {
            state.started = 1;
        }
        while ms > 0 && !state.sources.is_empty() {
            let duration = state
                .durations
                .get(&state.sources[0])
                .copied()
                .unwrap_or(DEFAULT_DURATION_MS);
            let remaining = duration.saturating_sub(state.position_ms);
            if ms < remaining {
                state.position_ms += ms;
                ms = 0;
            } else {
                ms -= remaining;
                state.sources.remove(0);
                state.position_ms = 0;
                if !state.sources.is_empty() {
                    state.started += 1;
                }
            }
        }
        if state.sources.is_empty() {
            state.playing = false;
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }

    pub fn clear_calls(&self) {
        self.state.lock().unwrap().calls.clear();
    }

    pub fn current_source(&self) -> Option<PathBuf> {
        self.state.lock().unwrap().sources.first().cloned()
    }

    pub fn sources(&self) -> Vec<PathBuf> {
        self.state.lock().unwrap().sources.clone()
    }

    fn record(&self, call: Call) {
        self.state.lock().unwrap().calls.push(call);
    }
}

impl PlayerEngine for FakeEngine {
    fn prepare(&self, path: &Path) -> AppResult<PathBuf> {
        self.record(Call::Prepare(path.to_path_buf()));
        if self.state.lock().unwrap().broken.iter().any(|p| p == path) {
            return Err(AppError::msg(format!("cannot decode {}", path.display())));
        }
        Ok(path.to_path_buf())
    }

    fn set_uri(&self, path: &Path) -> AppResult<()> {
        self.record(Call::SetUri(path.to_path_buf()));
        let mut state = self.state.lock().unwrap();
        state.sources = vec![path.to_path_buf()];
        state.position_ms = 0;
        state.playing = false;
        state.started = 0;
        Ok(())
    }

    fn play(&self) -> AppResult<()> {
        self.record(Call::Play);
        let mut state = self.state.lock().unwrap();
        state.playing = !state.sources.is_empty();
        if state.playing && state.started == 0 {
            state.started = 1;
        }
        Ok(())
    }

    fn pause(&self) -> AppResult<()> {
        self.record(Call::Pause);
        self.state.lock().unwrap().playing = false;
        Ok(())
    }

    fn stop(&self) -> AppResult<()> {
        self.record(Call::Stop);
        let mut state = self.state.lock().unwrap();
        state.sources.clear();
        state.position_ms = 0;
        state.playing = false;
        Ok(())
    }

    fn seek(&self, position_ms: u64) -> AppResult<()> {
        self.record(Call::Seek(position_ms));
        self.state.lock().unwrap().position_ms = position_ms;
        Ok(())
    }

    fn position_ms(&self) -> u64 {
        self.state.lock().unwrap().position_ms
    }

    fn duration_ms(&self) -> u64 {
        let state = self.state.lock().unwrap();
        state
            .sources
            .first()
            .and_then(|path| state.durations.get(path).copied())
            .unwrap_or(if state.sources.is_empty() {
                0
            } else {
                DEFAULT_DURATION_MS
            })
    }

    fn set_volume(&self, volume: f64) -> AppResult<()> {
        self.state.lock().unwrap().volume = volume;
        Ok(())
    }

    fn set_speed(&self, speed: f64) -> AppResult<()> {
        self.state.lock().unwrap().speed = speed;
        Ok(())
    }

    fn speed(&self) -> f64 {
        self.state.lock().unwrap().speed
    }

    fn set_eq(&self, _params: EqParams) -> AppResult<()> {
        Ok(())
    }

    fn set_gapless_next(&self, path: Option<&Path>) -> AppResult<()> {
        let Some(path) = path else {
            return Ok(());
        };
        self.record(Call::Append(path.to_path_buf()));
        let mut state = self.state.lock().unwrap();
        state.sources.truncate(1);
        state.sources.push(path.to_path_buf());
        Ok(())
    }

    fn cancel_gapless_next(&self) -> AppResult<()> {
        self.record(Call::CancelAppend);
        self.state.lock().unwrap().sources.truncate(1);
        Ok(())
    }

    fn started_sources(&self) -> u64 {
        self.state.lock().unwrap().started
    }

    fn is_playing(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.playing && !state.sources.is_empty()
    }

    fn is_empty(&self) -> bool {
        self.state.lock().unwrap().sources.is_empty()
    }

    fn sample_rate(&self) -> u32 {
        44_100
    }

    fn set_viz_enabled(&self, _enabled: bool) {}

    fn viz_enabled(&self) -> bool {
        false
    }

    fn spectrum(&self) -> Option<[u8; crate::viz::BANDS]> {
        None
    }
}

/// Records emitted events instead of talking to a webview.
#[derive(Clone, Default)]
pub struct RecordingHost {
    pub states: Arc<Mutex<Vec<PlayerSnapshot>>>,
    pub ticks: Arc<Mutex<Vec<Tick>>>,
}

impl RecordingHost {
    pub fn last_state(&self) -> Option<PlayerSnapshot> {
        self.states.lock().unwrap().last().cloned()
    }
}

impl Host for RecordingHost {
    fn emit_state(&self, snapshot: &PlayerSnapshot) {
        self.states.lock().unwrap().push(snapshot.clone());
    }

    fn emit_tick(&self, tick: &Tick) {
        self.ticks.lock().unwrap().push(tick.clone());
    }

    fn emit_viz(&self, _bands: [u8; crate::viz::BANDS]) {}

    fn raise_window(&self) {}

    fn quit_window(&self) {}

    fn track_loaded(&self, _path: &Path) {}
}

//! Side effects the player performs on the application, kept behind a trait
//! so the state machine can be driven in tests without a Tauri runtime.

use std::path::Path;

use tauri::{AppHandle, Emitter, Manager};

use super::{PlayerSnapshot, Tick, STATE_EVENT, TICK_EVENT, VIZ_EVENT};

pub trait Host: Send + Sync {
    fn emit_state(&self, snapshot: &PlayerSnapshot);
    fn emit_tick(&self, tick: &Tick);
    fn emit_viz(&self, bands: [u8; crate::viz::BANDS]);
    fn raise_window(&self);
    fn quit_window(&self);
    /// A new file was handed to the engine. The Tauri host prunes search
    /// temps that are not this file.
    fn track_loaded(&self, path: &Path);
}

pub struct TauriHost {
    app: AppHandle,
}

impl TauriHost {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Host for TauriHost {
    fn emit_state(&self, snapshot: &PlayerSnapshot) {
        let _ = self.app.emit(STATE_EVENT, snapshot);
    }

    fn emit_tick(&self, tick: &Tick) {
        let _ = self.app.emit(TICK_EVENT, tick);
    }

    fn emit_viz(&self, bands: [u8; crate::viz::BANDS]) {
        let _ = self.app.emit(VIZ_EVENT, bands);
    }

    fn raise_window(&self) {
        let Some(window) = self.app.get_webview_window("main") else {
            return;
        };
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }

    fn quit_window(&self) {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.close();
        }
    }

    fn track_loaded(&self, path: &Path) {
        crate::search::drop_temps_except(Some(path));
    }
}

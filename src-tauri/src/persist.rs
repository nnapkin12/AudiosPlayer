use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::eq::EqPersist;
use crate::error::AppResult;
use crate::player::queue::RepeatMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItem {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub items: Vec<PlaylistItem>,
    #[serde(default)]
    pub has_cover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistData {
    pub last_root: Option<String>,
    #[serde(default)]
    pub library_roots: Vec<String>,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub repeat: RepeatMode,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default = "default_true")]
    pub replaygain: bool,
    #[serde(default = "default_true")]
    pub gapless: bool,
    #[serde(default = "default_speed")]
    pub speed: f64,
    #[serde(default)]
    pub positions: HashMap<String, u64>,
    #[serde(default)]
    pub playlists: Vec<Playlist>,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_accent")]
    pub accent: String,
    #[serde(default)]
    pub custom_themes: Vec<CustomTheme>,
    #[serde(default)]
    pub minimize_movement: bool,
    #[serde(default)]
    pub visualizer: bool,
    #[serde(default = "default_viz_main")]
    pub visualizer_main: String,
    #[serde(default = "default_viz_border")]
    pub visualizer_border: String,
    #[serde(default = "default_viz_glow")]
    pub visualizer_glow: String,
    #[serde(default)]
    pub eq: EqPersist,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomTheme {
    pub id: String,
    pub name: String,
    pub colors: ThemeColors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColors {
    pub frame: String,
    pub app: String,
    pub raised: String,
    pub bar: String,
    pub bar_line: String,
    pub hover: String,
    pub border: String,
    pub line: String,
    pub muted: String,
    pub text: String,
    pub subtle: String,
    pub danger: String,
    pub play: String,
    pub play_fg: String,
    pub accent: String,
    pub accent_dim: String,
}

fn default_theme() -> String {
    "dusk".into()
}

fn default_accent() -> String {
    "blue".into()
}

fn default_speed() -> f64 {
    1.0
}

fn default_volume() -> f64 {
    0.85
}

fn default_true() -> bool {
    true
}

pub fn default_viz_main() -> String {
    "#8ec8ff".into()
}

pub fn default_viz_border() -> String {
    "#e8f3ff".into()
}

pub fn default_viz_glow() -> String {
    "#4aa3ff".into()
}

pub fn hex_color(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        return value.to_ascii_lowercase();
    }
    fallback.into()
}

impl Default for PersistData {
    fn default() -> Self {
        Self {
            last_root: None,
            library_roots: Vec::new(),
            volume: default_volume(),
            muted: false,
            repeat: RepeatMode::Off,
            shuffle: false,
            replaygain: default_true(),
            gapless: default_true(),
            speed: default_speed(),
            positions: HashMap::new(),
            playlists: Vec::new(),
            theme: default_theme(),
            accent: default_accent(),
            custom_themes: Vec::new(),
            minimize_movement: false,
            visualizer: false,
            visualizer_main: default_viz_main(),
            visualizer_border: default_viz_border(),
            visualizer_glow: default_viz_glow(),
            eq: EqPersist::default(),
        }
    }
}

#[derive(Clone)]
pub struct Store {
    path: PathBuf,
    data: Arc<Mutex<PersistData>>,
    /// Something the user should know about their saved state: the file
    /// could not be read, or the last write failed.
    warning: Arc<Mutex<Option<String>>>,
    /// In-memory changes not yet on disk. See [`Store::update_soon`].
    dirty: Arc<AtomicBool>,
    /// Bumped on every in-memory change so the folder watcher can skip `stat`.
    revision: Arc<AtomicU64>,
    /// Session-only status (inotify limit, and similar). Not written to disk.
    session_note: Arc<Mutex<Option<String>>>,
}

impl Store {
    #[cfg(test)]
    pub fn for_test(dir: &Path) -> Self {
        Self {
            path: dir.join("state.json"),
            data: Arc::new(Mutex::new(PersistData::default())),
            warning: Arc::new(Mutex::new(None)),
            dirty: Arc::new(AtomicBool::new(false)),
            revision: Arc::new(AtomicU64::new(0)),
            session_note: Arc::new(Mutex::new(None)),
        }
    }

    pub fn load() -> Self {
        Self::load_from(config_path())
    }

    /// Read `path`. A missing file is a fresh install. A file that exists but
    /// does not parse is moved aside as `state.json.corrupt-<unix time>` so
    /// nothing is overwritten, and the warning is kept for the UI.
    pub fn load_from(path: PathBuf) -> Self {
        let mut warning = None;
        let mut data = match std::fs::read_to_string(&path) {
            Err(_) => PersistData::default(),
            Ok(raw) => match serde_json::from_str::<PersistData>(&raw) {
                Ok(data) => data,
                Err(error) => {
                    let quarantine = quarantine_path(&path);
                    let moved = std::fs::rename(&path, &quarantine).is_ok();
                    warning = Some(if moved {
                        format!(
                            "Saved settings could not be read ({error}). The file was kept as {} and defaults are in use.",
                            quarantine.display()
                        )
                    } else {
                        format!(
                            "Saved settings could not be read ({error}). Defaults are in use and will not be saved over {}.",
                            path.display()
                        )
                    });
                    eprintln!("Audios! state: {}", warning.as_deref().unwrap_or(""));
                    PersistData::default()
                }
            },
        };
        if data.library_roots.is_empty() {
            if let Some(root) = data.last_root.clone() {
                if !root.trim().is_empty() {
                    data.library_roots.push(root);
                }
            }
        }
        // If the broken file could not be moved, refuse to write over it.
        let path = if warning.is_some() && path.exists() {
            path.with_extension("json.unsaved")
        } else {
            path
        };
        Self {
            path,
            data: Arc::new(Mutex::new(data)),
            warning: Arc::new(Mutex::new(warning)),
            dirty: Arc::new(AtomicBool::new(false)),
            revision: Arc::new(AtomicU64::new(0)),
            session_note: Arc::new(Mutex::new(None)),
        }
    }

    pub fn snapshot(&self) -> PersistData {
        self.data.lock().expect("persist lock").clone()
    }

    /// A load or save problem worth showing. Cleared once a save succeeds
    /// after a save failure; a load warning stays for the session.
    pub fn warning(&self) -> Option<String> {
        self.warning.lock().expect("persist warning").clone()
    }

    pub fn session_note(&self) -> Option<String> {
        self.session_note
            .lock()
            .expect("persist session note")
            .clone()
    }

    /// Shown in the status line for this session. Does not overwrite a
    /// corrupt-file warning, and is not written to `state.json`.
    pub fn set_session_note(&self, note: Option<String>) {
        let mut slot = self.session_note.lock().expect("persist session note");
        *slot = note;
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    fn bump_revision(&self) {
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    pub fn config_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.path.clone())
    }

    pub fn update<F>(&self, mutate: F)
    where
        F: FnOnce(&mut PersistData),
    {
        let mut data = self.data.lock().expect("persist lock");
        mutate(&mut data);
        self.bump_revision();
        self.dirty.store(false, Ordering::Relaxed);
        self.save_locked(&data);
    }

    /// Change the in-memory state now and write it on the next
    /// [`Store::flush_if_dirty`]. For values that change many times a second.
    pub fn update_soon<F>(&self, mutate: F)
    where
        F: FnOnce(&mut PersistData),
    {
        let mut data = self.data.lock().expect("persist lock");
        mutate(&mut data);
        self.bump_revision();
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Write pending `update_soon` changes. Cheap when there are none.
    pub fn flush_if_dirty(&self) {
        if !self.dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        let data = self.data.lock().expect("persist lock");
        self.save_locked(&data);
    }

    fn save_locked(&self, data: &PersistData) {
        match save_to(&self.path, data) {
            Ok(()) => {
                let mut warning = self.warning.lock().expect("persist warning");
                if warning
                    .as_deref()
                    .is_some_and(|w| w.starts_with(SAVE_FAILED))
                {
                    *warning = None;
                }
            }
            Err(error) => {
                let message = format!("{SAVE_FAILED} ({error}). Changes are not being saved.");
                eprintln!("Audios! state: {message}");
                let mut warning = self.warning.lock().expect("persist warning");
                // Do not replace a load warning; that one explains more.
                if warning.is_none()
                    || warning
                        .as_deref()
                        .is_some_and(|w| w.starts_with(SAVE_FAILED))
                {
                    *warning = Some(message);
                }
            }
        }
    }
}

pub fn rename_atomic(from: &Path, to: &Path) -> AppResult<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)?;
            Ok(())
        }
    }
}

const SAVE_FAILED: &str = "Could not save settings";

fn quarantine_path(path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "state.json".into());
    path.with_file_name(format!("{name}.corrupt-{stamp}"))
}

fn config_path() -> PathBuf {
    let dirs = directories::ProjectDirs::from("com", "audios", "Audios")
        .expect("a home directory is required");
    let dir = dirs.config_dir();
    let _ = std::fs::create_dir_all(dir);
    dir.join("state.json")
}

fn save_to(path: &Path, data: &PersistData) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(data)?)?;
    rename_atomic(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_volume_is_safe() {
        let data = PersistData::default();
        assert!(data.volume > 0.0 && data.volume <= 1.0);
        assert!(data.gapless);
        assert!(data.replaygain);
        assert!(data.library_roots.is_empty());
        assert!(data.custom_themes.is_empty());
        assert!(!data.eq.enabled);
        assert_eq!(data.eq.preset_id, "flat");
        assert!(data.eq.auto_preamp);
        assert!(data.eq.custom_presets.is_empty());
    }

    #[test]
    fn appearance_survives_old_state() {
        let data: PersistData = serde_json::from_str(
            r#"{"lastRoot":null,"volume":0.5,"muted":false,"repeat":"off","shuffle":false,"replaygain":true,"gapless":true,"positions":{}}"#,
        )
        .unwrap();
        assert_eq!(data.theme, "dusk");
        assert!(data.custom_themes.is_empty());
        assert!(!data.minimize_movement);
        assert!(!data.visualizer);
        assert_eq!(data.visualizer_main, default_viz_main());
        assert!(!data.eq.enabled);
        assert_eq!(data.eq.bands.len(), 10);
    }

    #[test]
    fn eq_user_preset_round_trip_in_state() {
        let mut data = PersistData::default();
        data.eq.enabled = true;
        data.eq.preset_id = "custom-1".into();
        data.eq.bands[0].gain = 1.0;
        data.eq.bands[9].gain = 2.0;
        data.eq.custom_presets.push(crate::eq::EqUserPreset {
            id: "custom-1".into(),
            name: "Desk".into(),
            bands: data.eq.bands,
            preamp: -2.0,
            auto_preamp: true,
        });
        let raw = serde_json::to_string(&data).unwrap();
        let back: PersistData = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.eq.preset_id, "custom-1");
        assert_eq!(back.eq.custom_presets[0].name, "Desk");
        assert_eq!(back.eq.bands[9].gain, 2.0);
        let legacy = r#"{"lastRoot":null,"volume":0.5,"muted":false,"repeat":"off","shuffle":false,"replaygain":true,"gapless":true,"positions":{},"eq":{"enabled":true,"presetId":"custom","gains":[4,0,0,0,0,0,0,0,0,1],"preamp":-1,"autoPreamp":true}}"#;
        let old: PersistData = serde_json::from_str(legacy).unwrap();
        assert_eq!(old.eq.bands[0].freq, 32.0);
        assert_eq!(old.eq.bands[0].gain, 4.0);
        assert_eq!(old.eq.bands[9].gain, 1.0);
    }

    #[test]
    fn corrupt_state_is_quarantined_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{\"volume\": 0.5, \"playlists\": [{\"id\"").unwrap();
        let store = Store::load_from(path.clone());
        assert!(store.warning().unwrap().contains("could not be read"));
        assert!(!path.exists(), "the broken file must be moved aside");
        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("state.json.corrupt-"))
            .collect();
        assert_eq!(kept.len(), 1);
        // A later save writes a fresh file and leaves the quarantine alone.
        store.update(|data| data.volume = 0.3);
        assert!(path.exists());
        assert_eq!(kept.len(), 1);
        assert!(
            store.warning().is_some(),
            "load warning stays for the session"
        );
    }

    #[test]
    fn missing_state_is_a_fresh_install() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::load_from(dir.path().join("state.json"));
        assert!(store.warning().is_none());
        assert_eq!(store.snapshot().volume, default_volume());
    }

    #[test]
    fn partial_v1_state_fills_defaults() {
        // Only one v1 field present: nothing else may be required.
        let data: PersistData = serde_json::from_str(r#"{"volume":0.2}"#).unwrap();
        assert_eq!(data.volume, 0.2);
        assert!(data.gapless);
        assert!(data.replaygain);
        assert!(!data.muted);
        assert!(data.positions.is_empty());
    }

    #[test]
    fn save_failure_is_reported_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should be makes the rename fail.
        let path = dir.path().join("state.json");
        std::fs::create_dir_all(&path).unwrap();
        let store = Store {
            path: path.clone(),
            data: Arc::new(Mutex::new(PersistData::default())),
            warning: Arc::new(Mutex::new(None)),
            dirty: Arc::new(AtomicBool::new(false)),
            revision: Arc::new(AtomicU64::new(0)),
            session_note: Arc::new(Mutex::new(None)),
        };
        store.update(|data| data.volume = 0.1);
        assert!(store.warning().unwrap().starts_with(SAVE_FAILED));
        std::fs::remove_dir_all(&path).unwrap();
        store.update(|data| data.volume = 0.2);
        assert!(store.warning().is_none());
    }

    #[test]
    fn update_soon_waits_for_flush() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let path = dir.path().join("state.json");
        store.update_soon(|data| data.volume = 0.42);
        store.update_soon(|data| data.volume = 0.43);
        assert!(!path.exists(), "nothing written yet");
        assert_eq!(store.snapshot().volume, 0.43, "memory is current");
        store.flush_if_dirty();
        let saved: PersistData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.volume, 0.43);
        // A plain update writes through and clears the flag.
        std::fs::remove_file(&path).unwrap();
        store.flush_if_dirty();
        assert!(!path.exists());
        store.update(|data| data.muted = true);
        assert!(path.exists());
    }
}

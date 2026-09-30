use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    pub volume: f64,
    pub muted: bool,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    pub replaygain: bool,
    pub gapless: bool,
    #[serde(default = "default_speed")]
    pub speed: f64,
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
            volume: 0.85,
            muted: false,
            repeat: RepeatMode::Off,
            shuffle: false,
            replaygain: true,
            gapless: true,
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
}

impl Store {
    #[cfg(test)]
    pub fn for_test(dir: &Path) -> Self {
        Self {
            path: dir.join("state.json"),
            data: Arc::new(Mutex::new(PersistData::default())),
        }
    }

    pub fn load() -> Self {
        let path = config_path();
        let mut data: PersistData = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        if data.library_roots.is_empty() {
            if let Some(root) = data.last_root.clone() {
                if !root.trim().is_empty() {
                    data.library_roots.push(root);
                }
            }
        }
        Self {
            path,
            data: Arc::new(Mutex::new(data)),
        }
    }

    pub fn snapshot(&self) -> PersistData {
        self.data.lock().expect("persist lock").clone()
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
        let _ = save_to(&self.path, &data);
    }

    #[cfg(test)]
    pub fn position_for(&self, path: &str) -> Option<u64> {
        self.data
            .lock()
            .expect("persist lock")
            .positions
            .get(path)
            .copied()
    }

    pub fn forget_position(&self, path: &str) {
        self.update(|data| {
            data.positions.remove(path);
        });
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
    fn forgets_saved_position() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store {
            path: dir.path().join("state.json"),
            data: Arc::new(Mutex::new(PersistData::default())),
        };
        store.update(|data| {
            data.positions.insert("/song.flac".into(), 20_000);
        });
        assert_eq!(store.position_for("/song.flac"), Some(20_000));
        store.forget_position("/song.flac");
        assert!(store.position_for("/song.flac").is_none());
    }
}

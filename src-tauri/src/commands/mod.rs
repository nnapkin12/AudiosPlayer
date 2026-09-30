use std::path::Path;
use std::sync::Arc;

use tauri::State;

use crate::eq::{EqUpdate, EqUserPreset};
use crate::error::{AppError, AppResult};
use crate::persist::{CustomTheme, Playlist, Store};
use crate::player::queue::RepeatMode;
use crate::player::scan::Track;
use crate::player::{Player, PlayerSnapshot};
use crate::search::MediaHit;
use crate::tags::{BatchResult, CoverArt, TagDoc, TagFields};

#[tauri::command]
pub fn player_state(player: State<Player>) -> PlayerSnapshot {
    player.snapshot_ui()
}

#[tauri::command]
pub async fn open_path(player: State<'_, Player>, path: String) -> AppResult<PlayerSnapshot> {
    let player = (*player).clone();
    run_blocking(move || player.open_path(&path)).await
}

#[tauri::command]
pub fn play(player: State<Player>) -> AppResult<PlayerSnapshot> {
    player.play()
}

#[tauri::command]
pub fn pause(player: State<Player>) -> AppResult<PlayerSnapshot> {
    player.pause()
}

#[tauri::command]
pub fn toggle(player: State<Player>) -> AppResult<PlayerSnapshot> {
    player.toggle()
}

#[tauri::command]
pub fn stop(player: State<Player>) -> AppResult<PlayerSnapshot> {
    player.stop()
}

#[tauri::command]
pub fn next_track(player: State<Player>) -> AppResult<PlayerSnapshot> {
    player.next()
}

#[tauri::command]
pub fn previous_track(player: State<Player>) -> AppResult<PlayerSnapshot> {
    player.previous()
}

#[tauri::command]
pub fn seek(player: State<Player>, position_ms: u64) -> AppResult<PlayerSnapshot> {
    player.seek(position_ms)
}

#[tauri::command]
pub fn play_index(player: State<Player>, index: usize) -> AppResult<PlayerSnapshot> {
    player.play_index(index)
}

#[tauri::command]
pub async fn play_path(player: State<'_, Player>, path: String) -> AppResult<PlayerSnapshot> {
    let player = (*player).clone();
    run_blocking(move || player.play_path(&path)).await
}

#[tauri::command]
pub fn enqueue_path(player: State<Player>, path: String) -> AppResult<PlayerSnapshot> {
    player.enqueue_path(&path)
}

#[tauri::command]
pub async fn play_playlist(
    player: State<'_, Player>,
    store: State<'_, Store>,
    id: String,
    start_path: Option<String>,
) -> AppResult<PlayerSnapshot> {
    let player = (*player).clone();
    let store = (*store).clone();
    run_blocking(move || {
        crate::playlists::sync(&store);
        let playlists = crate::playlists::list(&store);
        let playlist = playlists
            .iter()
            .find(|playlist| playlist.id == id)
            .ok_or_else(|| crate::error::AppError::msg("playlist not found"))?;
        player.play_queue_paths(crate::playlists::flatten_paths(playlist), start_path)
    })
    .await
}

#[tauri::command]
pub async fn play_queue_paths(
    player: State<'_, Player>,
    paths: Vec<String>,
    start_path: Option<String>,
) -> AppResult<PlayerSnapshot> {
    let player = (*player).clone();
    run_blocking(move || player.play_queue_paths(paths, start_path)).await
}

#[tauri::command]
pub async fn play_tracks(
    player: State<'_, Player>,
    tracks: Vec<Track>,
    start_path: Option<String>,
) -> AppResult<PlayerSnapshot> {
    let player = (*player).clone();
    run_blocking(move || player.play_tracks(tracks, start_path)).await
}

#[tauri::command]
pub async fn scan_tracks(path: String, fast: bool) -> AppResult<Vec<Track>> {
    run_blocking(move || {
        if fast {
            crate::player::scan::collect_tracks_fast(Path::new(&path))
        } else {
            crate::player::scan::collect_tracks(Path::new(&path))
        }
    })
    .await
}

#[tauri::command]
pub async fn scan_playlist(
    store: State<'_, Store>,
    id: String,
    fast: bool,
) -> AppResult<Vec<Track>> {
    let store = (*store).clone();
    run_blocking(move || {
        crate::playlists::sync(&store);
        let playlists = crate::playlists::list(&store);
        let playlist = playlists
            .iter()
            .find(|playlist| playlist.id == id)
            .ok_or_else(|| AppError::msg("playlist not found"))?;
        Ok(crate::player::scan::collect_many(
            &crate::playlists::flatten_paths(playlist),
            fast,
        ))
    })
    .await
}

#[tauri::command]
pub fn list_library_roots(store: State<Store>) -> Vec<String> {
    crate::library::list(&store)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChange {
    pub roots: Vec<String>,
    pub path: String,
}

#[tauri::command]
pub fn add_library_root(store: State<Store>, path: String) -> AppResult<LibraryChange> {
    let (roots, path) = crate::library::add(&store, path)?;
    Ok(LibraryChange { roots, path })
}

#[tauri::command]
pub fn remove_library_root(store: State<Store>, path: String) -> AppResult<Vec<String>> {
    crate::library::remove(&store, path)
}

#[tauri::command]
pub fn set_volume(player: State<Player>, volume: f64) -> AppResult<PlayerSnapshot> {
    player.set_volume(volume)
}

#[tauri::command]
pub fn set_muted(player: State<Player>, muted: bool) -> AppResult<PlayerSnapshot> {
    player.set_muted(muted)
}

#[tauri::command]
pub fn set_repeat(player: State<Player>, repeat: RepeatMode) -> AppResult<PlayerSnapshot> {
    player.set_repeat(repeat)
}

#[tauri::command]
pub fn set_shuffle(player: State<Player>, shuffle: bool) -> AppResult<PlayerSnapshot> {
    player.set_shuffle(shuffle)
}

#[tauri::command]
pub fn set_replaygain(player: State<Player>, enabled: bool) -> AppResult<PlayerSnapshot> {
    player.set_replaygain(enabled)
}

#[tauri::command]
pub fn set_gapless(player: State<Player>, enabled: bool) -> AppResult<PlayerSnapshot> {
    player.set_gapless(enabled)
}

#[tauri::command]
pub fn set_speed(player: State<Player>, speed: f64) -> AppResult<PlayerSnapshot> {
    player.set_speed(speed)
}

#[tauri::command]
pub fn set_eq(player: State<Player>, eq: EqUpdate) -> AppResult<PlayerSnapshot> {
    player.set_eq(eq)
}

#[tauri::command]
pub fn save_custom_eq(player: State<Player>, preset: EqUserPreset) -> AppResult<PlayerSnapshot> {
    player.save_custom_eq(preset)
}

#[tauri::command]
pub fn delete_custom_eq(player: State<Player>, id: String) -> AppResult<PlayerSnapshot> {
    player.delete_custom_eq(id)
}

#[tauri::command]
pub fn import_parametric_eq(player: State<Player>, text: String) -> AppResult<PlayerSnapshot> {
    player.import_parametric_eq(text)
}

#[tauri::command]
pub async fn read_tags(path: String) -> AppResult<TagDoc> {
    run_blocking(move || crate::tags::read_tags(&path)).await
}

#[tauri::command]
pub async fn write_tags(path: String, fields: TagFields) -> AppResult<TagDoc> {
    run_blocking(move || crate::tags::write_tags(&path, fields)).await
}

#[tauri::command]
pub async fn batch_write(
    paths: Vec<String>,
    fields: TagFields,
    apply: Vec<String>,
) -> AppResult<BatchResult> {
    run_blocking(move || crate::tags::batch_write(paths, fields, apply)).await
}

#[tauri::command]
pub async fn list_audio_paths(path: String) -> AppResult<Vec<String>> {
    run_blocking(move || crate::tags::list_audio_paths(&path)).await
}

#[tauri::command]
pub async fn add_picture(path: String, image_path: String, kind: String) -> AppResult<TagDoc> {
    run_blocking(move || crate::tags::add_picture_from_path(&path, &image_path, kind)).await
}

#[tauri::command]
pub async fn remove_picture(path: String, index: usize) -> AppResult<TagDoc> {
    run_blocking(move || crate::tags::remove_picture(&path, index)).await
}

#[tauri::command]
pub async fn export_picture(path: String, index: usize, dest: String) -> AppResult<()> {
    run_blocking(move || crate::tags::export_picture(&path, index, &dest)).await
}

#[tauri::command]
pub async fn add_custom_field(path: String, key: String, value: String) -> AppResult<TagDoc> {
    run_blocking(move || crate::tags::add_custom_field(&path, key, value)).await
}

#[tauri::command]
pub async fn remove_custom_field(path: String, key: String) -> AppResult<TagDoc> {
    run_blocking(move || crate::tags::remove_custom_field(&path, key)).await
}

#[tauri::command]
pub async fn refresh_metadata(
    player: State<'_, Player>,
    paths: Vec<String>,
) -> AppResult<Vec<Track>> {
    let player = (*player).clone();
    run_blocking(move || Ok(player.refresh_metadata(&paths))).await
}

#[tauri::command]
pub async fn cover_art(path: String) -> AppResult<Option<CoverArt>> {
    run_blocking(move || crate::tags::cover_for(&path)).await
}

#[tauri::command]
pub async fn picture_preview(path: String, index: usize) -> AppResult<Option<CoverArt>> {
    run_blocking(move || crate::tags::picture_preview(&path, index)).await
}

#[tauri::command]
pub async fn cover_thumb(path: String) -> AppResult<Option<CoverArt>> {
    run_blocking(move || crate::tags::cover_thumb(&path)).await
}

#[tauri::command]
pub fn list_playlists(store: State<Store>) -> Vec<Playlist> {
    crate::playlists::list(&store)
}

#[tauri::command]
pub fn create_playlist(store: State<Store>, name: String) -> AppResult<Vec<Playlist>> {
    crate::playlists::create(&store, name)
}

#[tauri::command]
pub fn rename_playlist(store: State<Store>, id: String, name: String) -> AppResult<Vec<Playlist>> {
    crate::playlists::rename(&store, id, name)
}

#[tauri::command]
pub fn delete_playlist(store: State<Store>, id: String) -> AppResult<Vec<Playlist>> {
    crate::playlists::delete(&store, id)
}

#[tauri::command]
pub fn add_to_playlist(
    store: State<Store>,
    id: String,
    paths: Vec<String>,
) -> AppResult<Vec<Playlist>> {
    crate::playlists::add_paths(&store, id, paths)
}

#[tauri::command]
pub fn remove_from_playlist(
    store: State<Store>,
    id: String,
    path: String,
) -> AppResult<Vec<Playlist>> {
    crate::playlists::remove_item(&store, id, path)
}

#[tauri::command]
pub async fn set_playlist_cover(
    store: State<'_, Store>,
    id: String,
    path: String,
) -> AppResult<Vec<Playlist>> {
    let store = (*store).clone();
    run_blocking(move || crate::playlists::set_cover_from_path(&store, id, path)).await
}

#[tauri::command]
pub fn clear_playlist_cover(store: State<Store>, id: String) -> AppResult<Vec<Playlist>> {
    crate::playlists::clear_cover(&store, id)
}

#[tauri::command]
pub async fn playlist_cover(store: State<'_, Store>, id: String) -> AppResult<Option<CoverArt>> {
    let store = (*store).clone();
    run_blocking(move || crate::playlists::cover(&store, &id)).await
}

#[tauri::command]
pub async fn set_artist_image(store: State<'_, Store>, key: String, path: String) -> AppResult<()> {
    let store = (*store).clone();
    run_blocking(move || crate::playlists::set_artist_image(&store, &key, &path)).await
}

#[tauri::command]
pub async fn artist_image(store: State<'_, Store>, key: String) -> AppResult<Option<CoverArt>> {
    let store = (*store).clone();
    run_blocking(move || crate::playlists::artist_image(&store, &key)).await
}

#[tauri::command]
pub fn list_missing(store: State<Store>) -> Vec<crate::relink::MissingItem> {
    crate::relink::list_missing(&store)
}

#[tauri::command]
pub fn relink_missing(
    store: State<Store>,
    scope: String,
    id: String,
    path: String,
    new_path: String,
) -> AppResult<()> {
    crate::relink::relink(&store, &scope, &id, &path, &new_path)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    pub theme: String,
    pub accent: String,
    pub custom_themes: Vec<CustomTheme>,
    #[serde(default)]
    pub minimize_movement: bool,
    #[serde(default)]
    pub visualizer: bool,
    #[serde(default = "crate::persist::default_viz_main")]
    pub visualizer_main: String,
    #[serde(default = "crate::persist::default_viz_border")]
    pub visualizer_border: String,
    #[serde(default = "crate::persist::default_viz_glow")]
    pub visualizer_glow: String,
}

fn appearance_from(store: &Store) -> Appearance {
    let data = store.snapshot();
    Appearance {
        theme: data.theme,
        accent: data.accent,
        custom_themes: data.custom_themes,
        minimize_movement: data.minimize_movement,
        visualizer: data.visualizer,
        visualizer_main: data.visualizer_main,
        visualizer_border: data.visualizer_border,
        visualizer_glow: data.visualizer_glow,
    }
}

#[tauri::command]
pub fn get_appearance(store: State<Store>) -> Appearance {
    appearance_from(&store)
}

#[tauri::command]
pub fn set_appearance(store: State<Store>, theme: String, accent: String) -> Appearance {
    store.update(|data| {
        data.theme = theme;
        data.accent = accent;
    });
    appearance_from(&store)
}

#[tauri::command]
pub fn set_minimize_movement(store: State<Store>, enabled: bool) -> Appearance {
    store.update(|data| data.minimize_movement = enabled);
    appearance_from(&store)
}

#[tauri::command]
pub fn set_visualizer(
    store: State<Store>,
    player: State<Player>,
    enabled: bool,
    main: String,
    border: String,
    glow: String,
) -> Appearance {
    let main = crate::persist::hex_color(&main, &store.snapshot().visualizer_main);
    let border = crate::persist::hex_color(&border, &store.snapshot().visualizer_border);
    let glow = crate::persist::hex_color(&glow, &store.snapshot().visualizer_glow);
    store.update(|data| {
        data.visualizer = enabled;
        data.visualizer_main = main;
        data.visualizer_border = border;
        data.visualizer_glow = glow;
    });
    player.set_viz_enabled(enabled);
    appearance_from(&store)
}

#[tauri::command]
pub fn save_custom_theme(store: State<Store>, theme: CustomTheme) -> Appearance {
    store.update(|data| {
        if let Some(existing) = data
            .custom_themes
            .iter_mut()
            .find(|item| item.id == theme.id)
        {
            *existing = theme.clone();
        } else {
            data.custom_themes.push(theme.clone());
        }
        data.theme = theme.id;
    });
    appearance_from(&store)
}

#[tauri::command]
pub fn delete_custom_theme(store: State<Store>, id: String) -> Appearance {
    store.update(|data| {
        data.custom_themes.retain(|item| item.id != id);
        if data.theme == id {
            data.theme = "dusk".into();
        }
    });
    appearance_from(&store)
}

#[tauri::command]
pub fn remote_status(remote: State<crate::remote::Remote>) -> crate::remote::RemoteStatus {
    remote.status()
}

#[tauri::command]
pub fn remote_start(
    remote: State<crate::remote::Remote>,
) -> AppResult<crate::remote::RemoteStatus> {
    remote.start()
}

#[tauri::command]
pub async fn remote_stop(
    remote: State<'_, crate::remote::Remote>,
) -> AppResult<crate::remote::RemoteStatus> {
    // Async commands run off the main thread; joining the remote's threads
    // here does not freeze the window.
    Ok(remote.stop())
}

#[tauri::command]
pub async fn search_stream(query: String) -> AppResult<MediaHit> {
    run_blocking(move || crate::search::search_stream(&query)).await
}

#[tauri::command]
pub async fn search_media(query: String) -> AppResult<Vec<MediaHit>> {
    run_blocking(move || crate::search::search_media(&query)).await
}

#[tauri::command]
pub async fn search_covers(query: String) -> AppResult<Vec<MediaHit>> {
    run_blocking(move || crate::search::search_covers(&query)).await
}

#[tauri::command]
pub async fn add_cover_from_url(path: String, url: String, kind: String) -> AppResult<TagDoc> {
    run_blocking(move || {
        let data = crate::search::fetch_cover_image(&url)?;
        crate::tags::add_picture(&path, data, "image/jpeg".into(), kind)
    })
    .await
}

#[tauri::command]
pub async fn play_media(
    player: State<'_, Player>,
    title: String,
    url: String,
    page_url: Option<String>,
) -> AppResult<PlayerSnapshot> {
    let player = (*player).clone();
    let gen = player.bump_play_generation();
    let watcher = player.clone();
    run_blocking(move || {
        if crate::search::find_tool("ffmpeg").is_none() {
            return Err(AppError::msg(
                "ffmpeg is required to play search results. Install it with: sudo apt install ffmpeg",
            ));
        }
        let cancel: Arc<dyn Fn() -> bool + Send + Sync> =
            Arc::new(move || gen != watcher.play_generation());
        crate::search::with_cancel(cancel, || {
            let source = crate::search::play_source(&url, page_url.as_deref()).to_string();
            let path = match crate::search::cache_media(&source) {
                Ok(path) => path,
                Err(_) if gen != player.play_generation() => return Ok(player.snapshot()),
                Err(error) => return Err(error),
            };
            if gen != player.play_generation() {
                // A newer play started while this one was downloading.
                return Ok(player.snapshot());
            }
            let track = crate::search::track_for(&path, &title);
            player.play_tracks(vec![track], None)
        })
    })
    .await
}

#[tauri::command]
pub async fn save_media(url: String, dest: String, page_url: Option<String>) -> AppResult<String> {
    run_blocking(move || {
        let source = crate::search::play_source(&url, page_url.as_deref()).to_string();
        let path = crate::search::save_media(&source, Path::new(&dest))?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
}

async fn run_blocking<T: Send + 'static>(
    work: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| AppError::msg("background task failed"))?
}

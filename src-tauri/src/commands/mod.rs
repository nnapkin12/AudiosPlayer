use std::path::Path;

use tauri::State;

use crate::eq::{EqUpdate, EqUserPreset};
use crate::error::{AppError, AppResult};
use crate::persist::{CustomTheme, Playlist, Store};
use crate::player::queue::RepeatMode;
use crate::player::scan::Track;
use crate::player::{Player, PlayerSnapshot};
use crate::search::MediaHit;
use crate::tags::{CoverArt, TagDoc, TagFields};

#[tauri::command]
pub fn player_state(player: State<Player>) -> PlayerSnapshot {
    player.snapshot_ui()
}

#[tauri::command]
pub fn open_path(player: State<Player>, path: String) -> AppResult<PlayerSnapshot> {
    player.open_path(&path)
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
pub fn play_path(player: State<Player>, path: String) -> AppResult<PlayerSnapshot> {
    player.play_path(&path)
}

#[tauri::command]
pub fn play_playlist(
    player: State<Player>,
    store: State<Store>,
    id: String,
    start_path: Option<String>,
) -> AppResult<PlayerSnapshot> {
    crate::playlists::sync(&store);
    let playlists = crate::playlists::list(&store);
    let playlist = playlists
        .iter()
        .find(|playlist| playlist.id == id)
        .ok_or_else(|| crate::error::AppError::msg("playlist not found"))?;
    player.play_queue_paths(crate::playlists::flatten_paths(playlist), start_path)
}

#[tauri::command]
pub fn play_queue_paths(
    player: State<Player>,
    paths: Vec<String>,
    start_path: Option<String>,
) -> AppResult<PlayerSnapshot> {
    player.play_queue_paths(paths, start_path)
}

#[tauri::command]
pub fn play_tracks(
    player: State<Player>,
    tracks: Vec<Track>,
    start_path: Option<String>,
) -> AppResult<PlayerSnapshot> {
    player.play_tracks(tracks, start_path)
}

#[tauri::command]
pub fn scan_tracks(path: String, fast: bool) -> AppResult<Vec<Track>> {
    if fast {
        crate::player::scan::collect_tracks_fast(Path::new(&path))
    } else {
        crate::player::scan::collect_tracks(Path::new(&path))
    }
}

#[tauri::command]
pub fn scan_playlist(store: State<Store>, id: String, fast: bool) -> AppResult<Vec<Track>> {
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
pub fn read_tags(path: String) -> AppResult<TagDoc> {
    crate::tags::read_tags(&path)
}

#[tauri::command]
pub fn write_tags(path: String, fields: TagFields) -> AppResult<TagDoc> {
    crate::tags::write_tags(&path, fields)
}

#[tauri::command]
pub fn batch_write(paths: Vec<String>, fields: TagFields, apply: Vec<String>) -> AppResult<usize> {
    crate::tags::batch_write(paths, fields, apply)
}

#[tauri::command]
pub fn list_audio_paths(path: String) -> AppResult<Vec<String>> {
    crate::tags::list_audio_paths(&path)
}

#[tauri::command]
pub fn add_picture(path: String, data: Vec<u8>, mime: String, kind: String) -> AppResult<TagDoc> {
    crate::tags::add_picture(&path, data, mime, kind)
}

#[tauri::command]
pub fn remove_picture(path: String, index: usize) -> AppResult<TagDoc> {
    crate::tags::remove_picture(&path, index)
}

#[tauri::command]
pub fn export_picture(path: String, index: usize, dest: String) -> AppResult<()> {
    crate::tags::export_picture(&path, index, &dest)
}

#[tauri::command]
pub fn add_custom_field(path: String, key: String, value: String) -> AppResult<TagDoc> {
    crate::tags::add_custom_field(&path, key, value)
}

#[tauri::command]
pub fn remove_custom_field(path: String, key: String) -> AppResult<TagDoc> {
    crate::tags::remove_custom_field(&path, key)
}

#[tauri::command]
pub fn refresh_metadata(player: State<Player>, paths: Vec<String>) -> Vec<Track> {
    player.refresh_metadata(&paths)
}

#[tauri::command]
pub fn cover_art(path: String) -> AppResult<Option<CoverArt>> {
    crate::tags::cover_for(&path)
}

#[tauri::command]
pub fn cover_thumb(path: String) -> AppResult<Option<CoverArt>> {
    crate::tags::cover_thumb(&path)
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
pub fn set_playlist_cover(
    store: State<Store>,
    id: String,
    path: String,
) -> AppResult<Vec<Playlist>> {
    crate::playlists::set_cover_from_path(&store, id, path)
}

#[tauri::command]
pub fn clear_playlist_cover(store: State<Store>, id: String) -> AppResult<Vec<Playlist>> {
    crate::playlists::clear_cover(&store, id)
}

#[tauri::command]
pub fn playlist_cover(store: State<Store>, id: String) -> AppResult<Option<CoverArt>> {
    crate::playlists::cover(&store, &id)
}

#[tauri::command]
pub fn set_artist_image(store: State<Store>, key: String, path: String) -> AppResult<()> {
    crate::playlists::set_artist_image(&store, &key, &path)
}

#[tauri::command]
pub fn artist_image(store: State<Store>, key: String) -> AppResult<Option<CoverArt>> {
    crate::playlists::artist_image(&store, &key)
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
}

fn appearance_from(store: &Store) -> Appearance {
    let data = store.snapshot();
    Appearance {
        theme: data.theme,
        accent: data.accent,
        custom_themes: data.custom_themes,
        minimize_movement: data.minimize_movement,
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
pub fn remote_stop(remote: State<crate::remote::Remote>) -> crate::remote::RemoteStatus {
    remote.stop()
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
    run_blocking(move || {
        let source = crate::search::play_source(&url, page_url.as_deref()).to_string();
        let path = crate::search::cache_media(&source)?;
        let track = crate::search::track_for(&path, &title);
        player.play_tracks(vec![track], None)
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

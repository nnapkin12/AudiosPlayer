mod commands;
#[cfg(target_os = "linux")]
mod desktop;
mod eq;
mod error;
mod library;
#[cfg(target_os = "linux")]
mod media;
mod mp4_recover;
mod persist;
mod player;
mod playlists;
mod relink;
mod remote;
mod search;
mod tags;
mod watch;

use persist::Store;
use player::Player;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let persist = Store::load();
            playlists::sync(&persist);
            library::prune_missing(&persist);
            app.manage(persist.clone());
            let player = Player::new(app.handle().clone(), persist.clone());
            #[cfg(target_os = "linux")]
            media::start(player.clone());
            let remote = remote::Remote::new(app.handle().clone(), player.clone(), persist.clone());
            app.manage(player);
            app.manage(remote);
            watch::spawn(app.handle().clone(), persist);
            #[cfg(target_os = "linux")]
            desktop::install();
            apply_window_icon(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::player_state,
            commands::open_path,
            commands::play,
            commands::pause,
            commands::toggle,
            commands::stop,
            commands::next_track,
            commands::previous_track,
            commands::seek,
            commands::play_index,
            commands::play_path,
            commands::play_queue_paths,
            commands::play_tracks,
            commands::play_playlist,
            commands::scan_tracks,
            commands::scan_playlist,
            commands::list_library_roots,
            commands::add_library_root,
            commands::remove_library_root,
            commands::set_volume,
            commands::set_muted,
            commands::set_repeat,
            commands::set_shuffle,
            commands::set_replaygain,
            commands::set_gapless,
            commands::set_speed,
            commands::set_eq,
            commands::save_custom_eq,
            commands::delete_custom_eq,
            commands::import_parametric_eq,
            commands::read_tags,
            commands::write_tags,
            commands::batch_write,
            commands::list_audio_paths,
            commands::add_picture,
            commands::remove_picture,
            commands::export_picture,
            commands::add_custom_field,
            commands::remove_custom_field,
            commands::cover_art,
            commands::cover_thumb,
            commands::refresh_metadata,
            commands::list_playlists,
            commands::create_playlist,
            commands::rename_playlist,
            commands::delete_playlist,
            commands::add_to_playlist,
            commands::remove_from_playlist,
            commands::set_playlist_cover,
            commands::clear_playlist_cover,
            commands::playlist_cover,
            commands::set_artist_image,
            commands::artist_image,
            commands::list_missing,
            commands::relink_missing,
            commands::get_appearance,
            commands::set_appearance,
            commands::set_minimize_movement,
            commands::save_custom_theme,
            commands::delete_custom_theme,
            commands::remote_status,
            commands::remote_start,
            commands::remote_stop,
            commands::search_stream,
            commands::search_media,
            commands::search_covers,
            commands::add_cover_from_url,
            commands::play_media,
            commands::save_media,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Audios!");
}

fn apply_window_icon(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Some(icon) = app.default_window_icon() {
        let _ = window.set_icon(icon.clone());
    }
}

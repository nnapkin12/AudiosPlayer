use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{imageops::FilterType, DynamicImage, Rgb, RgbImage};

use crate::error::{AppError, AppResult};
use crate::persist::{Playlist, PlaylistItem, Store};
use crate::tags::CoverArt;

pub fn list(store: &Store) -> Vec<Playlist> {
    store
        .snapshot()
        .playlists
        .into_iter()
        .map(|playlist| with_cover_flag(store, playlist))
        .collect()
}

fn next_default_name(playlists: &[Playlist]) -> String {
    let mut used = std::collections::HashSet::new();
    for playlist in playlists {
        if let Some(number) = default_number(&playlist.name) {
            used.insert(number);
        }
    }
    let mut number = 1u32;
    loop {
        if !used.contains(&number) {
            break;
        }
        number = match number.checked_add(1) {
            Some(next) => next,
            None => break,
        };
    }
    format!("New playlist #{number}")
}

/// `New playlist #N` and the older `Playlist N` occupy the same number.
fn default_number(name: &str) -> Option<u32> {
    let rest = name
        .strip_prefix("New playlist #")
        .or_else(|| name.strip_prefix("Playlist "))?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok().filter(|number| *number > 0)
}

fn with_cover_flag(store: &Store, mut playlist: Playlist) -> Playlist {
    playlist.has_cover = cover_file(store, &playlist.id).is_some_and(|path| path.is_file());
    playlist
}

pub fn create(store: &Store, name: String) -> AppResult<Vec<Playlist>> {
    let name = name.trim();
    let name = if name.is_empty() {
        next_default_name(&store.snapshot().playlists)
    } else {
        name.to_string()
    };
    store.update(|data| {
        data.playlists.push(Playlist {
            id: format!("pl-{:016x}", fastrand::u64(..)),
            name,
            items: Vec::new(),
            has_cover: false,
        });
    });
    Ok(list(store))
}

pub fn rename(store: &Store, id: String, name: String) -> AppResult<Vec<Playlist>> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::msg("playlist name cannot be empty"));
    }
    let mut found = false;
    store.update(|data| {
        if let Some(playlist) = data.playlists.iter_mut().find(|playlist| playlist.id == id) {
            playlist.name = name.to_string();
            found = true;
        }
    });
    if !found {
        return Err(AppError::msg("playlist not found"));
    }
    Ok(list(store))
}

pub fn delete(store: &Store, id: String) -> AppResult<Vec<Playlist>> {
    store.update(|data| {
        data.playlists.retain(|playlist| playlist.id != id);
    });
    if let Some(path) = cover_file(store, &id) {
        let _ = std::fs::remove_file(path);
    }
    drop_auto_cover(store, &id);
    Ok(list(store))
}

pub fn set_cover(store: &Store, id: String, data: Vec<u8>) -> AppResult<Vec<Playlist>> {
    playlist_exists(store, &id)?;
    let path = cover_file(store, &id).ok_or_else(|| AppError::msg("bad playlist id"))?;
    write_jpeg(&path, &encode_cover_jpeg(&data)?)?;
    Ok(list(store))
}

pub fn set_cover_from_path(store: &Store, id: String, path: String) -> AppResult<Vec<Playlist>> {
    let data = std::fs::read(&path).map_err(|_| AppError::msg("could not read that picture"))?;
    set_cover(store, id, data)
}

pub fn clear_cover(store: &Store, id: String) -> AppResult<Vec<Playlist>> {
    playlist_exists(store, &id)?;
    if let Some(path) = cover_file(store, &id) {
        let _ = std::fs::remove_file(path);
    }
    drop_auto_cover(store, &id);
    Ok(list(store))
}

pub fn set_artist_image(store: &Store, key: &str, source: &str) -> AppResult<()> {
    let data = std::fs::read(source)?;
    let jpeg = encode_cover_jpeg(&data)?;
    let path = artist_image_file(store, key)?;
    write_jpeg(&path, &jpeg)
}

pub fn artist_image(store: &Store, key: &str) -> AppResult<Option<CoverArt>> {
    Ok(jpeg_from_file(Some(artist_image_file(store, key)?)))
}

fn artist_image_file(store: &Store, key: &str) -> AppResult<PathBuf> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(AppError::msg("missing artist"));
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    trimmed.to_lowercase().hash(&mut hasher);
    Ok(store
        .config_dir()
        .join("artist-images")
        .join(format!("{:016x}.jpg", hasher.finish())))
}

pub fn cover(store: &Store, id: &str) -> AppResult<Option<CoverArt>> {
    if let Some(art) = jpeg_from_file(cover_file(store, id)) {
        return Ok(Some(art));
    }
    ensure_auto_cover(store, id);
    Ok(jpeg_from_file(auto_cover_file(store, id)))
}

fn playlist_exists(store: &Store, id: &str) -> AppResult<()> {
    if store
        .snapshot()
        .playlists
        .iter()
        .any(|playlist| playlist.id == id)
    {
        Ok(())
    } else {
        Err(AppError::msg("playlist not found"))
    }
}

fn cover_file(store: &Store, id: &str) -> Option<PathBuf> {
    named_cover(store, id, "jpg")
}

fn auto_cover_file(store: &Store, id: &str) -> Option<PathBuf> {
    named_cover(store, id, "auto.jpg")
}

fn named_cover(store: &Store, id: &str, suffix: &str) -> Option<PathBuf> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return None;
    }
    Some(
        store
            .config_dir()
            .join("playlist-covers")
            .join(format!("{id}.{suffix}")),
    )
}

fn drop_auto_cover(store: &Store, id: &str) {
    if let Some(path) = auto_cover_file(store, id) {
        let _ = std::fs::remove_file(path);
    }
}

fn jpeg_from_file(path: Option<PathBuf>) -> Option<CoverArt> {
    let path = path?;
    let bytes = std::fs::read(&path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(CoverArt {
        mime: "image/jpeg".into(),
        data_base64: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
    })
}

fn write_jpeg(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn encode_cover_jpeg(data: &[u8]) -> AppResult<Vec<u8>> {
    if data.is_empty() {
        return Err(AppError::msg("picture data was empty"));
    }
    rgb_to_jpeg(&downscale_rgb(decode_image(data)?, 1600), 88)
}

fn decode_image(data: &[u8]) -> AppResult<RgbImage> {
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|_| AppError::msg("that image could not be read"))?;
    reader
        .decode()
        .map(|image| image.to_rgb8())
        .map_err(|_| AppError::msg("that image could not be read"))
}

fn downscale_rgb(image: RgbImage, max_edge: u32) -> RgbImage {
    let (width, height) = image.dimensions();
    if width <= max_edge && height <= max_edge {
        return image;
    }
    DynamicImage::ImageRgb8(image)
        .resize(max_edge, max_edge, FilterType::Triangle)
        .to_rgb8()
}

fn rgb_to_jpeg(image: &RgbImage, quality: u8) -> AppResult<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|_| AppError::msg("could not save playlist picture"))?;
    Ok(out.into_inner())
}

fn ensure_auto_cover(store: &Store, id: &str) {
    let Some(path) = auto_cover_file(store, id) else {
        return;
    };
    if path.is_file() {
        return;
    }
    let Some(jpeg) = build_auto_cover(store, id) else {
        return;
    };
    let _ = write_jpeg(&path, &jpeg);
}

fn build_auto_cover(store: &Store, id: &str) -> Option<Vec<u8>> {
    let playlist = store
        .snapshot()
        .playlists
        .into_iter()
        .find(|playlist| playlist.id == id)?;
    let mut paths: Vec<String> = crate::player::scan::collect_many(&flatten_paths(&playlist), true)
        .into_iter()
        .map(|track| track.path)
        .collect();
    if paths.is_empty() {
        return None;
    }
    let mut rng = fastrand::Rng::with_seed(seed_from_id(id));
    rng.shuffle(&mut paths);
    let mut tiles = Vec::new();
    for path in paths.into_iter().take(48) {
        if let Some(image) = rgb_from_audio(&path) {
            tiles.push(image);
            if tiles.len() == 4 {
                break;
            }
        }
    }
    if tiles.is_empty() {
        return None;
    }
    rgb_to_jpeg(&compose_mosaic(&tiles), 86).ok()
}

fn seed_from_id(id: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

fn rgb_from_audio(path: &str) -> Option<RgbImage> {
    let cover = crate::tags::cover_for(path).ok()??;
    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        cover.data_base64.as_bytes(),
    )
    .ok()?;
    decode_image(&bytes).ok()
}

fn compose_mosaic(tiles: &[RgbImage]) -> RgbImage {
    const CELL: u32 = 512;
    if tiles.len() == 1 {
        return DynamicImage::ImageRgb8(tiles[0].clone())
            .resize_to_fill(CELL * 2, CELL * 2, FilterType::Triangle)
            .to_rgb8();
    }
    let mut canvas = RgbImage::from_pixel(CELL * 2, CELL * 2, Rgb([36, 36, 36]));
    let slots = [(0, 0), (CELL, 0), (0, CELL), (CELL, CELL)];
    for (tile, (x, y)) in tiles.iter().take(4).zip(slots) {
        let filled = DynamicImage::ImageRgb8(tile.clone())
            .resize_to_fill(CELL, CELL, FilterType::Triangle)
            .to_rgb8();
        image::imageops::replace(&mut canvas, &filled, i64::from(x), i64::from(y));
    }
    canvas
}

pub fn add_paths(store: &Store, id: String, paths: Vec<String>) -> AppResult<Vec<Playlist>> {
    if paths.is_empty() {
        return Err(AppError::msg("Nothing to add"));
    }
    let mut found = false;
    let mut added = false;
    store.update(|data| {
        let Some(playlist) = data.playlists.iter_mut().find(|playlist| playlist.id == id) else {
            return;
        };
        found = true;
        for path in &paths {
            if link_path(playlist, path) {
                added = true;
            }
        }
    });
    if !found {
        return Err(AppError::msg("playlist not found"));
    }
    if !added {
        return Err(AppError::msg("No songs here"));
    }
    drop_auto_cover(store, &id);
    Ok(list(store))
}

/// Link song lists back to their folders, then drop entries whose parent
/// folder is still on disk but the file or folder itself is gone. A missing
/// parent means the drive may be offline, so those entries stay.
pub fn sync(store: &Store) -> bool {
    let mut playlists = store.snapshot().playlists;
    crate::relink::retarget_playlists(&mut playlists, &store.snapshot().library_roots);
    let mut changed = Vec::new();
    for playlist in &mut playlists {
        let before = playlist.items.clone();
        playlist.items.retain(keep_item);
        if playlist.items != before {
            changed.push(playlist.id.clone());
        }
    }
    if changed.is_empty() {
        return false;
    }
    store.update(|data| {
        data.playlists = playlists;
    });
    for id in changed {
        drop_auto_cover(store, &id);
    }
    true
}

pub fn remove_item(store: &Store, id: String, path: String) -> AppResult<Vec<Playlist>> {
    let mut found = false;
    store.update(|data| {
        if let Some(playlist) = data.playlists.iter_mut().find(|playlist| playlist.id == id) {
            found = true;
            playlist.items = exclude_song(&playlist.items, &path);
        }
    });
    if !found {
        return Err(AppError::msg("playlist not found"));
    }
    drop_auto_cover(store, &id);
    Ok(list(store))
}

fn link_path(playlist: &mut Playlist, path: &str) -> bool {
    let root = Path::new(path);
    if root.is_dir() {
        playlist.items.retain(|item| {
            if item.kind == "dir" && same_path(&item.path, path) {
                return true;
            }
            !inside(&item.path, path)
        });
        if !playlist
            .items
            .iter()
            .any(|item| item.kind == "dir" && same_path(&item.path, path))
        {
            playlist.items.push(PlaylistItem {
                path: path.to_string(),
                kind: "dir".into(),
            });
        }
        return true;
    }
    if root.exists() && !crate::player::scan::is_audio_path(root) {
        return false;
    }
    playlist
        .items
        .retain(|item| !(item.kind == "exclude" && same_path(&item.path, path)));
    if playlist
        .items
        .iter()
        .any(|item| item.kind == "dir" && inside(path, &item.path))
    {
        return true;
    }
    if playlist
        .items
        .iter()
        .any(|item| item.kind != "exclude" && same_path(&item.path, path))
    {
        return true;
    }
    playlist.items.push(PlaylistItem {
        path: path.to_string(),
        kind: "file".into(),
    });
    true
}

fn exclude_song(items: &[PlaylistItem], path: &str) -> Vec<PlaylistItem> {
    let mut next = Vec::new();
    let mut covered = false;
    for item in items {
        if item.kind == "exclude" {
            next.push(item.clone());
            continue;
        }
        if item.kind != "dir" && same_path(&item.path, path) {
            continue;
        }
        if item.kind == "dir" && inside(path, &item.path) {
            covered = true;
        }
        next.push(item.clone());
    }
    if covered
        && !next
            .iter()
            .any(|item| item.kind == "exclude" && same_path(&item.path, path))
    {
        next.push(PlaylistItem {
            path: path.to_string(),
            kind: "exclude".into(),
        });
    }
    next
}

fn keep_item(item: &PlaylistItem) -> bool {
    let path = Path::new(&item.path);
    if item.kind == "exclude" {
        return path.is_file() || !parent_is_dir(path);
    }
    true
}

fn parent_is_dir(path: &Path) -> bool {
    path.parent().is_some_and(|parent| parent.is_dir())
}

fn same_path(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    let left = Path::new(left)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(left));
    let right = Path::new(right)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(right));
    left == right
}

fn inside(path: &str, dir: &str) -> bool {
    let path = Path::new(path);
    let dir = Path::new(dir);
    path != dir && path.starts_with(dir)
}

fn push_unique(songs: &mut Vec<String>, path: String) {
    if !songs.iter().any(|song| song == &path) {
        songs.push(path);
    }
}

pub fn flatten_paths(playlist: &Playlist) -> Vec<String> {
    let excluded: std::collections::HashSet<&str> = playlist
        .items
        .iter()
        .filter(|item| item.kind == "exclude")
        .map(|item| item.path.as_str())
        .collect();
    let mut songs = Vec::new();
    for item in &playlist.items {
        if item.kind == "exclude" {
            continue;
        }
        let root = Path::new(&item.path);
        if item.kind == "dir" || root.is_dir() {
            if let Ok(files) = crate::player::scan::audio_paths(root) {
                for file in files {
                    let song = file.to_string_lossy().to_string();
                    if excluded.contains(song.as_str()) {
                        continue;
                    }
                    push_unique(&mut songs, song);
                }
            }
            continue;
        }
        if excluded.contains(item.path.as_str()) || !root.is_file() {
            continue;
        }
        push_unique(&mut songs, item.path.clone());
    }
    songs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::{PlaylistItem, Store};

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        (dir, store)
    }

    #[test]
    fn default_names_skip_used_numbers() {
        let (_dir, store) = store();
        create(&store, "Playlist 1".into()).unwrap();
        create(&store, "New playlist #2".into()).unwrap();
        let playlists = create(&store, "".into()).unwrap();
        assert_eq!(playlists[2].name, "New playlist #3");
        let id = playlists[0].id.clone();
        delete(&store, id).unwrap();
        let playlists = create(&store, "  ".into()).unwrap();
        assert_eq!(playlists.last().unwrap().name, "New playlist #1");
    }

    #[test]
    fn create_and_add() {
        let (_dir, store) = store();
        let playlists = create(&store, "Late".into()).unwrap();
        assert_eq!(playlists.len(), 1);
        assert!(!playlists[0].has_cover);
        let id = playlists[0].id.clone();
        let playlists = add_paths(&store, id, vec!["/a/song.mp3".into()]).unwrap();
        assert_eq!(playlists[0].items.len(), 1);
        assert_eq!(playlists[0].items[0].kind, "file");
    }

    #[test]
    fn add_folder_keeps_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(album.join("a.mp3"), []).unwrap();
        std::fs::write(album.join("b.mp3"), []).unwrap();
        std::fs::write(album.join("notes.txt"), []).unwrap();
        let playlists = create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        let playlists = add_paths(&store, id, vec![album.to_string_lossy().into()]).unwrap();
        assert_eq!(playlists[0].items.len(), 1);
        assert_eq!(playlists[0].items[0].kind, "dir");
        let songs = flatten_paths(&playlists[0]);
        assert_eq!(songs.len(), 2);
    }

    #[test]
    fn empty_folder_stays_linked() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let album = dir.path().join("empty");
        std::fs::create_dir_all(&album).unwrap();
        let playlists = create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        let playlists = add_paths(&store, id, vec![album.to_string_lossy().into()]).unwrap();
        assert_eq!(playlists[0].items.len(), 1);
        assert_eq!(playlists[0].items[0].kind, "dir");
        assert!(flatten_paths(&playlists[0]).is_empty());
    }

    #[test]
    fn linked_folder_follows_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        let kept = album.join("kept.mp3");
        std::fs::write(&kept, []).unwrap();
        let playlists = create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        add_paths(&store, id.clone(), vec![album.to_string_lossy().into()]).unwrap();
        let added = album.join("added.mp3");
        std::fs::write(&added, []).unwrap();
        assert_eq!(flatten_paths(&store.snapshot().playlists[0]).len(), 2);
        std::fs::remove_file(&kept).unwrap();
        let songs = flatten_paths(&store.snapshot().playlists[0]);
        assert_eq!(songs.len(), 1);
        assert!(songs[0].ends_with("added.mp3"));
        assert_eq!(store.snapshot().playlists[0].items[0].kind, "dir");
    }

    #[test]
    fn one_song_does_not_pull_the_rest_of_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        let skipped = album.join("skipped.mp3");
        let kept = album.join("kept.mp3");
        std::fs::write(&skipped, []).unwrap();
        std::fs::write(&kept, []).unwrap();
        let playlists = create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        let playlists = add_paths(&store, id, vec![kept.to_string_lossy().into()]).unwrap();
        assert_eq!(playlists[0].items.len(), 1);
        assert_eq!(playlists[0].items[0].kind, "file");
        let songs = flatten_paths(&playlists[0]);
        assert_eq!(songs.len(), 1);
        assert!(songs[0].ends_with("kept.mp3"));
        std::fs::write(album.join("later.mp3"), []).unwrap();
        let _ = sync(&store);
        let songs = flatten_paths(&store.snapshot().playlists[0]);
        assert_eq!(songs.len(), 1);
        assert!(songs[0].ends_with("kept.mp3"));
    }

    #[test]
    fn nested_folder_does_not_include_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let artist = dir.path().join("artist");
        let album = artist.join("album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::write(artist.join("loose.mp3"), []).unwrap();
        std::fs::write(album.join("one.mp3"), []).unwrap();
        std::fs::write(album.join("two.mp3"), []).unwrap();
        let playlists = create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        let playlists = add_paths(&store, id, vec![album.to_string_lossy().into()]).unwrap();
        assert_eq!(playlists[0].items[0].kind, "dir");
        let songs = flatten_paths(&playlists[0]);
        assert_eq!(songs.len(), 2);
        assert!(songs.iter().all(|song| song.contains("/album/")));
        assert!(songs.iter().all(|song| !song.ends_with("loose.mp3")));
    }

    #[test]
    fn remove_song_from_folder_keeps_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        let skipped = album.join("a.mp3");
        let kept = album.join("b.mp3");
        std::fs::write(&skipped, []).unwrap();
        std::fs::write(&kept, []).unwrap();
        let playlists = create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        store.update(|data| {
            data.playlists[0].items = vec![PlaylistItem {
                path: album.to_string_lossy().into(),
                kind: "dir".into(),
            }];
        });
        let playlists = remove_item(&store, id, skipped.to_string_lossy().into()).unwrap();
        assert_eq!(playlists[0].items.len(), 2);
        assert!(playlists[0].items.iter().any(|item| item.kind == "dir"));
        assert!(playlists[0]
            .items
            .iter()
            .any(|item| item.kind == "exclude" && item.path == skipped.to_string_lossy()));
        let songs = flatten_paths(&playlists[0]);
        assert_eq!(songs.len(), 1);
        assert!(songs[0].ends_with("b.mp3"));
        std::fs::write(album.join("c.mp3"), []).unwrap();
        assert_eq!(flatten_paths(&store.snapshot().playlists[0]).len(), 2);
    }

    #[test]
    fn sync_drops_deleted_file_and_keeps_offline_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_test(dir.path());
        let album = dir.path().join("album");
        std::fs::create_dir_all(&album).unwrap();
        let song = album.join("gone.mp3");
        std::fs::write(&song, []).unwrap();
        create(&store, "Late".into()).unwrap();
        store.update(|data| {
            data.playlists[0].items = vec![
                PlaylistItem {
                    path: song.to_string_lossy().into(),
                    kind: "file".into(),
                },
                PlaylistItem {
                    path: "/volume-offline/album/stay.mp3".into(),
                    kind: "file".into(),
                },
            ];
        });
        std::fs::remove_file(&song).unwrap();
        let _ = sync(&store);
        let items = store.snapshot().playlists[0].items.clone();
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|item| item.path.ends_with("gone.mp3")));
        assert!(items
            .iter()
            .any(|item| item.path.contains("volume-offline")));
    }

    #[test]
    fn encode_cover_jpeg_accepts_png() {
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            2,
            image::Rgb([10, 20, 30]),
        ));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let jpeg = encode_cover_jpeg(png.get_ref()).unwrap();
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn playlist_cover_round_trip() {
        let (_dir, store) = store();
        let playlists = create(&store, "Late".into()).unwrap();
        let id = playlists[0].id.clone();
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            2,
            image::Rgb([10, 20, 30]),
        ));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let playlists = set_cover(&store, id.clone(), png.into_inner()).unwrap();
        assert!(playlists[0].has_cover);
        assert!(cover(&store, &id).unwrap().is_some());
        let playlists = clear_cover(&store, id).unwrap();
        assert!(!playlists[0].has_cover);
    }

    #[test]
    fn mosaic_uses_up_to_four_tiles() {
        let red = RgbImage::from_pixel(8, 8, Rgb([200, 20, 20]));
        let mosaic = compose_mosaic(&[red.clone()]);
        assert_eq!(mosaic.dimensions(), (1024, 1024));
        let mosaic = compose_mosaic(&[
            red,
            RgbImage::from_pixel(8, 8, Rgb([20, 200, 20])),
            RgbImage::from_pixel(8, 8, Rgb([20, 20, 200])),
            RgbImage::from_pixel(8, 8, Rgb([200, 200, 20])),
        ]);
        assert_eq!(mosaic.get_pixel(10, 10), &Rgb([200, 20, 20]));
        assert_eq!(mosaic.get_pixel(520, 10), &Rgb([20, 200, 20]));
    }
}

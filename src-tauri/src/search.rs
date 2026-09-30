//! Search lists titles via yt-dlp `--flat-playlist`. Play then downloads a
//! local temp file, remuxes it for rodio, and deletes it when the track changes.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::player::scan::Track;

const YTDLP_BASE: &[&str] = &[
    "--ignore-config",
    "--no-warnings",
    "--retries",
    "5",
    "--socket-timeout",
    "15",
];

// Current YouTube often returns an empty format list for android/ios/web.
// mediaconnect still exposes a downloadable AAC stream.
const PLAYER_CLIENTS: &[&str] = &["youtube:player_client=mediaconnect"];
const INVIDIOUS_APIS: &[&str] = &[
    "https://invidious.f5.si",
    "https://inv.nadeko.net",
    "https://yewtu.be",
    "https://invidious.nerdvpn.de",
];
const PIPED_APIS: &[&str] = &[
    "https://pipedapi.kavin.rocks",
    "https://pipedapi.adminforge.de",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaHit {
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

pub fn search_stream(query: &str) -> AppResult<MediaHit> {
    let query = clean_query(query)?;
    let hit = resolve_hit(&query)?;
    let stream = extract_stream(&hit.url)?;
    Ok(MediaHit {
        title: hit.title,
        url: stream,
        page_url: Some(hit.url),
        thumbnail_url: hit.thumbnail_url,
        channel: hit.channel,
    })
}

pub fn search_media(query: &str) -> AppResult<Vec<MediaHit>> {
    let query = clean_query(query)?;
    if is_url(&query) {
        return flat_search(&query);
    }
    let mut hits = Vec::new();
    let mut last_error = None;
    for target in list_query_variants(&query) {
        match flat_search(&target) {
            Ok(found) if !found.is_empty() => {
                merge_hits(&mut hits, found);
                break;
            }
            Ok(_) => {}
            Err(error) => last_error = Some(error),
        }
    }
    for target in soundcloud_queries(&query) {
        match flat_search(&target) {
            Ok(found) => merge_hits(&mut hits, found),
            Err(error) => last_error = Some(error),
        }
    }
    if hits.is_empty() {
        return Err(last_error.unwrap_or_else(|| AppError::msg("no search results")));
    }
    Ok(hits)
}

/// Metadata-only cover search. Four thumbnails, no audio download.
pub fn search_covers(query: &str) -> AppResult<Vec<MediaHit>> {
    let query = clean_query(query)?;
    for target in cover_query_variants(&query) {
        let mut hits = flat_search(&target)?;
        hits.retain(|hit| hit.thumbnail_url.is_some());
        hits.truncate(4);
        if !hits.is_empty() {
            return Ok(hits);
        }
    }
    Err(AppError::msg("no artwork found"))
}

pub fn fetch_cover_image(url: &str) -> AppResult<Vec<u8>> {
    if !is_allowed_cover_url(url) {
        return Err(AppError::msg("that image can't be used"));
    }
    let output = spawn_tool(find_curl()?)
        .args([
            "-fsSL",
            "--max-time",
            "20",
            "--max-filesize",
            "2097152",
            "-A",
            "Mozilla/5.0",
            url,
        ])
        .output_within(Duration::from_secs(30))
        .map_err(|error| AppError::msg(format!("could not run curl: {error}")))?;
    if !output.status.success() {
        return Err(AppError::msg("couldn't download that image"));
    }
    let bytes = output.stdout;
    if bytes.len() < 32 || bytes.len() > 2_097_152 {
        return Err(AppError::msg("that image is too large"));
    }
    if crate::tags::sniff_image_mime(&bytes).is_none() {
        return Err(AppError::msg("that file isn't an image"));
    }
    Ok(bytes)
}

pub fn is_allowed_cover_url(url: &str) -> bool {
    let url = url.trim();
    if !url.starts_with("https://") || url.len() > 500 {
        return false;
    }
    if url
        .bytes()
        .any(|byte| byte < b' ' || byte == b'\\' || byte == b'\'' || byte == b'"')
    {
        return false;
    }
    let host = url
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    host == "i.ytimg.com"
        || host == "img.youtube.com"
        || host == "yt3.ggpht.com"
        || host == "yt3.googleusercontent.com"
        || (host.starts_with('i') && host.ends_with(".ytimg.com"))
}

pub fn play_source<'a>(url: &'a str, page_url: Option<&'a str>) -> &'a str {
    page_url
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .unwrap_or(url)
}

pub fn cache_media(url: &str) -> AppResult<PathBuf> {
    prepare_for_player(fetch_original(url)?)
}

/// Download (or reuse) the cached original, without remuxing for rodio.
fn fetch_original(url: &str) -> AppResult<PathBuf> {
    fail_if_cancelled()?;
    let url = url.trim();
    if url.is_empty() {
        return Err(AppError::msg("missing stream url"));
    }
    let id = video_id_from_url(url).unwrap_or_else(|| stable_id(url));
    if let Some(existing) = existing_cache(&cache_dir(), &id) {
        return Ok(existing);
    }

    let _inflight = InflightGuard::new(id.clone());
    let mut errors = Vec::new();
    if let Ok(info) = probe_info(url) {
        fail_if_cancelled()?;
        if let Some(picked) = pick_audio(&info) {
            if let Some(format_id) = picked.format_id.as_deref() {
                fail_if_cancelled()?;
                match download_with_format(url, format_id) {
                    Ok(path) => return Ok(path),
                    Err(error) => errors.push(error),
                }
            }
            if let Some(direct) = picked.url.as_deref() {
                fail_if_cancelled()?;
                match save_direct(&id, direct, &picked.ext) {
                    Ok(path) => return Ok(path),
                    Err(error) => errors.push(error),
                }
            }
        }
    }

    fail_if_cancelled()?;
    match download_default(url) {
        Ok(path) => return Ok(path),
        Err(error) => errors.push(error),
    }

    fail_if_cancelled()?;
    if let Some(video_id) = video_id_from_url(url) {
        match download_via_frontends(&video_id) {
            Ok(path) => return Ok(path),
            Err(error) => errors.push(error),
        }
    }

    Err(errors
        .pop()
        .unwrap_or_else(|| AppError::msg("could not cache that stream")))
}

pub fn save_media(url: &str, dest: &Path) -> AppResult<PathBuf> {
    let cached = fetch_original(url)?;
    let dest = save_dest(&cached, dest);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&cached, &dest)?;
    Ok(dest)
}

/// Keep the name the user picked. Only add an extension when they omitted one.
fn save_dest(cached: &Path, dest: &Path) -> PathBuf {
    if dest.extension().is_some() {
        dest.to_path_buf()
    } else {
        match cached.extension() {
            Some(ext) => dest.with_extension(ext),
            None => dest.to_path_buf(),
        }
    }
}

pub fn drop_temps_except(keep: Option<&Path>) {
    drop_temps_except_in(&cache_dir(), keep);
}

fn drop_temps_except_in(dir: &Path, keep: Option<&Path>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if keep.is_some_and(|keep| keep == path.as_path()) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.ends_with(".part") || name.ends_with(".ytdl") {
            continue;
        }
        if is_inflight_name(name) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

pub fn clear_temps() {
    drop_temps_except(None);
}

thread_local! {
    static CANCEL: RefCell<Option<Arc<dyn Fn() -> bool + Send + Sync>>> = RefCell::new(None);
}

struct CancelOnDrop;

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        CANCEL.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Run `work` so nested `output_within` calls abort when `cancel` is true.
pub(crate) fn with_cancel<R>(
    cancel: Arc<dyn Fn() -> bool + Send + Sync>,
    work: impl FnOnce() -> R,
) -> R {
    CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel));
    let _guard = CancelOnDrop;
    work()
}

fn search_cancelled() -> bool {
    CANCEL.with(|slot| slot.borrow().as_ref().is_some_and(|check| check()))
}

fn fail_if_cancelled() -> AppResult<()> {
    if search_cancelled() {
        Err(AppError::msg("search play was replaced"))
    } else {
        Ok(())
    }
}

fn inflight() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

struct InflightGuard(String);

impl InflightGuard {
    fn new(id: String) -> Self {
        inflight()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone());
        Self(id)
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        inflight()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

fn is_inflight_name(name: &str) -> bool {
    let set = inflight().lock().unwrap_or_else(|e| e.into_inner());
    set.iter()
        .any(|id| name == id || name.starts_with(&format!("{id}.")))
}

pub fn track_for(path: &Path, title: &str) -> Track {
    Track {
        path: path.to_string_lossy().into_owned(),
        title: title.trim().to_string(),
        artist: String::new(),
        album: "Search".into(),
        album_artist: String::new(),
        track: None,
        disc: None,
        duration_ms: 0,
        folder: path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default(),
        replaygain_track: None,
        replaygain_album: None,
    }
}

fn resolve_hit(query: &str) -> AppResult<MediaHit> {
    if is_url(query) {
        let hits = flat_search(query)?;
        return hits
            .into_iter()
            .next()
            .ok_or_else(|| AppError::msg("no playable result"));
    }
    let mut last_error = None;
    for target in play_query_variants(query)
        .into_iter()
        .chain(soundcloud_queries(query))
    {
        match flat_search(&target) {
            Ok(hits) => {
                if let Some(hit) = hits.into_iter().next() {
                    return Ok(hit);
                }
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| AppError::msg("no playable result")))
}

fn flat_search(target: &str) -> AppResult<Vec<MediaHit>> {
    let mut args = vec![
        "-J",
        "--flat-playlist",
        "--ignore-errors",
        "--no-playlist",
        "--",
        target,
    ];
    if !is_url(target) {
        args.retain(|item| *item != "--no-playlist");
    }
    let value = ytdlp_json(&args)?;
    Ok(hits_from_json(&value))
}

fn extract_stream(page_url: &str) -> AppResult<String> {
    if let Ok(info) = probe_info(page_url) {
        if let Some(url) = pick_audio(&info).and_then(|picked| picked.url) {
            return Ok(url);
        }
    }
    Err(AppError::msg("could not extract a stream"))
}

#[derive(Debug, Clone)]
struct PickedAudio {
    format_id: Option<String>,
    url: Option<String>,
    ext: String,
}

fn probe_info(url: &str) -> AppResult<Value> {
    let mut errors = Vec::new();
    for clients in client_attempts(url) {
        let mut args = vec!["-J", "--skip-download", "--no-playlist"];
        if let Some(clients) = clients {
            args.extend(["--extractor-args", clients]);
        }
        args.extend(["--", url]);
        match ytdlp_json(&args) {
            Ok(value) => return Ok(value),
            Err(error) => errors.push(error),
        }
    }
    Err(errors
        .pop()
        .unwrap_or_else(|| AppError::msg("could not read media info")))
}

fn collect_formats(value: &Value) -> Vec<&Value> {
    if let Some(formats) = value.get("formats").and_then(Value::as_array) {
        return formats.iter().collect();
    }
    if let Some(entries) = value.get("entries").and_then(Value::as_array) {
        for entry in entries {
            if let Some(formats) = entry.get("formats").and_then(Value::as_array) {
                return formats.iter().collect();
            }
        }
    }
    Vec::new()
}

fn pick_audio(value: &Value) -> Option<PickedAudio> {
    let mut best: Option<(i32, PickedAudio)> = None;
    for format in collect_formats(value) {
        let acodec = format
            .get("acodec")
            .and_then(Value::as_str)
            .unwrap_or("none");
        if acodec == "none" {
            continue;
        }
        let vcodec = format
            .get("vcodec")
            .and_then(Value::as_str)
            .unwrap_or("none");
        let ext = format
            .get("ext")
            .and_then(Value::as_str)
            .unwrap_or("m4a")
            .to_string();
        let bitrate = format
            .get("abr")
            .and_then(Value::as_f64)
            .or_else(|| format.get("tbr").and_then(Value::as_f64))
            .unwrap_or(0.0) as i32;
        let mut score = bitrate;
        if vcodec == "none" {
            score += 10_000;
        }
        if matches!(ext.as_str(), "m4a" | "mp3" | "ogg" | "opus" | "aac") {
            score += 500;
        } else if ext == "webm" {
            score += 200;
        }
        let picked = PickedAudio {
            format_id: format
                .get("format_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            url: format
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string),
            ext,
        };
        if best
            .as_ref()
            .map(|(best_score, _)| score > *best_score)
            .unwrap_or(true)
        {
            best = Some((score, picked));
        }
    }
    if let Some((_, picked)) = best {
        return Some(picked);
    }
    let url = value.get("url").and_then(Value::as_str)?;
    if url.starts_with("http") {
        return Some(PickedAudio {
            format_id: None,
            url: Some(url.to_string()),
            ext: value
                .get("ext")
                .and_then(Value::as_str)
                .unwrap_or("m4a")
                .to_string(),
        });
    }
    None
}

fn download_with_format(url: &str, format_id: &str) -> AppResult<PathBuf> {
    download_with_clients(url, &["-f", format_id])
}

fn download_default(url: &str) -> AppResult<PathBuf> {
    download_with_clients(url, &["-f", "ba[ext=m4a]/ba[ext=mp3]/ba/b"])
}

fn download_with_clients(url: &str, extra: &[&str]) -> AppResult<PathBuf> {
    let mut errors = Vec::new();
    for clients in client_attempts(url) {
        match download_template(url, extra, clients) {
            Ok(path) => return Ok(path),
            Err(error) => errors.push(error),
        }
    }
    Err(errors
        .pop()
        .unwrap_or_else(|| AppError::msg("could not cache that stream")))
}

fn client_attempts(url: &str) -> Vec<Option<&'static str>> {
    if youtube_page(url) {
        PLAYER_CLIENTS
            .iter()
            .copied()
            .map(Some)
            .chain([None])
            .collect()
    } else {
        vec![None]
    }
}

fn download_template(url: &str, extra: &[&str], clients: Option<&str>) -> AppResult<PathBuf> {
    let dir = cache_dir();
    let id = video_id_from_url(url).unwrap_or_else(|| stable_id(url));
    if let Some(existing) = existing_cache(&dir, &id) {
        return Ok(existing);
    }
    let template = dir.join(format!("{id}.%(ext)s"));
    let template = template.to_string_lossy();
    let mut args = extra.to_vec();
    if let Some(clients) = clients {
        args.extend(["--extractor-args", clients]);
    }
    args.extend([
        "--no-playlist",
        "--no-overwrites",
        "--no-simulate",
        "--quiet",
        "-o",
        template.as_ref(),
        "--",
        url,
    ]);
    run_ytdlp(&args)?;
    existing_cache(&dir, &id).ok_or_else(|| AppError::msg("could not cache that stream"))
}

fn save_direct(id: &str, url: &str, ext: &str) -> AppResult<PathBuf> {
    let ext = if ext.is_empty() { "m4a" } else { ext };
    let dest = cache_dir().join(format!("{id}.{ext}"));
    if dest.is_file() {
        return Ok(dest);
    }
    http_download(url, &dest)?;
    if dest.is_file() && dest.metadata().map(|meta| meta.len() > 0).unwrap_or(false) {
        return Ok(dest);
    }
    let _ = std::fs::remove_file(&dest);
    Err(AppError::msg("could not download that stream"))
}

fn download_via_frontends(id: &str) -> AppResult<PathBuf> {
    let mut last_error = AppError::msg("could not fetch audio from another source");
    for base in INVIDIOUS_APIS {
        match http_get(&format!("{base}/api/v1/videos/{id}")) {
            Ok(body) => {
                if let Ok(value) = serde_json::from_str::<Value>(&body) {
                    if let Some((url, ext)) = pick_invidious_audio(&value) {
                        match save_direct(id, &url, &ext) {
                            Ok(path) => return Ok(path),
                            Err(error) => last_error = error,
                        }
                    }
                }
            }
            Err(error) => last_error = error,
        }
    }
    for base in PIPED_APIS {
        match http_get(&format!("{base}/streams/{id}")) {
            Ok(body) => {
                if let Ok(value) = serde_json::from_str::<Value>(&body) {
                    if let Some((url, ext)) = pick_piped_audio(&value) {
                        match save_direct(id, &url, &ext) {
                            Ok(path) => return Ok(path),
                            Err(error) => last_error = error,
                        }
                    }
                }
            }
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

pub fn pick_invidious_audio(value: &Value) -> Option<(String, String)> {
    let formats = value.get("adaptiveFormats").and_then(Value::as_array)?;
    let mut best: Option<(i64, String, String)> = None;
    for format in formats {
        let kind = format.get("type").and_then(Value::as_str).unwrap_or("");
        if !kind.starts_with("audio/") {
            continue;
        }
        let url = format.get("url").and_then(Value::as_str)?;
        let bitrate = format
            .get("bitrate")
            .and_then(Value::as_i64)
            .or_else(|| format.get("audioSampleRate").and_then(Value::as_i64))
            .unwrap_or(0);
        let ext = if kind.contains("webm") { "webm" } else { "m4a" };
        if best
            .as_ref()
            .map(|(best_rate, _, _)| bitrate > *best_rate)
            .unwrap_or(true)
        {
            best = Some((bitrate, url.to_string(), ext.to_string()));
        }
    }
    best.map(|(_, url, ext)| (url, ext))
}

pub fn pick_piped_audio(value: &Value) -> Option<(String, String)> {
    let streams = value.get("audioStreams").and_then(Value::as_array)?;
    let first = streams.first()?;
    let url = first.get("url").and_then(Value::as_str)?;
    let mime = first.get("mimeType").and_then(Value::as_str).unwrap_or("");
    let ext = if mime.contains("webm") { "webm" } else { "m4a" };
    Some((url.to_string(), ext.to_string()))
}

fn http_get(url: &str) -> AppResult<String> {
    let output = spawn_tool(find_curl()?)
        .args(["-fsSL", "--max-time", "25", "-A", "Mozilla/5.0", url])
        .output_within(Duration::from_secs(35))
        .map_err(|error| AppError::msg(format!("could not run curl: {error}")))?;
    if !output.status.success() {
        return Err(AppError::msg("could not reach an audio source"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn http_download(url: &str, dest: &Path) -> AppResult<()> {
    if let Ok(curl) = find_curl() {
        let output = spawn_tool(curl)
            .args(["-fsSL", "--max-time", "90", "-A", "Mozilla/5.0", "-o"])
            .arg(dest)
            .arg(url)
            .output_within(Duration::from_secs(100))
            .map_err(|error| AppError::msg(format!("could not run curl: {error}")))?;
        if output.status.success() {
            return Ok(());
        }
    }
    let dest_arg = dest.to_string_lossy();
    run_ytdlp(&[
        "--no-playlist",
        "--no-overwrites",
        "--no-simulate",
        "--quiet",
        "-o",
        dest_arg.as_ref(),
        "--",
        url,
    ])?;
    Ok(())
}

fn find_curl() -> AppResult<PathBuf> {
    find_tool("curl").ok_or_else(|| AppError::msg("curl is required to fetch search audio"))
}

fn existing_cache(dir: &Path, id: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut matches: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            if !path.is_file() {
                return false;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.ends_with(".part") {
                return false;
            }
            path.file_stem().and_then(|stem| stem.to_str()) == Some(id)
        })
        .collect();
    matches.sort_by_key(|path| player_safe_rank(path));
    matches.into_iter().next()
}

// YouTube AAC-in-MP4 makes rodio/Symphonia panic during decoder init.
// Keep playback on wav/mp3/flac/ogg and remux anything else.
fn player_safe_path(path: &Path) -> bool {
    player_safe_rank(path) < 9
}

fn player_safe_rank(path: &Path) -> u8 {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "wav" => 0,
        "mp3" => 1,
        "flac" | "ogg" => 2,
        _ => 9,
    }
}

fn prepare_for_player(path: PathBuf) -> AppResult<PathBuf> {
    if player_safe_path(&path) {
        return Ok(path);
    }
    remux_for_player(&path)
}

fn remux_for_player(path: &Path) -> AppResult<PathBuf> {
    let ffmpeg = find_ffmpeg()?;
    for (ext, extra) in [
        ("mp3", &["-codec:a", "libmp3lame", "-q:a", "4"] as &[&str]),
        ("wav", &["-acodec", "pcm_s16le", "-ar", "44100", "-ac", "2"]),
    ] {
        let dest = path.with_extension(ext);
        if dest != path
            && dest.is_file()
            && dest.metadata().map(|meta| meta.len() > 0).unwrap_or(false)
        {
            let _ = std::fs::remove_file(path);
            return Ok(dest);
        }
        let output = spawn_tool(&ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-vn"])
            .args(extra)
            .arg(&dest)
            .output_within(TOOL_TRANSCODE_TIMEOUT)
            .map_err(|error| AppError::msg(format!("could not run ffmpeg: {error}")))?;
        if output.status.success()
            && dest.is_file()
            && dest.metadata().map(|meta| meta.len() > 0).unwrap_or(false)
        {
            if dest != path {
                let _ = std::fs::remove_file(path);
            }
            return Ok(dest);
        }
        let _ = std::fs::remove_file(&dest);
    }
    Err(AppError::msg(
        "downloaded the song, but could not convert it into a playable file",
    ))
}

fn find_ffmpeg() -> AppResult<PathBuf> {
    find_tool("ffmpeg").ok_or_else(|| {
        AppError::msg(
            "ffmpeg is required to play search results. Install it with: sudo apt install ffmpeg",
        )
    })
}

pub(crate) fn find_tool(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs.push(PathBuf::from("/bin"));
    dirs.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let mut seen = Vec::new();
    for dir in dirs {
        if seen.iter().any(|item| item == &dir) {
            continue;
        }
        seen.push(dir.clone());
        if is_bundled_path(&dir) {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn is_bundled_path(path: &Path) -> bool {
    if let Ok(appdir) = std::env::var("APPDIR") {
        if !appdir.is_empty() && path.starts_with(&appdir) {
            return true;
        }
    }
    path.to_string_lossy().contains("/tmp/.mount_")
}

pub fn video_id_from_url(url: &str) -> Option<String> {
    if let Some(id) = url.split("v=").nth(1) {
        let id = id.split(['&', '#']).next()?.trim();
        if youtube_video_id(id) {
            return Some(id.to_string());
        }
    }
    for prefix in ["youtu.be/", "youtube.com/embed/", "youtube.com/shorts/"] {
        if let Some(index) = url.find(prefix) {
            let id = url[index + prefix.len()..]
                .split(['?', '&', '/', '#'])
                .next()?
                .trim();
            if youtube_video_id(id) {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn stable_id(url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn hits_from_json(value: &Value) -> Vec<MediaHit> {
    if let Some(entries) = value.get("entries").and_then(Value::as_array) {
        return entries.iter().filter_map(hit_from_value).collect();
    }
    hit_from_value(value).into_iter().collect()
}

fn hit_from_value(value: &Value) -> Option<MediaHit> {
    if value.is_null() {
        return None;
    }
    let title = value.get("title").and_then(Value::as_str)?.trim();
    if title.is_empty() {
        return None;
    }
    let url = locator_from_value(value)?;
    Some(MediaHit {
        title: title.to_string(),
        url: url.clone(),
        page_url: Some(url.clone()),
        thumbnail_url: thumbnail_url_for(&url, value),
        channel: text_field(value, &["channel", "uploader", "artist"]),
    })
}

fn thumbnail_url_for(url: &str, value: &Value) -> Option<String> {
    if let Some(id) = video_id_from_url(url) {
        return Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"));
    }
    value
        .get("thumbnails")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().rev().find_map(|item| {
                item.get("url")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|thumb| !thumb.is_empty())
                    .map(str::to_string)
            })
        })
}

fn text_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
    })
}

fn locator_from_value(value: &Value) -> Option<String> {
    // SoundCloud and other sites put the real page in webpage_url. A short id
    // must not be turned into a YouTube watch URL when that page is present.
    if let Some(page) = http_field(value, "webpage_url").or_else(|| http_field(value, "url")) {
        return Some(page);
    }
    if let Some(id) = value.get("id").and_then(Value::as_str).map(str::trim) {
        if youtube_video_id(id) {
            return Some(format!("https://www.youtube.com/watch?v={id}"));
        }
    }
    value
        .get("webpage_url")
        .and_then(Value::as_str)
        .or_else(|| value.get("url").and_then(Value::as_str))
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn http_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| item.starts_with("http://") || item.starts_with("https://"))
        .map(str::to_string)
}

pub fn youtube_page(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("://youtube.com")
        || lower.contains("://www.youtube.com")
        || lower.contains("://m.youtube.com")
        || lower.contains("://music.youtube.com")
        || lower.contains("://youtu.be/")
        || lower.contains("ytsearch")
}

fn youtube_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

pub fn play_query_variants(query: &str) -> Vec<String> {
    if is_url(query) {
        return vec![query.to_string()];
    }
    let mut queries = Vec::new();
    add_search(&mut queries, &format!("{query} audio"), 5);
    add_search(&mut queries, query, 5);
    let stripped = strip_search_noise(query);
    if stripped != query {
        add_search(&mut queries, &format!("{stripped} audio"), 5);
        add_search(&mut queries, &stripped, 5);
    }
    if !stripped.is_empty() && stripped != query {
        add_search(&mut queries, &stripped, 10);
    }
    queries
}

fn list_query_variants(query: &str) -> Vec<String> {
    if is_url(query) {
        return vec![query.to_string()];
    }
    let mut queries = Vec::new();
    add_search(&mut queries, query, 8);
    let stripped = strip_search_noise(query);
    if stripped != query {
        add_search(&mut queries, &stripped, 8);
    }
    queries
}

pub fn cover_query_variants(query: &str) -> Vec<String> {
    if is_url(query) {
        return Vec::new();
    }
    let mut queries = Vec::new();
    add_search(&mut queries, query, 4);
    let stripped = strip_search_noise(query);
    if stripped != query {
        add_search(&mut queries, &stripped, 4);
    }
    queries
}

fn add_search(queries: &mut Vec<String>, terms: &str, count: u8) {
    add_source(queries, "ytsearch", terms, count);
}

pub fn soundcloud_queries(query: &str) -> Vec<String> {
    if is_url(query) {
        return Vec::new();
    }
    let mut queries = Vec::new();
    let cleaned = strip_search_noise(query);
    let terms = if cleaned.is_empty() { query } else { &cleaned };
    add_source(&mut queries, "scsearch", terms, 5);
    queries
}

fn add_source(queries: &mut Vec<String>, prefix: &str, terms: &str, count: u8) {
    let terms = collapse_ws(terms);
    if terms.is_empty() {
        return;
    }
    let query = format!("{prefix}{count}:{terms}");
    if !queries.iter().any(|item| item == &query) {
        queries.push(query);
    }
}

fn merge_hits(hits: &mut Vec<MediaHit>, found: Vec<MediaHit>) {
    for hit in found {
        if hits.iter().any(|item| item.url == hit.url) {
            continue;
        }
        hits.push(hit);
    }
}

pub fn strip_search_noise(query: &str) -> String {
    let mut out = String::new();
    let mut depth_round = 0u32;
    let mut depth_square = 0u32;
    let mut depth_curly = 0u32;
    for ch in query.chars() {
        match ch {
            '(' => depth_round += 1,
            ')' => depth_round = depth_round.saturating_sub(1),
            '[' => depth_square += 1,
            ']' => depth_square = depth_square.saturating_sub(1),
            '{' => depth_curly += 1,
            '}' => depth_curly = depth_curly.saturating_sub(1),
            _ if depth_round + depth_square + depth_curly == 0 => out.push(ch),
            _ => {}
        }
    }
    let mut out = collapse_ws(&out);
    if let Some(stripped) = strip_feat_tail(&out) {
        out = stripped;
    }
    out = out.replace(['\'', '’'], "");
    out = out
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch.is_whitespace() || ch == '-' {
                ch
            } else {
                ' '
            }
        })
        .collect();
    collapse_ws(&out)
}

fn strip_feat_tail(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    for marker in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(index) = lower.find(marker) {
            return Some(collapse_ws(&value[..index]));
        }
    }
    None
}

fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn clean_query(query: &str) -> AppResult<String> {
    let cleaned = collapse_ws(&query.replace(['\n', '\r', '\0'], " "));
    if cleaned.is_empty() {
        return Err(AppError::msg("type something to search"));
    }
    if cleaned.chars().count() > 200 {
        return Err(AppError::msg("search is too long"));
    }
    Ok(cleaned)
}

fn node_available() -> bool {
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(probe_node)
}

fn probe_node() -> bool {
    spawn_tool("node")
        .arg("--version")
        .output_within(TOOL_PROBE_TIMEOUT)
        .map(|output| output.status.success())
        .unwrap_or(false)
}

const CHILD_ENV_STRIP: &[&str] = &[
    "PYTHONHOME",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "PYTHONUSERBASE",
    "PYTHONEXECUTABLE",
    "PYTHONSAFEPATH",
    "VIRTUAL_ENV",
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "GCONV_PATH",
    "LOCPATH",
    "PERL5LIB",
    "PERLLIB",
    "GTK_PATH",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GIO_EXTRA_MODULES",
    "GSETTINGS_SCHEMA_DIR",
    "GDK_PIXBUF_MODULE_FILE",
    "QT_PLUGIN_PATH",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GI_TYPELIB_PATH",
];

/// Wall-clock limits for child processes. `.output()` alone waits forever on
/// a hung network call or a JS challenge that never resolves.
pub(crate) const TOOL_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
pub(crate) const TOOL_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(240);
pub(crate) const TOOL_TRANSCODE_TIMEOUT: Duration = Duration::from_secs(600);

pub(crate) trait OutputWithin {
    /// Like `Command::output`, but kills the child after `timeout`.
    fn output_within(&mut self, timeout: Duration) -> std::io::Result<Output>;
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

impl OutputWithin for Command {
    fn output_within(&mut self, timeout: Duration) -> std::io::Result<Output> {
        self.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = self.spawn()?;
        let out = drain(child.stdout.take());
        let err = drain(child.stderr.take());
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if search_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "search play was replaced",
                ));
            }
            if started.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("gave up after {} s", timeout.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        Ok(Output {
            status,
            stdout: out.join().unwrap_or_default(),
            stderr: err.join().unwrap_or_default(),
        })
    }
}

pub(crate) fn spawn_tool(bin: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(bin);
    // AppImage AppRun exports GTK/GIO/Python paths that point at the squashfs.
    // Host yt-dlp is a Python zipapp; ffmpeg and curl need the host linker
    // and certs. Named vars are always dropped; anything whose value lives
    // under $APPDIR or /tmp/.mount_* is dropped too.
    for (key, value) in std::env::vars_os() {
        if should_strip_child_env(&key, &value) {
            command.env_remove(&key);
        }
    }
    command.env("PATH", host_path());
    command
}

pub fn should_strip_child_env(key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> bool {
    let key = key.as_ref();
    if key == "PATH" || keep_child_env(key) {
        return false;
    }
    if CHILD_ENV_STRIP.iter().any(|item| OsStr::new(item) == key) {
        return true;
    }
    env_value_is_bundled(value.as_ref())
}

fn keep_child_env(key: &OsStr) -> bool {
    let Some(name) = key.to_str() else {
        return false;
    };
    if name.starts_with("LC_") {
        return true;
    }
    matches!(
        name,
        "HOME"
            | "USER"
            | "LOGNAME"
            | "USERNAME"
            | "SHELL"
            | "DISPLAY"
            | "WAYLAND_DISPLAY"
            | "XAUTHORITY"
            | "XDG_RUNTIME_DIR"
            | "XDG_SESSION_TYPE"
            | "XDG_SESSION_CLASS"
            | "XDG_SESSION_DESKTOP"
            | "XDG_CURRENT_DESKTOP"
            | "XDG_MENU_PREFIX"
            | "XDG_CONFIG_HOME"
            | "XDG_CACHE_HOME"
            | "XDG_STATE_HOME"
            | "XDG_DATA_HOME"
            | "DESKTOP_SESSION"
            | "DBUS_SESSION_BUS_ADDRESS"
            | "DBUS_SYSTEM_BUS_ADDRESS"
            | "LANG"
            | "LANGUAGE"
            | "TZ"
            | "TERM"
            | "COLORTERM"
            | "http_proxy"
            | "https_proxy"
            | "HTTP_PROXY"
            | "HTTPS_PROXY"
            | "no_proxy"
            | "NO_PROXY"
            | "all_proxy"
            | "ALL_PROXY"
            | "PULSE_SERVER"
            | "PULSE_COOKIE"
            | "PIPEWIRE_RUNTIME_DIR"
            | "AUDIOS_YTDLP"
            | "SSH_AUTH_SOCK"
    )
}

fn env_value_is_bundled(value: &OsStr) -> bool {
    value
        .to_string_lossy()
        .split([':', '\n', ';'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| is_bundled_path(Path::new(part)))
}

fn host_path() -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".local/share/pipx/venvs/yt-dlp/bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs.push(PathBuf::from("/bin"));
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if is_bundled_path(&dir) {
            continue;
        }
        if !dirs.iter().any(|item| item == &dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into())
}

fn ytdlp_json(args: &[&str]) -> AppResult<Value> {
    let stdout = run_ytdlp(args)?;
    let value: Value = serde_json::from_str(&stdout)
        .map_err(|_| AppError::msg("could not read search results"))?;
    Ok(value)
}

fn run_ytdlp(args: &[&str]) -> AppResult<String> {
    let bin = find_ytdlp()?;
    let mut command = spawn_tool(&bin);
    command.args(YTDLP_BASE);
    if node_available() {
        command.args(["--js-runtimes", "node"]);
    }
    let output = command
        .args(args)
        .output_within(TOOL_DOWNLOAD_TIMEOUT)
        .map_err(|error| AppError::msg(format!("could not run yt-dlp: {error}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::msg(clean_ytdlp_error(&err)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn find_ytdlp() -> AppResult<PathBuf> {
    static CACHED: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    match CACHED.get_or_init(probe_ytdlp) {
        Ok(path) => Ok(path.clone()),
        Err(msg) => Err(AppError::msg(msg.clone())),
    }
}

fn probe_ytdlp() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("AUDIOS_YTDLP") {
        let path = PathBuf::from(path);
        if path.is_file() && !is_bundled_path(&path) {
            return Ok(path);
        }
    }

    // Distro packages are often too old to extract current YouTube responses.
    // Prefer the newest binary among PATH, ~/.local/bin, and pipx.
    let mut dirs: Vec<PathBuf> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.insert(0, home.join(".local/bin"));
        dirs.insert(0, home.join(".local/share/pipx/venvs/yt-dlp/bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));

    let mut seen = Vec::new();
    let mut best: Option<(PathBuf, (u16, u8, u8))> = None;
    let mut fallback = None;
    for dir in dirs {
        if seen.iter().any(|item| item == &dir) {
            continue;
        }
        seen.push(dir.clone());
        if is_bundled_path(&dir) {
            continue;
        }
        for name in ["yt-dlp", "yt-dlp_linux"] {
            let candidate = dir.join(name);
            if !candidate.is_file() {
                continue;
            }
            fallback.get_or_insert_with(|| candidate.clone());
            if let Some(version) = ytdlp_version(&candidate) {
                if best
                    .as_ref()
                    .map(|(_, best_version)| version > *best_version)
                    .unwrap_or(true)
                {
                    best = Some((candidate, version));
                }
            }
        }
    }

    if let Some((path, _)) = best {
        return Ok(path);
    }
    if let Some(path) = fallback {
        return Ok(path);
    }

    Err(
        "yt-dlp is required for search. apt is usually too old; install a current copy with: curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o ~/.local/bin/yt-dlp && chmod a+rx ~/.local/bin/yt-dlp".into(),
    )
}

fn ytdlp_version(bin: &Path) -> Option<(u16, u8, u8)> {
    let output = spawn_tool(bin)
        .arg("--version")
        .output_within(TOOL_PROBE_TIMEOUT)
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ytdlp_version(&String::from_utf8_lossy(&output.stdout))
}

pub fn parse_ytdlp_version(text: &str) -> Option<(u16, u8, u8)> {
    let token = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut parts = token.split('.');
    Some((
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ))
}

fn cache_dir() -> PathBuf {
    let dirs = directories::ProjectDirs::from("com", "audios", "Audios")
        .expect("a home directory is required");
    let dir = dirs.cache_dir().join("search");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn clean_ytdlp_error(error: &str) -> String {
    if error.contains("Python path configuration") || error.contains("PYTHONHOME") {
        return "yt-dlp could not start Python. Use a current AppImage and a system yt-dlp (not one from inside the package).".to_string();
    }
    let line = error
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("Python path"))
        .unwrap_or("search failed");
    line.trim_start_matches("ERROR: ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn parses_extracted_stream() {
        let value = json!({
            "id": "dQw4w9WgXcQ",
            "title": "Example Song",
            "url": "https://cdn.example/stream.m4a",
            "webpage_url": "https://example.com/watch?v=dQw4w9WgXcQ"
        });
        assert_eq!(
            hits_from_json(&value),
            vec![MediaHit {
                title: "Example Song".into(),
                url: "https://example.com/watch?v=dQw4w9WgXcQ".into(),
                page_url: Some("https://example.com/watch?v=dQw4w9WgXcQ".into()),
                thumbnail_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".into()),
                channel: None,
            }]
        );
    }

    #[test]
    fn parses_ytsearch_playlist() {
        let value = json!({
            "_type": "playlist",
            "entries": [
                { "id": "aaaaaaaaaaa", "title": "One", "url": "https://www.youtube.com/watch?v=aaaaaaaaaaa", "channel": "Artist One" },
                null,
                { "id": "bbbbbbbbbbb", "title": "Two", "webpage_url": "https://www.youtube.com/watch?v=bbbbbbbbbbb" },
                { "title": "  ", "url": "https://www.youtube.com/watch?v=3" }
            ]
        });
        assert_eq!(
            hits_from_json(&value),
            vec![
                MediaHit {
                    title: "One".into(),
                    url: "https://www.youtube.com/watch?v=aaaaaaaaaaa".into(),
                    page_url: Some("https://www.youtube.com/watch?v=aaaaaaaaaaa".into()),
                    thumbnail_url: Some("https://i.ytimg.com/vi/aaaaaaaaaaa/hqdefault.jpg".into()),
                    channel: Some("Artist One".into()),
                },
                MediaHit {
                    title: "Two".into(),
                    url: "https://www.youtube.com/watch?v=bbbbbbbbbbb".into(),
                    page_url: Some("https://www.youtube.com/watch?v=bbbbbbbbbbb".into()),
                    thumbnail_url: Some("https://i.ytimg.com/vi/bbbbbbbbbbb/hqdefault.jpg".into()),
                    channel: None,
                },
            ]
        );
    }

    #[test]
    fn play_variants_add_audio_and_widen() {
        let queries = play_query_variants("blackbird beatles");
        assert_eq!(queries[0], "ytsearch5:blackbird beatles audio");
        assert_eq!(queries[1], "ytsearch5:blackbird beatles");
        assert!(!queries.iter().any(|item| item.starts_with("ytsearch1:")));
    }

    #[test]
    fn play_variants_keep_artist_dash_song() {
        let queries = play_query_variants("The Beatles - Blackbird");
        assert!(queries
            .iter()
            .any(|item| item.contains("The Beatles - Blackbird")));
        assert!(!queries.iter().any(|item| item == "ytsearch5:The Beatles"));
    }

    #[test]
    fn soundcloud_search_uses_cleaned_terms() {
        assert_eq!(
            soundcloud_queries("Helium (From the Film)"),
            vec!["scsearch5:Helium".to_string()]
        );
        assert!(soundcloud_queries("https://soundcloud.com/artist/song").is_empty());
    }

    #[test]
    fn soundcloud_page_is_not_rewritten_as_youtube() {
        let value = json!({
            "id": "aaaaaaaaaaa",
            "title": "Lanius",
            "webpage_url": "https://soundcloud.com/benprunty/lanius-battle",
            "uploader": "Ben Prunty"
        });
        let hits = hits_from_json(&value);
        assert_eq!(
            hits[0].url,
            "https://soundcloud.com/benprunty/lanius-battle"
        );
        assert!(!youtube_page(&hits[0].url));
        assert!(youtube_page("https://www.youtube.com/watch?v=aaaaaaaaaaa"));
    }

    #[test]
    fn cover_search_is_small_and_not_a_url_lookup() {
        let queries = cover_query_variants("Artist - Song (Live)");
        assert!(queries.iter().all(|item| item.starts_with("ytsearch4:")));
        assert!(!queries.iter().any(|item| item.contains("ytsearch8:")));
        assert!(cover_query_variants("https://www.youtube.com/watch?v=dQw4w9WgXcQ").is_empty());
    }

    #[test]
    fn cover_urls_stay_on_youtube_thumbs() {
        assert!(is_allowed_cover_url(
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg"
        ));
        assert!(is_allowed_cover_url(
            "https://i1.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"
        ));
        assert!(!is_allowed_cover_url(
            "http://i.ytimg.com/vi/x/hqdefault.jpg"
        ));
        assert!(!is_allowed_cover_url("https://example.com/cover.jpg"));
        assert!(!is_allowed_cover_url("https://evil.com/i.ytimg.com/x.jpg"));
        assert!(!is_allowed_cover_url("file:///tmp/cover.jpg"));
    }

    #[test]
    fn strips_parens_and_feat() {
        assert_eq!(
            strip_search_noise("Song (Remastered 2011) (feat. X)"),
            "Song"
        );
        assert_eq!(
            strip_search_noise("Hello [Official Video] ft. Guest"),
            "Hello"
        );
    }

    #[test]
    fn play_prefers_page_url() {
        assert_eq!(
            play_source(
                "https://cdn.example/stream.m4a",
                Some("https://www.youtube.com/watch?v=aaaaaaaaaaa")
            ),
            "https://www.youtube.com/watch?v=aaaaaaaaaaa"
        );
        assert_eq!(
            play_source("https://cdn.example/stream.m4a", None),
            "https://cdn.example/stream.m4a"
        );
    }

    #[test]
    fn rejects_empty_query() {
        assert!(clean_query("   ").is_err());
    }

    #[test]
    fn reads_youtube_ids() {
        assert_eq!(
            video_id_from_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=x"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(
            video_id_from_url("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".into())
        );
        assert_eq!(video_id_from_url("https://cdn.example/stream.m4a"), None);
    }

    #[test]
    fn drops_other_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("keep.m4a");
        let gone = dir.path().join("gone.m4a");
        std::fs::write(&keep, b"keep").unwrap();
        std::fs::write(&gone, b"gone").unwrap();
        drop_temps_except_in(dir.path(), Some(&keep));
        assert!(keep.is_file());
        assert!(!gone.exists());
    }

    #[test]
    fn save_keeps_the_users_extension() {
        let cached = Path::new("/cache/id.m4a");
        assert_eq!(
            save_dest(cached, Path::new("/home/me/song.ogg")),
            Path::new("/home/me/song.ogg")
        );
        assert_eq!(
            save_dest(cached, Path::new("/home/me/song")),
            Path::new("/home/me/song.m4a")
        );
    }

    #[test]
    fn drop_temps_keeps_an_in_flight_download() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("vid11111111.m4a");
        let gone = dir.path().join("old.m4a");
        std::fs::write(&keep, b"partial").unwrap();
        std::fs::write(&gone, b"gone").unwrap();
        let _guard = InflightGuard::new("vid11111111".into());
        drop_temps_except_in(dir.path(), None);
        assert!(keep.is_file(), "in-flight cache must survive a prune");
        assert!(!gone.exists());
    }

    #[test]
    fn output_within_stops_when_search_is_cancelled() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let flag = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&flag);
        let worker = std::thread::spawn(move || {
            with_cancel(Arc::new(move || seen.load(Ordering::SeqCst)), || {
                Command::new("sleep")
                    .arg("30")
                    .output_within(Duration::from_secs(20))
            })
        });
        std::thread::sleep(Duration::from_millis(150));
        flag.store(true, Ordering::SeqCst);
        let result = worker.join().expect("sleep thread");
        assert!(result.is_err(), "cancelled child must not run to timeout");
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
    }

    #[test]
    fn picks_audio_only_format() {
        let value = json!({
            "formats": [
                { "format_id": "18", "acodec": "mp4a.40.2", "vcodec": "avc1", "ext": "mp4", "tbr": 400 },
                { "format_id": "251", "acodec": "opus", "vcodec": "none", "ext": "webm", "abr": 160, "url": "https://cdn.example/a.webm" },
                { "format_id": "140", "acodec": "mp4a.40.2", "vcodec": "none", "ext": "m4a", "abr": 128, "url": "https://cdn.example/a.m4a" }
            ]
        });
        let picked = pick_audio(&value).unwrap();
        assert_eq!(picked.format_id.as_deref(), Some("140"));
        assert_eq!(picked.ext, "m4a");
    }

    #[test]
    fn parses_ytdlp_version() {
        assert_eq!(parse_ytdlp_version("2024.04.09\n"), Some((2024, 4, 9)));
        assert_eq!(parse_ytdlp_version("2026.08.19"), Some((2026, 8, 19)));
        assert!(parse_ytdlp_version("2026.08.19") > parse_ytdlp_version("2024.04.09"));
    }

    #[test]
    fn picks_invidious_audio() {
        let value = json!({
            "adaptiveFormats": [
                { "type": "video/mp4", "url": "https://cdn.example/v.mp4", "bitrate": 800000 },
                { "type": "audio/mp4; codecs=mp4a.40.2", "url": "https://cdn.example/a.m4a", "bitrate": 131072 }
            ]
        });
        assert_eq!(
            pick_invidious_audio(&value),
            Some(("https://cdn.example/a.m4a".into(), "m4a".into()))
        );
    }

    #[test]
    fn prefers_safe_cache_file() {
        let dir = tempfile::tempdir().unwrap();
        let id = "abcdefghijk";
        std::fs::write(dir.path().join(format!("{id}.m4a")), b"a").unwrap();
        std::fs::write(dir.path().join(format!("{id}.wav")), b"b").unwrap();
        let found = existing_cache(dir.path(), id).unwrap();
        assert_eq!(found.extension().and_then(|ext| ext.to_str()), Some("wav"));
    }

    #[test]
    fn treats_m4a_as_unsafe_for_player() {
        assert!(player_safe_path(Path::new("song.wav")));
        assert!(player_safe_path(Path::new("song.mp3")));
        assert!(!player_safe_path(Path::new("song.m4a")));
        assert!(!player_safe_path(Path::new("song.webm")));
    }

    #[test]
    fn ignores_appimage_mount_paths() {
        assert!(is_bundled_path(Path::new(
            "/tmp/.mount_Audiosxyz/usr/bin/yt-dlp"
        )));
        assert!(!is_bundled_path(Path::new("/usr/bin/yt-dlp")));
        assert!(!is_bundled_path(Path::new("/home/user/.local/bin/yt-dlp")));
    }

    #[test]
    fn python_path_errors_are_readable() {
        let msg = clean_ytdlp_error("Python path configuration:\n  PYTHONHOME = '/tmp/.mount_x'");
        assert!(msg.contains("Python"));
        assert!(!msg.starts_with("Python path configuration"));
    }

    #[test]
    fn strips_appimage_child_env() {
        assert!(should_strip_child_env("PYTHONHOME", "/usr"));
        assert!(should_strip_child_env(
            "GIO_EXTRA_MODULES",
            "/tmp/.mount_x/usr/lib/gio/modules"
        ));
        assert!(should_strip_child_env(
            "XDG_DATA_DIRS",
            "/tmp/.mount_x/usr/share:/usr/share"
        ));
        assert!(should_strip_child_env(
            "SSL_CERT_FILE",
            "/tmp/.mount_x/usr/lib/ssl/cert.pem"
        ));
        assert!(!should_strip_child_env("HOME", "/home/user"));
        assert!(!should_strip_child_env("DISPLAY", ":0"));
        assert!(!should_strip_child_env("LANG", "en_US.UTF-8"));
        assert!(!should_strip_child_env(
            "PATH",
            "/tmp/.mount_x/usr/bin:/usr/bin"
        ));
        assert!(!should_strip_child_env(
            "SSL_CERT_FILE",
            "/etc/ssl/certs/ca-certificates.crt"
        ));
    }
}

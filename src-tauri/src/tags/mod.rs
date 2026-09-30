use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use base64::Engine as _;
use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::{ItemKey, ItemValue, Tag, TagItem};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::persist::rename_atomic;
use crate::player::scan::{collect_tracks, is_audio_path};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagFields {
    pub title: Option<String>,
    pub artists: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub date: Option<String>,
    pub track: Option<String>,
    pub track_total: Option<String>,
    pub disc: Option<String>,
    pub disc_total: Option<String>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub composer: Option<String>,
    pub conductor: Option<String>,
    pub remixer: Option<String>,
    pub grouping: Option<String>,
    pub bpm: Option<String>,
    pub key: Option<String>,
    pub isrc: Option<String>,
    pub barcode: Option<String>,
    pub catalog: Option<String>,
    pub copyright: Option<String>,
    pub encoder: Option<String>,
    pub language: Option<String>,
    pub compilation: Option<String>,
    pub lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
    pub replaygain_track_gain: Option<String>,
    pub replaygain_track_peak: Option<String>,
    pub replaygain_album_gain: Option<String>,
    pub replaygain_album_peak: Option<String>,
    pub musicbrainz_recording_id: Option<String>,
    pub musicbrainz_release_id: Option<String>,
    pub musicbrainz_artist_id: Option<String>,
    pub musicbrainz_release_artist_id: Option<String>,
    pub musicbrainz_track_id: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureInfo {
    pub index: usize,
    pub kind: String,
    pub mime: String,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawItem {
    pub key: String,
    pub value: String,
    pub known: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagDoc {
    pub path: String,
    pub format: String,
    pub tag_type: String,
    pub duration_ms: u64,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bitrate_kbps: Option<u32>,
    pub bit_depth: Option<u8>,
    pub fields: TagFields,
    pub pictures: Vec<PictureInfo>,
    pub raw: Vec<RawItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverArt {
    pub mime: String,
    pub data_base64: String,
}

const KNOWN_KEYS: &[ItemKey] = &[
    ItemKey::TrackTitle,
    ItemKey::TrackArtist,
    ItemKey::AlbumTitle,
    ItemKey::AlbumArtist,
    ItemKey::Year,
    ItemKey::RecordingDate,
    ItemKey::TrackNumber,
    ItemKey::TrackTotal,
    ItemKey::DiscNumber,
    ItemKey::DiscTotal,
    ItemKey::Genre,
    ItemKey::Comment,
    ItemKey::Composer,
    ItemKey::Conductor,
    ItemKey::Remixer,
    ItemKey::ContentGroup,
    ItemKey::Bpm,
    ItemKey::InitialKey,
    ItemKey::Isrc,
    ItemKey::Barcode,
    ItemKey::CatalogNumber,
    ItemKey::CopyrightMessage,
    ItemKey::EncodedBy,
    ItemKey::Language,
    ItemKey::FlagCompilation,
    ItemKey::Lyrics,
    ItemKey::ReplayGainTrackGain,
    ItemKey::ReplayGainTrackPeak,
    ItemKey::ReplayGainAlbumGain,
    ItemKey::ReplayGainAlbumPeak,
    ItemKey::MusicBrainzRecordingId,
    ItemKey::MusicBrainzReleaseId,
    ItemKey::MusicBrainzArtistId,
    ItemKey::MusicBrainzReleaseArtistId,
    ItemKey::MusicBrainzTrackId,
];

pub fn read_tags(path: &str) -> AppResult<TagDoc> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(AppError::msg("pick an audio file to edit"));
    }
    let tagged = read_audio(&path)?;
    let properties = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let tag_type = tagged
        .primary_tag()
        .map(|tag| format!("{:?}", tag.tag_type()))
        .unwrap_or_else(|| format!("{:?}", tagged.primary_tag_type()));

    let mut fields = TagFields::default();
    let mut pictures = Vec::new();
    let mut raw = Vec::new();

    if let Some(tag) = tag {
        fields = fields_from_tag(tag);
        pictures = pictures_from_tag(tag);
        raw = raw_from_tag(tag);
    }

    Ok(TagDoc {
        path: path.to_string_lossy().to_string(),
        format: format!("{:?}", tagged.file_type()),
        tag_type,
        duration_ms: properties.duration().as_millis() as u64,
        sample_rate: properties.sample_rate(),
        channels: properties.channels(),
        bitrate_kbps: properties.audio_bitrate(),
        bit_depth: properties.bit_depth(),
        fields,
        pictures,
        raw,
    })
}

pub fn write_tags(path: &str, fields: TagFields) -> AppResult<TagDoc> {
    mutate_tag(Path::new(path), |tag| apply_fields(tag, &fields, None))?;
    forget_thumb(path);
    read_tags(path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchFailure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub written: usize,
    pub failed: Vec<BatchFailure>,
}

/// Write `apply` fields to every path. A file that fails is reported and the
/// rest still get written; files already done are not rolled back.
pub fn batch_write(
    paths: Vec<String>,
    fields: TagFields,
    apply: Vec<String>,
) -> AppResult<BatchResult> {
    if apply.is_empty() {
        return Err(AppError::msg("choose at least one field to apply"));
    }
    let mut result = BatchResult {
        written: 0,
        failed: Vec::new(),
    };
    for path in paths {
        match mutate_tag(Path::new(&path), |tag| {
            apply_fields(tag, &fields, Some(&apply))
        }) {
            Ok(()) => {
                forget_thumb(&path);
                result.written += 1;
            }
            Err(error) => result.failed.push(BatchFailure {
                path,
                error: error.to_string(),
            }),
        }
    }
    Ok(result)
}

pub fn list_audio_paths(path: &str) -> AppResult<Vec<String>> {
    let path = PathBuf::from(path);
    if path.is_file() {
        return Ok(vec![path.to_string_lossy().to_string()]);
    }
    Ok(collect_tracks(&path)?
        .into_iter()
        .map(|track| track.path)
        .collect())
}

const PICTURE_FILE_CAP: u64 = 16 * 1024 * 1024;
const COVER_DISPLAY_EDGE: u32 = 512;
const PICTURE_PREVIEW_EDGE: u32 = 480;

/// Read the image from disk and embed it. The webview sends a path, not bytes.
pub fn add_picture_from_path(path: &str, image_path: &str, kind: String) -> AppResult<TagDoc> {
    let image = Path::new(image_path);
    if !image.is_file() {
        return Err(AppError::msg("pick an image to embed"));
    }
    let len = image.metadata()?.len();
    if len == 0 || len > PICTURE_FILE_CAP {
        return Err(AppError::msg("that image is too large to embed"));
    }
    let data = std::fs::read(image)?;
    add_picture(path, data, String::new(), kind)
}

pub fn add_picture(path: &str, data: Vec<u8>, mime: String, kind: String) -> AppResult<TagDoc> {
    let picture = picture_from_bytes(Path::new(path), &data, &mime, &kind)?;
    mutate_tag(Path::new(path), |tag| {
        tag.push_picture(picture);
    })?;
    forget_thumb(path);
    read_tags(path)
}

pub fn remove_picture(path: &str, index: usize) -> AppResult<TagDoc> {
    mutate_tag(Path::new(path), |tag| {
        if index < tag.pictures().len() {
            let _ = tag.remove_picture(index);
        }
    })?;
    forget_thumb(path);
    read_tags(path)
}

pub fn export_picture(path: &str, index: usize, dest: &str) -> AppResult<()> {
    let tagged = read_audio(Path::new(path))?;
    let tag = tagged
        .primary_tag()
        .or_else(|| tagged.first_tag())
        .ok_or_else(|| AppError::msg("that picture is gone"))?;
    let picture = tag
        .pictures()
        .get(index)
        .ok_or_else(|| AppError::msg("that picture is gone"))?;
    std::fs::write(dest, picture.data())?;
    Ok(())
}

pub fn add_custom_field(path: &str, key: String, value: String) -> AppResult<TagDoc> {
    if key.trim().is_empty() {
        return Err(AppError::msg("custom field key is required"));
    }
    mutate_tag(Path::new(path), |tag| {
        let item_key = ItemKey::Unknown(key);
        if value.is_empty() {
            tag.remove_key(&item_key);
        } else {
            let _ = tag.insert(TagItem::new(item_key, ItemValue::Text(value)));
        }
    })?;
    read_tags(path)
}

pub fn remove_custom_field(path: &str, key: String) -> AppResult<TagDoc> {
    mutate_tag(Path::new(path), |tag| {
        tag.remove_key(&ItemKey::Unknown(key.clone()));
        if let Some(known) = known_key(&key) {
            tag.remove_key(&known);
        }
    })?;
    read_tags(path)
}

/// One embedded picture, downscaled for the Tags preview. Bytes are not
/// included in [`TagDoc`] so `read_tags` stays small.
pub fn picture_preview(path: &str, index: usize) -> AppResult<Option<CoverArt>> {
    if !is_audio_path(Path::new(path)) {
        return Ok(None);
    }
    let tagged = read_audio(Path::new(path))?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(None);
    };
    let Some(picture) = tag.pictures().get(index) else {
        return Ok(None);
    };
    let data_base64 = display_base64(picture.data(), PICTURE_PREVIEW_EDGE);
    if data_base64.is_empty() {
        return Ok(None);
    }
    Ok(Some(CoverArt {
        mime: "image/jpeg".into(),
        data_base64,
    }))
}

pub fn cover_for(path: &str) -> AppResult<Option<CoverArt>> {
    if !is_audio_path(Path::new(path)) {
        return Ok(None);
    }
    let tagged = read_audio(Path::new(path))?;
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(None);
    };
    let pictures = tag.pictures();
    let chosen = pictures
        .iter()
        .find(|picture| picture_kind(picture.pic_type()) == "front")
        .or_else(|| pictures.first());
    let Some(picture) = chosen else {
        return Ok(None);
    };
    let data_base64 = display_base64(picture.data(), COVER_DISPLAY_EDGE);
    if data_base64.is_empty() {
        return Ok(None);
    }
    Ok(Some(CoverArt {
        mime: "image/jpeg".into(),
        data_base64,
    }))
}

const THUMB_CACHE_CAP: usize = 256;

fn thumb_cache() -> &'static Mutex<ThumbCache> {
    static CACHE: OnceLock<Mutex<ThumbCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ThumbCache::default()))
}

#[derive(Default)]
struct ThumbCache {
    map: HashMap<String, Option<CoverArt>>,
    order: VecDeque<String>,
}

pub fn cover_thumb(path: &str) -> AppResult<Option<CoverArt>> {
    {
        let mut cache = thumb_cache().lock().expect("cover cache");
        if let Some(hit) = cache.map.get(path).cloned() {
            cache.order.retain(|item| item != path);
            cache.order.push_back(path.to_string());
            return Ok(hit);
        }
    }
    let cover = match cover_for(path)? {
        Some(cover) => shrink_cover(&cover).or(Some(cover)),
        None => None,
    };
    let mut cache = thumb_cache().lock().expect("cover cache");
    cache.order.retain(|item| item != path);
    while cache.map.len() >= THUMB_CACHE_CAP {
        let Some(old) = cache.order.pop_front() else {
            break;
        };
        cache.map.remove(&old);
    }
    cache.order.push_back(path.to_string());
    cache.map.insert(path.to_string(), cover.clone());
    Ok(cover)
}

pub fn forget_thumb(path: &str) {
    let mut cache = thumb_cache().lock().expect("cover cache");
    cache.map.remove(path);
    cache.order.retain(|item| item != path);
}

fn shrink_cover(cover: &CoverArt) -> Option<CoverArt> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&cover.data_base64)
        .ok()?;
    let jpeg = bounded_jpeg(&bytes, 80)?;
    Some(CoverArt {
        mime: "image/jpeg".into(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(jpeg),
    })
}

fn display_base64(data: &[u8], max_edge: u32) -> String {
    if let Some(jpeg) = bounded_jpeg(data, max_edge) {
        return base64::engine::general_purpose::STANDARD.encode(jpeg);
    }
    if data.len() <= 256 * 1024 {
        return base64::engine::general_purpose::STANDARD.encode(data);
    }
    String::new()
}

fn bounded_jpeg(data: &[u8], max_edge: u32) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(32 * 1024 * 1024);
    limits.max_image_width = Some(8_000);
    limits.max_image_height = Some(8_000);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    let image = if image.width() <= max_edge && image.height() <= max_edge {
        image
    } else {
        image.thumbnail(max_edge, max_edge)
    };
    let rgb = image.to_rgb8();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(out.into_inner())
}

pub fn staging_path(path: &Path) -> PathBuf {
    let name = path
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "track".into());
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .unwrap_or("tmp");
    path.with_file_name(format!(".{name}.audios-tmp.{ext}"))
}

const STAGING_MARK: &str = ".audios-tmp.";
const STAGING_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Is this a tag-write staging file left behind by a crash? Only files older
/// than an hour count, so a write in progress on another thread is safe.
pub fn is_stale_staging(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if !name.starts_with('.') || !name.contains(STAGING_MARK) {
        return false;
    }
    path.metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age > STAGING_STALE_AFTER)
}

/// Remove stale staging files next to `path`.
pub fn sweep_stale_staging(path: &Path) {
    let dir = if path.is_dir() {
        path
    } else {
        match path.parent() {
            Some(parent) => parent,
            None => return,
        }
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let candidate = entry.path();
        if is_stale_staging(&candidate) {
            let _ = std::fs::remove_file(&candidate);
        }
    }
}

fn read_audio(path: &Path) -> AppResult<lofty::file::TaggedFile> {
    // BestAttempt still fails the whole file on a bad ID3 TDRC/TYER. Relaxed
    // drops that one frame so the rest of the tag can load. MP4 files with a
    // 64-bit mdat size are adjusted in memory; see mp4_recover.
    crate::mp4_recover::open(path)
}

fn mutate_tag<F>(path: &Path, mutate: F) -> AppResult<()>
where
    F: FnOnce(&mut Tag),
{
    if !path.is_file() {
        return Err(AppError::msg("that path is not a file"));
    }
    sweep_stale_staging(path);
    let tmp = staging_path(path);
    if tmp.exists() {
        std::fs::remove_file(&tmp)?;
    }
    std::fs::copy(path, &tmp)?;
    let result = (|| -> AppResult<()> {
        // A 64-bit `mdat` makes Lofty skip past `moov`. Adjust a copy, then edit that.
        if crate::mp4_recover::read_lofty(&tmp).is_err() {
            if let Some(fixed) = crate::mp4_recover::for_lofty(&tmp)? {
                std::fs::write(&tmp, fixed)?;
            }
        }
        let mut tagged = crate::mp4_recover::read_lofty(&tmp)?;
        if tagged.primary_tag().is_none() && tagged.first_tag().is_none() {
            tagged.insert_tag(Tag::new(tagged.primary_tag_type()));
        }
        let tag = if let Some(tag) = tagged.primary_tag_mut() {
            tag
        } else if let Some(tag) = tagged.first_tag_mut() {
            tag
        } else {
            return Err(AppError::msg("could not create a tag for this file"));
        };
        mutate(tag);
        tagged.save_to_path(&tmp, WriteOptions::default())?;
        Ok(())
    })();
    match result {
        Ok(()) => rename_atomic(&tmp, path),
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            Err(error)
        }
    }
}

fn fields_from_tag(tag: &Tag) -> TagFields {
    TagFields {
        title: text(tag, &ItemKey::TrackTitle).or_else(|| tag.title().map(|v| v.to_string())),
        artists: text(tag, &ItemKey::TrackArtist).or_else(|| tag.artist().map(|v| v.to_string())),
        album: text(tag, &ItemKey::AlbumTitle).or_else(|| tag.album().map(|v| v.to_string())),
        album_artist: text(tag, &ItemKey::AlbumArtist),
        year: tag
            .year()
            .or_else(|| text(tag, &ItemKey::Year).as_deref().and_then(parse_year))
            .or_else(|| {
                text(tag, &ItemKey::RecordingDate)
                    .as_deref()
                    .and_then(parse_year)
            })
            .map(|year| year.to_string()),
        date: text(tag, &ItemKey::RecordingDate)
            .as_deref()
            .and_then(sanitize_release_date)
            .filter(|value| value.len() > 4),
        track: tag
            .track()
            .map(|v| v.to_string())
            .or_else(|| text(tag, &ItemKey::TrackNumber)),
        track_total: tag
            .track_total()
            .map(|v| v.to_string())
            .or_else(|| text(tag, &ItemKey::TrackTotal)),
        disc: tag
            .disk()
            .map(|v| v.to_string())
            .or_else(|| text(tag, &ItemKey::DiscNumber)),
        disc_total: tag
            .disk_total()
            .map(|v| v.to_string())
            .or_else(|| text(tag, &ItemKey::DiscTotal)),
        genre: text(tag, &ItemKey::Genre).or_else(|| tag.genre().map(|v| v.to_string())),
        comment: text(tag, &ItemKey::Comment).or_else(|| tag.comment().map(|v| v.to_string())),
        composer: text(tag, &ItemKey::Composer),
        conductor: text(tag, &ItemKey::Conductor),
        remixer: text(tag, &ItemKey::Remixer),
        grouping: text(tag, &ItemKey::ContentGroup),
        bpm: text(tag, &ItemKey::Bpm),
        key: text(tag, &ItemKey::InitialKey),
        isrc: text(tag, &ItemKey::Isrc),
        barcode: text(tag, &ItemKey::Barcode),
        catalog: text(tag, &ItemKey::CatalogNumber),
        copyright: text(tag, &ItemKey::CopyrightMessage),
        encoder: text(tag, &ItemKey::EncodedBy),
        language: text(tag, &ItemKey::Language),
        compilation: text(tag, &ItemKey::FlagCompilation),
        lyrics: text(tag, &ItemKey::Lyrics),
        synced_lyrics: text(tag, &ItemKey::Unknown("SYNCEDLYRICS".into())),
        replaygain_track_gain: text(tag, &ItemKey::ReplayGainTrackGain),
        replaygain_track_peak: text(tag, &ItemKey::ReplayGainTrackPeak),
        replaygain_album_gain: text(tag, &ItemKey::ReplayGainAlbumGain),
        replaygain_album_peak: text(tag, &ItemKey::ReplayGainAlbumPeak),
        musicbrainz_recording_id: text(tag, &ItemKey::MusicBrainzRecordingId),
        musicbrainz_release_id: text(tag, &ItemKey::MusicBrainzReleaseId),
        musicbrainz_artist_id: text(tag, &ItemKey::MusicBrainzArtistId),
        musicbrainz_release_artist_id: text(tag, &ItemKey::MusicBrainzReleaseArtistId),
        musicbrainz_track_id: text(tag, &ItemKey::MusicBrainzTrackId),
        url: url_from_tag(tag),
    }
}

fn apply_fields(tag: &mut Tag, fields: &TagFields, only: Option<&[String]>) {
    let allow = |name: &str| only.is_none_or(|keys| keys.iter().any(|key| key == name));
    if allow("title") {
        set_text(tag, ItemKey::TrackTitle, &fields.title);
    }
    if allow("artists") {
        set_text(tag, ItemKey::TrackArtist, &fields.artists);
    }
    if allow("album") {
        set_text(tag, ItemKey::AlbumTitle, &fields.album);
    }
    if allow("albumArtist") {
        set_text(tag, ItemKey::AlbumArtist, &fields.album_artist);
    }
    if allow("year") || allow("date") {
        apply_release_time(
            tag,
            if allow("year") {
                fields.year.as_deref()
            } else {
                None
            },
            if allow("date") {
                fields.date.as_deref()
            } else {
                None
            },
            allow("year"),
            allow("date"),
        );
    }
    if allow("track") {
        set_text(tag, ItemKey::TrackNumber, &fields.track);
        set_number(
            tag,
            fields.track.as_deref(),
            |tag, value| tag.set_track(value),
            |tag| tag.remove_track(),
        );
    }
    if allow("trackTotal") {
        set_text(tag, ItemKey::TrackTotal, &fields.track_total);
        set_number(
            tag,
            fields.track_total.as_deref(),
            |tag, value| tag.set_track_total(value),
            |tag| tag.remove_track_total(),
        );
    }
    if allow("disc") {
        set_text(tag, ItemKey::DiscNumber, &fields.disc);
        set_number(
            tag,
            fields.disc.as_deref(),
            |tag, value| tag.set_disk(value),
            |tag| tag.remove_disk(),
        );
    }
    if allow("discTotal") {
        set_text(tag, ItemKey::DiscTotal, &fields.disc_total);
        set_number(
            tag,
            fields.disc_total.as_deref(),
            |tag, value| tag.set_disk_total(value),
            |tag| tag.remove_disk_total(),
        );
    }
    if allow("genre") {
        set_text(tag, ItemKey::Genre, &fields.genre);
    }
    if allow("comment") {
        set_text(tag, ItemKey::Comment, &fields.comment);
    }
    if allow("composer") {
        set_text(tag, ItemKey::Composer, &fields.composer);
    }
    if allow("conductor") {
        set_text(tag, ItemKey::Conductor, &fields.conductor);
    }
    if allow("remixer") {
        set_text(tag, ItemKey::Remixer, &fields.remixer);
    }
    if allow("grouping") {
        set_text(tag, ItemKey::ContentGroup, &fields.grouping);
    }
    if allow("bpm") {
        set_text(tag, ItemKey::Bpm, &fields.bpm);
    }
    if allow("key") {
        set_text(tag, ItemKey::InitialKey, &fields.key);
    }
    if allow("isrc") {
        set_text(tag, ItemKey::Isrc, &fields.isrc);
    }
    if allow("barcode") {
        set_text(tag, ItemKey::Barcode, &fields.barcode);
    }
    if allow("catalog") {
        set_text(tag, ItemKey::CatalogNumber, &fields.catalog);
    }
    if allow("copyright") {
        set_text(tag, ItemKey::CopyrightMessage, &fields.copyright);
    }
    if allow("encoder") {
        set_text(tag, ItemKey::EncodedBy, &fields.encoder);
    }
    if allow("language") {
        set_text(tag, ItemKey::Language, &fields.language);
    }
    if allow("compilation") {
        set_text(tag, ItemKey::FlagCompilation, &fields.compilation);
    }
    if allow("lyrics") {
        set_text(tag, ItemKey::Lyrics, &fields.lyrics);
    }
    if allow("syncedLyrics") {
        set_text(
            tag,
            ItemKey::Unknown("SYNCEDLYRICS".into()),
            &fields.synced_lyrics,
        );
    }
    if allow("replaygainTrackGain") {
        set_text(
            tag,
            ItemKey::ReplayGainTrackGain,
            &fields.replaygain_track_gain,
        );
    }
    if allow("replaygainTrackPeak") {
        set_text(
            tag,
            ItemKey::ReplayGainTrackPeak,
            &fields.replaygain_track_peak,
        );
    }
    if allow("replaygainAlbumGain") {
        set_text(
            tag,
            ItemKey::ReplayGainAlbumGain,
            &fields.replaygain_album_gain,
        );
    }
    if allow("replaygainAlbumPeak") {
        set_text(
            tag,
            ItemKey::ReplayGainAlbumPeak,
            &fields.replaygain_album_peak,
        );
    }
    if allow("musicbrainzRecordingId") {
        set_text(
            tag,
            ItemKey::MusicBrainzRecordingId,
            &fields.musicbrainz_recording_id,
        );
    }
    if allow("musicbrainzReleaseId") {
        set_text(
            tag,
            ItemKey::MusicBrainzReleaseId,
            &fields.musicbrainz_release_id,
        );
    }
    if allow("musicbrainzArtistId") {
        set_text(
            tag,
            ItemKey::MusicBrainzArtistId,
            &fields.musicbrainz_artist_id,
        );
    }
    if allow("musicbrainzReleaseArtistId") {
        set_text(
            tag,
            ItemKey::MusicBrainzReleaseArtistId,
            &fields.musicbrainz_release_artist_id,
        );
    }
    if allow("musicbrainzTrackId") {
        set_text(
            tag,
            ItemKey::MusicBrainzTrackId,
            &fields.musicbrainz_track_id,
        );
    }
    if allow("url") {
        set_url(tag, &fields.url);
    }
}

fn pictures_from_tag(tag: &Tag) -> Vec<PictureInfo> {
    tag.pictures()
        .iter()
        .enumerate()
        .map(|(index, picture)| PictureInfo {
            index,
            kind: picture_kind(picture.pic_type()),
            mime: picture
                .mime_type()
                .map(|mime| mime.as_str().to_string())
                .unwrap_or_else(|| "image/jpeg".into()),
            size: picture.data().len(),
        })
        .collect()
}

fn raw_from_tag(tag: &Tag) -> Vec<RawItem> {
    tag.items()
        .map(|item| {
            let key = item_key_label(item.key());
            let known = KNOWN_KEYS.contains(item.key())
                || matches!(
                    item.key(),
                    ItemKey::TrackTitle | ItemKey::TrackArtist | ItemKey::AlbumTitle
                );
            let value = match item.value() {
                ItemValue::Text(value) | ItemValue::Locator(value) => value.clone(),
                ItemValue::Binary(bytes) => format!("<binary {} bytes>", bytes.len()),
            };
            RawItem { key, value, known }
        })
        .collect()
}

fn text(tag: &Tag, key: &ItemKey) -> Option<String> {
    tag.get_string(key).map(ToOwned::to_owned)
}

fn url_from_tag(tag: &Tag) -> Option<String> {
    text(tag, &ItemKey::AudioFileUrl)
        .or_else(|| text(tag, &ItemKey::AudioSourceUrl))
        .or_else(|| {
            tag.items().find_map(|item| match item.key() {
                ItemKey::Unknown(name)
                    if name.eq_ignore_ascii_case("WWW") || name.eq_ignore_ascii_case("URL") =>
                {
                    match item.value() {
                        ItemValue::Text(value) | ItemValue::Locator(value) => Some(value.clone()),
                        _ => None,
                    }
                }
                _ => None,
            })
        })
}

fn set_url(tag: &mut Tag, value: &Option<String>) {
    set_text(tag, ItemKey::AudioSourceUrl, value);
}

fn apply_release_time(
    tag: &mut Tag,
    year: Option<&str>,
    date: Option<&str>,
    touch_year: bool,
    touch_date: bool,
) {
    let date = if touch_date {
        date.and_then(sanitize_release_date)
    } else {
        text(tag, &ItemKey::RecordingDate)
            .as_deref()
            .and_then(sanitize_release_date)
    };
    let year = if touch_year {
        year.and_then(parse_year)
    } else {
        tag.year()
            .or_else(|| text(tag, &ItemKey::Year).as_deref().and_then(parse_year))
            .or_else(|| date.as_deref().and_then(parse_year))
    };

    if touch_year || touch_date {
        tag.remove_key(&ItemKey::Year);
        tag.remove_key(&ItemKey::RecordingDate);
        tag.remove_year();
    }

    // ID3 TDRC is a timestamp. Year-only ("2024") is valid. Do not call
    // Tag::set_year — it splices onto any existing RecordingDate leftover.
    if let Some(date) = date {
        let _ = tag.insert_text(ItemKey::RecordingDate, date);
        return;
    }
    if let Some(year) = year {
        let value = year.to_string();
        let _ = tag.insert_text(ItemKey::Year, value.clone());
        let _ = tag.insert_text(ItemKey::RecordingDate, value);
    }
}

pub fn parse_year(value: &str) -> Option<u32> {
    let digits: String = value
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(4)
        .collect();
    if digits.len() != 4 {
        return None;
    }
    let year: u32 = digits.parse().ok()?;
    (1000..=9999).contains(&year).then_some(year)
}

pub fn sanitize_release_date(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed.replace(['/', '.'], "-");
    let date = normalized
        .split([' ', 'T'])
        .next()
        .unwrap_or(&normalized)
        .trim();
    if let Some(year) = parse_year(date) {
        if date.len() == 4 {
            return Some(year.to_string());
        }
    }
    let parts: Vec<&str> = date.split('-').filter(|part| !part.is_empty()).collect();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| !part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return parse_year(trimmed).map(|year| year.to_string());
    }
    match parts.as_slice() {
        [year] => parse_year(year).map(|year| year.to_string()),
        [year, month] if month.len() <= 2 => {
            let year = parse_year(year)?;
            let month: u32 = month.parse().ok()?;
            (1..=12)
                .contains(&month)
                .then_some(format!("{year:04}-{month:02}"))
        }
        [year, month, day, ..] if month.len() <= 2 && day.len() <= 2 => {
            let year = parse_year(year)?;
            let month: u32 = month.parse().ok()?;
            let day: u32 = day.parse().ok()?;
            ((1..=12).contains(&month) && (1..=31).contains(&day))
                .then_some(format!("{year:04}-{month:02}-{day:02}"))
        }
        _ => parse_year(trimmed).map(|year| year.to_string()),
    }
}

fn set_text(tag: &mut Tag, key: ItemKey, value: &Option<String>) {
    let Some(value) = value else {
        return;
    };
    if value.trim().is_empty() {
        tag.remove_key(&key);
        return;
    }
    let _ = tag.insert_text(key, value.clone());
}

fn set_number(tag: &mut Tag, value: Option<&str>, set: fn(&mut Tag, u32), remove: fn(&mut Tag)) {
    match value {
        None => {}
        Some("") => remove(tag),
        Some(raw) => {
            if let Ok(parsed) = raw.parse() {
                set(tag, parsed);
            }
        }
    }
}

fn item_key_label(key: &ItemKey) -> String {
    match key {
        ItemKey::Unknown(name) => name.to_string(),
        other => format!("{other:?}"),
    }
}

fn known_key(name: &str) -> Option<ItemKey> {
    KNOWN_KEYS
        .iter()
        .find(|key| format!("{key:?}") == name)
        .cloned()
}

fn picture_kind(kind: PictureType) -> String {
    match kind {
        PictureType::CoverFront => "front".into(),
        PictureType::CoverBack => "back".into(),
        PictureType::Artist => "artist".into(),
        PictureType::Leaflet => "leaflet".into(),
        _ => "other".into(),
    }
}

fn picture_type_from(kind: &str) -> PictureType {
    match kind.to_ascii_lowercase().as_str() {
        "front" => PictureType::CoverFront,
        "back" => PictureType::CoverBack,
        "artist" => PictureType::Artist,
        "leaflet" => PictureType::Leaflet,
        _ => PictureType::Other,
    }
}

fn mime_from(mime: &str) -> MimeType {
    MimeType::from_str(mime)
}

fn picture_from_bytes(path: &Path, data: &[u8], mime_hint: &str, kind: &str) -> AppResult<Picture> {
    if data.is_empty() {
        return Err(AppError::msg("picture data was empty"));
    }
    let (bytes, mime) = normalize_picture(path, data, mime_hint)?;
    Ok(Picture::new_unchecked(
        picture_type_from(kind),
        Some(mime),
        None,
        bytes,
    ))
}

fn normalize_picture(path: &Path, data: &[u8], mime_hint: &str) -> AppResult<(Vec<u8>, MimeType)> {
    let sniffed = sniff_image_mime(data).or_else(|| {
        let hint = mime_hint.trim().to_ascii_lowercase();
        hint.starts_with("image/").then(|| mime_from(&hint))
    });
    if wants_jpeg_cover(path) {
        if matches!(sniffed, Some(MimeType::Jpeg)) {
            return Ok((data.to_vec(), MimeType::Jpeg));
        }
        return encode_jpeg(data);
    }
    match sniffed {
        Some(mime @ (MimeType::Jpeg | MimeType::Png)) => Ok((data.to_vec(), mime)),
        _ => encode_jpeg(data),
    }
}

fn wants_jpeg_cover(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "m4a" | "m4b" | "mp4" | "aac"
    )
}

pub fn sniff_image_mime(data: &[u8]) -> Option<MimeType> {
    if data.len() >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return Some(MimeType::Jpeg);
    }
    if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(MimeType::Png);
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some(MimeType::Gif);
    }
    if data.starts_with(b"BM") {
        return Some(MimeType::Bmp);
    }
    if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        return Some(MimeType::Unknown("image/webp".into()));
    }
    None
}

fn encode_jpeg(data: &[u8]) -> AppResult<(Vec<u8>, MimeType)> {
    let image = image::load_from_memory(data)
        .map_err(|_| AppError::msg("that image could not be read"))?
        .to_rgb8();
    let mut out = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
        .encode(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|_| AppError::msg("could not convert that image for the tag"))?;
    Ok((out.into_inner(), MimeType::Jpeg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_mdat_is_not_reported_as_a_missing_moov() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.m4a");
        std::fs::write(&path, extended_mdat_bytes()).unwrap();
        let error = read_tags(path.to_str().unwrap()).unwrap_err().to_string();
        assert!(!error.to_ascii_lowercase().contains("moov"), "{error}");
    }

    fn extended_mdat_bytes() -> Vec<u8> {
        fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(payload);
            out
        }
        fn extended(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let size = 16 + payload.len();
            let mut out = Vec::new();
            out.extend_from_slice(&1u32.to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(&(size as u64).to_be_bytes());
            out.extend_from_slice(payload);
            out
        }
        let ftyp = atom(b"ftyp", b"M4A \0\0\0\0M4A mp42");
        let mdat = extended(b"mdat", &[0u8; 32]);
        let moov = atom(b"moov", &atom(b"mvhd", &[0u8; 32]));
        let mut file = ftyp;
        file.extend_from_slice(&mdat);
        file.extend_from_slice(&moov);
        file
    }

    #[test]
    fn picture_round_trip_kinds() {
        assert_eq!(picture_kind(picture_type_from("front")), "front");
        assert_eq!(picture_kind(picture_type_from("leaflet")), "leaflet");
        let info = PictureInfo {
            index: 0,
            kind: "front".into(),
            mime: "image/jpeg".into(),
            size: 4_000_000,
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("dataBase64").is_none());
        assert_eq!(json["size"], 4_000_000);
    }

    #[test]
    fn staging_path_keeps_audio_extension() {
        let path = Path::new("/music/Album/01 song.mp3");
        let staged = staging_path(path);
        assert_eq!(
            staged.file_name().and_then(|name| name.to_str()),
            Some(".01 song.audios-tmp.mp3")
        );
        assert_eq!(staged.extension().and_then(|ext| ext.to_str()), Some("mp3"));
        assert!(lofty::file::FileType::from_path(&staged).is_some());
        assert!(
            lofty::file::FileType::from_path(Path::new("/music/.01 song.mp3.audios-tmp")).is_none()
        );
    }

    #[test]
    fn batch_write_keeps_going_past_a_bad_file() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("missing.mp3");
        let not_audio = dir.path().join("notes.mp3");
        std::fs::write(&not_audio, b"this is not an mp3").unwrap();
        let fields = TagFields {
            title: Some("New".into()),
            ..TagFields::default()
        };
        let result = batch_write(
            vec![
                bad.to_string_lossy().into(),
                not_audio.to_string_lossy().into(),
            ],
            fields,
            vec!["title".into()],
        )
        .unwrap();
        assert_eq!(result.written, 0);
        assert_eq!(result.failed.len(), 2);
        assert!(result.failed[0].path.ends_with("missing.mp3"));
        assert!(!result.failed[1].error.is_empty());
    }

    #[test]
    fn stale_staging_files_are_swept_but_fresh_ones_kept() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("song.mp3");
        std::fs::write(&song, b"x").unwrap();
        let stale = staging_path(&song);
        std::fs::write(&stale, b"half").unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 60 * 60);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        let fresh = dir.path().join(".other.audios-tmp.flac");
        std::fs::write(&fresh, b"in progress").unwrap();
        let unrelated = dir.path().join(".hidden.mp3");
        std::fs::write(&unrelated, b"keep").unwrap();
        assert!(is_stale_staging(&stale));
        assert!(!is_stale_staging(&fresh));
        assert!(!is_stale_staging(&unrelated));
        sweep_stale_staging(&song);
        assert!(!stale.exists());
        assert!(fresh.exists());
        assert!(unrelated.exists());
        assert!(song.exists());
    }

    #[test]
    fn year_only_is_a_valid_release_time() {
        assert_eq!(parse_year("2024"), Some(2024));
        assert_eq!(parse_year(" 2024 "), Some(2024));
        assert_eq!(sanitize_release_date(""), None);
        assert_eq!(sanitize_release_date("2024"), Some("2024".into()));
        assert_eq!(
            sanitize_release_date("2024/08/19"),
            Some("2024-08-19".into())
        );
        assert_eq!(sanitize_release_date("2024-08"), Some("2024-08".into()));
        assert_eq!(sanitize_release_date("August 2024"), Some("2024".into()));
        assert_eq!(sanitize_release_date("n.d."), None);

        let mut tag = Tag::new(lofty::tag::TagType::Id3v2);
        apply_release_time(&mut tag, Some("2024"), Some(""), true, true);
        assert_eq!(tag.get_string(&ItemKey::RecordingDate), Some("2024"));
        apply_release_time(&mut tag, Some("2024"), Some("2024/08/19"), true, true);
        assert_eq!(tag.get_string(&ItemKey::RecordingDate), Some("2024-08-19"));
    }

    #[test]
    fn sniff_jpeg_and_png_magic() {
        assert_eq!(
            sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(MimeType::Jpeg)
        );
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(sniff_image_mime(&png), Some(MimeType::Png));
        assert_eq!(sniff_image_mime(b"not-an-image"), None);
    }
}

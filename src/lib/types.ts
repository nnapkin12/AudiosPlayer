import type { CustomTheme } from "./theme";
import type { EqState } from "./eq";

export type { CustomTheme, ThemeColors } from "./theme";
export type { EqBuiltin, EqState, EqUpdate, EqUserPreset } from "./eq";

export type RepeatMode = "off" | "one" | "all";

export interface PlaylistItem {
  path: string;
  kind: "file" | "dir" | string;
}

export interface Playlist {
  id: string;
  name: string;
  items: PlaylistItem[];
  hasCover?: boolean;
}

export type BrowsePage =
  | { kind: "home" }
  | { kind: "discover" }
  | { kind: "library" }
  | { kind: "playlists" }
  | { kind: "artist"; name: string; all?: boolean }
  | { kind: "album"; artist: string; album: string }
  | { kind: "folder"; path: string }
  | { kind: "playlist"; id: string };

export interface Appearance {
  theme: string;
  accent: string;
  customThemes: CustomTheme[];
  minimizeMovement?: boolean;
}

export interface MediaHit {
  title: string;
  url: string;
  pageUrl?: string;
  thumbnailUrl?: string;
  channel?: string;
}

export interface MissingItem {
  scope: "playlist" | "library" | string;
  id: string;
  path: string;
  kind: "file" | "dir" | string;
  label: string;
}

export interface LibraryChange {
  roots: string[];
  path: string;
}

export interface Track {
  path: string;
  title: string;
  artist: string;
  album: string;
  albumArtist: string;
  track: number | null;
  disc: number | null;
  durationMs: number;
  folder: string;
  replaygainTrack: number | null;
  replaygainAlbum: number | null;
}

export interface FolderNode {
  name: string;
  path: string;
  kind: "dir" | "file" | string;
  children: FolderNode[];
}

export interface PlayerSnapshot {
  current: Track | null;
  index: number;
  queue: Track[];
  playing: boolean;
  positionMs: number;
  durationMs: number;
  volume: number;
  muted: boolean;
  repeat: RepeatMode;
  shuffle: boolean;
  replaygain: boolean;
  gapless: boolean;
  speed: number;
  sampleRate?: number;
  eq?: EqState;
  root: string | null;
  tree: FolderNode | null;
  error: string | null;
}

export interface Tick {
  positionMs: number;
  durationMs: number;
}

export interface RemoteClient {
  id: number;
  name: string;
  connected: boolean;
}

export interface RemoteStatus {
  running: boolean;
  url: string | null;
  urls: string[];
  code: string | null;
  qrSvg: string | null;
  indexing: boolean;
  songs: number;
  clients: RemoteClient[];
}

export interface TagFields {
  title: string;
  artists: string;
  album: string;
  albumArtist: string;
  year: string;
  date: string;
  track: string;
  trackTotal: string;
  disc: string;
  discTotal: string;
  genre: string;
  comment: string;
  composer: string;
  conductor: string;
  remixer: string;
  grouping: string;
  bpm: string;
  key: string;
  isrc: string;
  barcode: string;
  catalog: string;
  copyright: string;
  encoder: string;
  language: string;
  compilation: string;
  lyrics: string;
  syncedLyrics: string;
  replaygainTrackGain: string;
  replaygainTrackPeak: string;
  replaygainAlbumGain: string;
  replaygainAlbumPeak: string;
  musicbrainzRecordingId: string;
  musicbrainzReleaseId: string;
  musicbrainzArtistId: string;
  musicbrainzReleaseArtistId: string;
  musicbrainzTrackId: string;
  url: string;
}

export interface PictureInfo {
  index: number;
  kind: string;
  mime: string;
  size: number;
  dataBase64: string;
}

export interface RawItem {
  key: string;
  value: string;
  known: boolean;
}

export interface TagDoc {
  path: string;
  format: string;
  tagType: string;
  durationMs: number;
  sampleRate: number | null;
  channels: number | null;
  bitrateKbps: number | null;
  bitDepth: number | null;
  fields: Partial<TagFields>;
  pictures: PictureInfo[];
  raw: RawItem[];
}

export interface CoverArt {
  mime: string;
  dataBase64: string;
}

export const EMPTY_FIELDS: TagFields = {
  title: "",
  artists: "",
  album: "",
  albumArtist: "",
  year: "",
  date: "",
  track: "",
  trackTotal: "",
  disc: "",
  discTotal: "",
  genre: "",
  comment: "",
  composer: "",
  conductor: "",
  remixer: "",
  grouping: "",
  bpm: "",
  key: "",
  isrc: "",
  barcode: "",
  catalog: "",
  copyright: "",
  encoder: "",
  language: "",
  compilation: "",
  lyrics: "",
  syncedLyrics: "",
  replaygainTrackGain: "",
  replaygainTrackPeak: "",
  replaygainAlbumGain: "",
  replaygainAlbumPeak: "",
  musicbrainzRecordingId: "",
  musicbrainzReleaseId: "",
  musicbrainzArtistId: "",
  musicbrainzReleaseArtistId: "",
  musicbrainzTrackId: "",
  url: "",
};

export const COMMON_FIELDS: Array<[keyof TagFields, string]> = [
  ["title", "Title"],
  ["artists", "Artists"],
  ["album", "Album"],
  ["albumArtist", "Album artist"],
  ["year", "Year"],
  ["date", "Date"],
  ["track", "Track"],
  ["trackTotal", "Track total"],
  ["disc", "Disc"],
  ["discTotal", "Disc total"],
  ["genre", "Genre"],
  ["comment", "Comment"],
];

export const EXTENDED_FIELDS: Array<[keyof TagFields, string]> = [
  ["composer", "Composer"],
  ["conductor", "Conductor"],
  ["remixer", "Remixer"],
  ["grouping", "Grouping"],
  ["bpm", "BPM"],
  ["key", "Key"],
  ["isrc", "ISRC"],
  ["barcode", "Barcode"],
  ["catalog", "Catalog"],
  ["copyright", "Copyright"],
  ["encoder", "Encoder"],
  ["language", "Language"],
  ["compilation", "Compilation"],
  ["url", "URL"],
  ["replaygainTrackGain", "ReplayGain track"],
  ["replaygainTrackPeak", "ReplayGain track peak"],
  ["replaygainAlbumGain", "ReplayGain album"],
  ["replaygainAlbumPeak", "ReplayGain album peak"],
  ["musicbrainzRecordingId", "MusicBrainz recording"],
  ["musicbrainzReleaseId", "MusicBrainz release"],
  ["musicbrainzArtistId", "MusicBrainz artist"],
  ["musicbrainzReleaseArtistId", "MusicBrainz release artist"],
  ["musicbrainzTrackId", "MusicBrainz track"],
];

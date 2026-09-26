import type { EqUpdate, EqUserPreset } from "./eq";
import type {
  Appearance,
  CoverArt,
  LibraryChange,
  MediaHit,
  MissingItem,
  Playlist,
  PlayerSnapshot,
  RemoteStatus,
  RepeatMode,
  TagDoc,
  TagFields,
  Track,
} from "./types";
import type { CustomTheme } from "./theme";

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new Error("Audios! needs the desktop shell. Run npm run tauri dev.");
  }
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export async function listen<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!isTauri()) {
    return () => undefined;
  }
  const { listen: tauriListen } = await import("@tauri-apps/api/event");
  const unlisten = await tauriListen<T>(event, (event) => handler(event.payload));
  return unlisten;
}

export async function pickAudioFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: true,
    directory: false,
    title: "Add songs",
    filters: [
      {
        name: "Audio",
        extensions: [
          "mp3",
          "flac",
          "ogg",
          "opus",
          "m4a",
          "mp4",
          "aac",
          "wav",
          "aiff",
          "wma",
          "wv",
          "ape",
        ],
      },
    ],
  });
  if (Array.isArray(selected)) return selected;
  return typeof selected === "string" ? [selected] : [];
}

export async function pickAudioFile(title = "Play song"): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: false,
    directory: false,
    title,
    filters: [
      {
        name: "Audio",
        extensions: [
          "mp3",
          "flac",
          "ogg",
          "opus",
          "m4a",
          "mp4",
          "aac",
          "wav",
          "aiff",
          "wma",
          "wv",
          "ape",
        ],
      },
    ],
  });
  return typeof selected === "string" ? selected : null;
}

export async function pickImageFile(): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: false,
    directory: false,
    title: "Choose picture",
    filters: [
      {
        name: "Images",
        extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"],
      },
    ],
  });
  return typeof selected === "string" ? selected : null;
}

export async function pickFolder(title = "Add music"): Promise<string | null> {
  if (!isTauri()) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    multiple: false,
    directory: true,
    title,
  });
  return typeof selected === "string" ? selected : null;
}

export async function pickSavePath(
  defaultName: string,
  title = "Export artwork",
): Promise<string | null> {
  if (!isTauri()) return null;
  const { save } = await import("@tauri-apps/plugin-dialog");
  const selected = await save({
    title,
    defaultPath: defaultName,
  });
  return selected ?? null;
}

export const api = {
  state: () => invoke<PlayerSnapshot>("player_state"),
  openPath: (path: string) => invoke<PlayerSnapshot>("open_path", { path }),
  play: () => invoke<PlayerSnapshot>("play"),
  pause: () => invoke<PlayerSnapshot>("pause"),
  toggle: () => invoke<PlayerSnapshot>("toggle"),
  stop: () => invoke<PlayerSnapshot>("stop"),
  next: () => invoke<PlayerSnapshot>("next_track"),
  previous: () => invoke<PlayerSnapshot>("previous_track"),
  seek: (positionMs: number) => invoke<PlayerSnapshot>("seek", { positionMs }),
  playIndex: (index: number) => invoke<PlayerSnapshot>("play_index", { index }),
  playPath: (path: string) => invoke<PlayerSnapshot>("play_path", { path }),
  playQueuePaths: (paths: string[], startPath?: string) =>
    invoke<PlayerSnapshot>("play_queue_paths", { paths, startPath: startPath ?? null }),
  playTracks: (tracks: Track[], startPath?: string) =>
    invoke<PlayerSnapshot>("play_tracks", { tracks, startPath: startPath ?? null }),
  playPlaylist: (id: string, startPath?: string) =>
    invoke<PlayerSnapshot>("play_playlist", { id, startPath: startPath ?? null }),
  scanTracks: (path: string, fast = false) =>
    invoke<Track[]>("scan_tracks", { path, fast }),
  scanPlaylist: (id: string, fast = false) =>
    invoke<Track[]>("scan_playlist", { id, fast }),
  listLibraryRoots: () => invoke<string[]>("list_library_roots"),
  listMissing: () => invoke<MissingItem[]>("list_missing"),
  relinkMissing: (scope: string, id: string, path: string, newPath: string) =>
    invoke<void>("relink_missing", { scope, id, path, newPath }),
  addLibraryRoot: (path: string) => invoke<LibraryChange>("add_library_root", { path }),
  removeLibraryRoot: (path: string) => invoke<string[]>("remove_library_root", { path }),
  setVolume: (volume: number) => invoke<PlayerSnapshot>("set_volume", { volume }),
  setMuted: (muted: boolean) => invoke<PlayerSnapshot>("set_muted", { muted }),
  setRepeat: (repeat: RepeatMode) => invoke<PlayerSnapshot>("set_repeat", { repeat }),
  setShuffle: (shuffle: boolean) => invoke<PlayerSnapshot>("set_shuffle", { shuffle }),
  setReplaygain: (enabled: boolean) => invoke<PlayerSnapshot>("set_replaygain", { enabled }),
  setGapless: (enabled: boolean) => invoke<PlayerSnapshot>("set_gapless", { enabled }),
  setSpeed: (speed: number) => invoke<PlayerSnapshot>("set_speed", { speed }),
  setEq: (eq: EqUpdate) => invoke<PlayerSnapshot>("set_eq", { eq }),
  saveCustomEq: (preset: EqUserPreset) => invoke<PlayerSnapshot>("save_custom_eq", { preset }),
  deleteCustomEq: (id: string) => invoke<PlayerSnapshot>("delete_custom_eq", { id }),
  importParametricEq: (text: string) => invoke<PlayerSnapshot>("import_parametric_eq", { text }),
  readTags: (path: string) => invoke<TagDoc>("read_tags", { path }),
  writeTags: (path: string, fields: TagFields) =>
    invoke<TagDoc>("write_tags", { path, fields }),
  batchWrite: (paths: string[], fields: TagFields, apply: string[]) =>
    invoke<number>("batch_write", { paths, fields, apply }),
  listAudioPaths: (path: string) => invoke<string[]>("list_audio_paths", { path }),
  addPicture: (path: string, data: number[], mime: string, kind: string) =>
    invoke<TagDoc>("add_picture", { path, data, mime, kind }),
  removePicture: (path: string, index: number) =>
    invoke<TagDoc>("remove_picture", { path, index }),
  exportPicture: (path: string, index: number, dest: string) =>
    invoke<void>("export_picture", { path, index, dest }),
  addCustomField: (path: string, key: string, value: string) =>
    invoke<TagDoc>("add_custom_field", { path, key, value }),
  removeCustomField: (path: string, key: string) =>
    invoke<TagDoc>("remove_custom_field", { path, key }),
  refreshTracks: (paths: string[]) => invoke<Track[]>("refresh_metadata", { paths }),
  coverArt: (path: string) => invoke<CoverArt | null>("cover_art", { path }),
  coverThumb: (path: string) => invoke<CoverArt | null>("cover_thumb", { path }),
  listPlaylists: () => invoke<Playlist[]>("list_playlists"),
  createPlaylist: (name: string) => invoke<Playlist[]>("create_playlist", { name }),
  renamePlaylist: (id: string, name: string) =>
    invoke<Playlist[]>("rename_playlist", { id, name }),
  deletePlaylist: (id: string) => invoke<Playlist[]>("delete_playlist", { id }),
  addToPlaylist: (id: string, paths: string[]) =>
    invoke<Playlist[]>("add_to_playlist", { id, paths }),
  removeFromPlaylist: (id: string, path: string) =>
    invoke<Playlist[]>("remove_from_playlist", { id, path }),
  setPlaylistCover: (id: string, path: string) =>
    invoke<Playlist[]>("set_playlist_cover", { id, path }),
  clearPlaylistCover: (id: string) => invoke<Playlist[]>("clear_playlist_cover", { id }),
  playlistCover: (id: string) => invoke<CoverArt | null>("playlist_cover", { id }),
  setArtistImage: (key: string, path: string) =>
    invoke<void>("set_artist_image", { key, path }),
  artistImage: (key: string) => invoke<CoverArt | null>("artist_image", { key }),
  getAppearance: () => invoke<Appearance>("get_appearance"),
  setAppearance: (theme: string, accent: string) =>
    invoke<Appearance>("set_appearance", { theme, accent }),
  setMinimizeMovement: (enabled: boolean) =>
    invoke<Appearance>("set_minimize_movement", { enabled }),
  saveCustomTheme: (theme: CustomTheme) =>
    invoke<Appearance>("save_custom_theme", { theme }),
  deleteCustomTheme: (id: string) => invoke<Appearance>("delete_custom_theme", { id }),
  remoteStatus: () => invoke<RemoteStatus>("remote_status"),
  remoteStart: () => invoke<RemoteStatus>("remote_start"),
  remoteStop: () => invoke<RemoteStatus>("remote_stop"),
  searchStream: (query: string) => invoke<MediaHit>("search_stream", { query }),
  searchMedia: (query: string) => invoke<MediaHit[]>("search_media", { query }),
  searchCovers: (query: string) => invoke<MediaHit[]>("search_covers", { query }),
  addCoverFromUrl: (path: string, url: string, kind: string) =>
    invoke<TagDoc>("add_cover_from_url", { path, url, kind }),
  playMedia: (title: string, url: string, pageUrl?: string) =>
    invoke<PlayerSnapshot>("play_media", { title, url, pageUrl: pageUrl ?? null }),
  saveMedia: (url: string, dest: string, pageUrl?: string) =>
    invoke<string>("save_media", { url, dest, pageUrl: pageUrl ?? null }),
};

export async function openExternal(url: string): Promise<void> {
  if (!isTauri()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

export async function revealInFiles(path: string): Promise<void> {
  if (!isTauri()) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

export async function windowAction(action: "minimize" | "toggleMaximize" | "close"): Promise<void> {
  if (!isTauri()) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const window = getCurrentWindow();
  if (action === "minimize") await window.minimize();
  if (action === "toggleMaximize") await window.toggleMaximize();
  if (action === "close") await window.close();
}

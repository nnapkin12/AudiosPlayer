import { api, pickAudioFile, pickFolder } from "@/lib/api";
import { baseName, errorMessage } from "@/lib/format";
import {
  cacheKey,
  dropCachedTracks,
  getCachedTracks,
  patchCachedTracks,
  setCachedTracks,
} from "@/lib/browseCache";
import { dropDiscoverTracks } from "@/features/player/discoverCache";
import { dropCover } from "@/lib/covers";
import type { BrowsePage, MissingItem, Track } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

export function filterTracks(tracks: Track[], query: string): Track[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return tracks;
  return tracks.filter((track) => {
    const file = baseName(track.path);
    const stem = file.replace(/\.[^.]+$/, "");
    const hay = [track.title, track.artist, track.albumArtist, track.album, file, stem]
      .join(" ")
      .toLowerCase();
    return hay.includes(needle);
  });
}

export function samePage(left: BrowsePage, right: BrowsePage): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "folder" && right.kind === "folder") return left.path === right.path;
  if (left.kind === "playlist" && right.kind === "playlist") return left.id === right.id;
  if (left.kind === "artist" && right.kind === "artist") return left.name === right.name;
  if (left.kind === "album" && right.kind === "album") {
    return left.artist === right.artist && left.album === right.album;
  }
  return left.kind === right.kind;
}

export function applyTrackMeta(tracks: Track[]): void {
  if (tracks.length === 0) return;
  patchCachedTracks(tracks);
  dropDiscoverTracks();
  const byPath = new Map(tracks.map((track) => [track.path, track]));
  const store = useAppStore.getState();
  store.setPageTracks(store.pageTracks.map((track) => byPath.get(track.path) ?? track));
  for (const track of tracks) dropCover(track.path);
  const current = store.snapshot?.current?.path;
  if (current && byPath.has(current)) void store.refreshCover(current);
}

export function invalidateBrowse(kind: "folder" | "playlist", id: string): void {
  dropCachedTracks(cacheKey(kind, id));
}

export async function locateMissing(item: MissingItem, asFolder = false): Promise<void> {
  const picked =
    item.kind === "dir" || asFolder
      ? await pickFolder("Locate album")
      : await pickAudioFile("Locate song");
  if (!picked) return;
  await api.relinkMissing(item.scope, item.id, item.path, picked);
  const [playlists, roots, missing] = await Promise.all([
    api.listPlaylists(),
    api.listLibraryRoots(),
    api.listMissing(),
  ]);
  const store = useAppStore.getState();
  store.setPlaylists(playlists);
  store.setLibraryRoots(roots);
  store.setMissing(missing);
  const browse = store.browse;
  if (
    browse.kind === "folder" &&
    item.scope === "library" &&
    browse.path === item.path &&
    item.kind === "dir"
  ) {
    await openBrowsePage({ kind: "folder", path: picked }, true);
    return;
  }
  if (browse.kind !== "home") await openBrowsePage(browse, true);
}

let browseGeneration = 0;
const browsePast: BrowsePage[] = [];

export function openBrowsePage(page: BrowsePage, force = false, record = true): Promise<void> {
  const store = useAppStore.getState();
  if (record && !samePage(store.browse, page)) {
    browsePast.push(store.browse);
    store.setCanGoBack(true);
  }
  store.setBrowse(page);
  store.setTab("player");
  return loadBrowse(page, force, true);
}

export function goBack(): Promise<void> {
  const prev = browsePast.pop();
  useAppStore.getState().setCanGoBack(browsePast.length > 0);
  if (!prev) return Promise.resolve();
  return openBrowsePage(prev, false, false);
}

/** Re-read the open folder or playlist from disk without leaving the current tab. */
export function refreshOpenBrowse(): Promise<void> {
  return loadBrowse(useAppStore.getState().browse, false, false);
}

async function loadBrowse(page: BrowsePage, force: boolean, clearStatus: boolean): Promise<void> {
  const ticket = ++browseGeneration;
  const store = useAppStore.getState();
  if (
    page.kind === "home" ||
    page.kind === "discover" ||
    page.kind === "artist" ||
    page.kind === "album" ||
    page.kind === "library" ||
    page.kind === "playlists"
  ) {
    store.setPageTracks([]);
    store.setPageLoading(false);
    return;
  }

  const key =
    page.kind === "folder" ? cacheKey("folder", page.path) : cacheKey("playlist", page.id);
  if (force) dropCachedTracks(key);
  const cached = getCachedTracks(key);
  if (cached) {
    store.setPageTracks(cached);
    store.setPageLoading(false);
  } else {
    store.setPageTracks([]);
    store.setPageLoading(true);
  }
  if (clearStatus) store.setStatus(null);
  try {
    const fast =
      page.kind === "folder"
        ? await api.scanTracks(page.path, true)
        : await api.scanPlaylist(page.id, true);
    if (!current(ticket, page)) return;
    const previous = useAppStore.getState().pageTracks;
    const tagged = previous.some((track) => track.durationMs > 0 || track.artist || track.album);
    if (!clearStatus && tagged && samePaths(previous, fast)) {
      setCachedTracks(key, previous);
      useAppStore.getState().setPageTracks(previous);
      return;
    }
    useAppStore.getState().setPageTracks(fast);

    const full =
      page.kind === "folder"
        ? await api.scanTracks(page.path, false)
        : await api.scanPlaylist(page.id, false);
    if (!current(ticket, page)) return;
    setCachedTracks(key, full);
    useAppStore.getState().setPageTracks(full);
  } catch (error) {
    if (current(ticket, page)) {
      useAppStore.getState().setStatus(errorMessage(error, "Could not open that library"));
      if (!cached) useAppStore.getState().setPageTracks([]);
    }
  } finally {
    if (current(ticket, page)) {
      useAppStore.getState().setPageLoading(false);
    }
  }
}

function current(ticket: number, page: BrowsePage): boolean {
  return ticket === browseGeneration && samePage(useAppStore.getState().browse, page);
}

function samePaths(left: Track[], right: Track[]): boolean {
  if (left.length !== right.length) return false;
  const paths = new Set(left.map((track) => track.path));
  return right.every((track) => paths.has(track.path));
}

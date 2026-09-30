import { describe, expect, it } from "vitest";
import {
  cacheKey,
  dropCachedTracks,
  getCachedTracks,
  patchCachedTracks,
  setCachedTracks,
} from "./browseCache";
import type { Track } from "./types";

function track(path: string, title = path): Track {
  return {
    path,
    title,
    artist: "",
    album: "",
    albumArtist: "",
    track: null,
    disc: null,
    durationMs: 0,
    folder: "/",
    replaygainTrack: null,
    replaygainAlbum: null,
  };
}

describe("browseCache", () => {
  it("round-trips a page and reports misses", () => {
    const key = cacheKey("folder", "/a");
    expect(getCachedTracks(key)).toBeUndefined();
    setCachedTracks(key, [track("/a/1.mp3")]);
    expect(getCachedTracks(key)?.map((t) => t.path)).toEqual(["/a/1.mp3"]);
    dropCachedTracks(key);
    expect(getCachedTracks(key)).toBeUndefined();
  });

  it("evicts the least recently used page past the cap", () => {
    for (let i = 0; i < 12; i += 1) setCachedTracks(cacheKey("folder", `/lru/${i}`), []);
    // Touch page 0 so it is the most recent, then push one more in.
    getCachedTracks(cacheKey("folder", "/lru/0"));
    setCachedTracks(cacheKey("folder", "/lru/new"), []);
    expect(getCachedTracks(cacheKey("folder", "/lru/0"))).toBeDefined();
    expect(getCachedTracks(cacheKey("folder", "/lru/1"))).toBeUndefined();
  });

  it("patches edited tracks into every cached page by path", () => {
    const a = cacheKey("folder", "/patch/a");
    const b = cacheKey("playlist", "p1");
    setCachedTracks(a, [track("/s.mp3", "old"), track("/t.mp3")]);
    setCachedTracks(b, [track("/s.mp3", "old")]);
    patchCachedTracks([track("/s.mp3", "new")]);
    expect(getCachedTracks(a)?.map((t) => t.title)).toEqual(["new", "/t.mp3"]);
    expect(getCachedTracks(b)?.[0].title).toBe("new");
  });
});

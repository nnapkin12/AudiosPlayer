import type { Track } from "@/lib/types";

export interface AlbumGroup {
  name: string;
  tracks: Track[];
}

export interface ArtistGroup {
  key: string;
  name: string;
  tracks: Track[];
  albums: AlbumGroup[];
}

export function artistLabel(track: Track): string {
  return (track.artist || track.albumArtist).trim();
}

export function artistKey(name: string): string {
  return name
    .normalize("NFKC")
    .replace(/[\u200B-\u200D\uFEFF]/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .toLocaleLowerCase();
}

/** One person per name. Combined credits are split, and a credit that is still two people is left out. */
export function soloArtists(label: string): string[] {
  const cleaned = label
    .normalize("NFKC")
    .replace(/[\u200B-\u200D\uFEFF]/g, "")
    .replace(/\(\s*((?:feat\.?|ft\.?|featuring)\s+[^)]+)\)/gi, " $1 ")
    .replace(/\s+/g, " ")
    .trim();
  const names = splitCredits(cleaned)
    .map((name) => name.trim())
    .filter((name) => name.length > 0 && !looksLikeTwoPeople(name));
  const seen = new Set<string>();
  const unique: string[] = [];
  for (const name of names) {
    const key = artistKey(name);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    unique.push(name);
  }
  return unique;
}

/** The song belongs to the first credited name. A feature stays on that artist's page. */
export function primaryArtist(label: string): string {
  return soloArtists(label)[0] ?? "";
}

function looksLikeTwoPeople(name: string): boolean {
  return /\s(?:&|x|\/)\s/i.test(name) || /\s(?:feat\.?|ft\.?|featuring)\s/i.test(name);
}

function splitCredits(label: string): string[] {
  const people: string[] = [];
  for (const featured of label.split(/\s+(?:feat\.?|ft\.?|featuring)\s+/i)) {
    for (const piece of featured.split(/\s+&\s+|\s+x\s+|\s+\|\s+|;\s*/i)) {
      for (const slashed of splitSlash(piece)) {
        people.push(...splitCommaNames(slashed));
      }
    }
  }
  return people;
}

function splitSlash(value: string): string[] {
  const trimmed = value.trim();
  if (!trimmed.includes("/")) return [trimmed];
  const bits = trimmed
    .split(/\s*\/\s*/)
    .map((bit) => bit.trim())
    .filter(Boolean);
  if (bits.length < 2) return [trimmed];
  if (bits.every((bit) => bit.length <= 3)) return [trimmed];
  return bits;
}

function splitCommaNames(value: string): string[] {
  const trimmed = value.trim();
  if (!trimmed.includes(",")) return [trimmed];
  const bits = trimmed
    .split(/\s*,\s*/)
    .map((bit) => bit.trim())
    .filter(Boolean);
  if (bits.length < 2) return [trimmed];
  if (bits.some((bit) => bit.length < 2)) return [trimmed];
  if (bits.slice(1).some((bit) => /^the\s/i.test(bit))) return [trimmed];
  return bits;
}

export function buildCatalog(tracks: Track[]): { artists: ArtistGroup[]; untagged: number } {
  const byKey = new Map<string, { spellings: Map<string, number>; tracks: Track[] }>();
  let untagged = 0;

  for (const track of tracks) {
    const label = primaryArtist(artistLabel(track));
    if (!label) {
      untagged += 1;
      continue;
    }
    const key = artistKey(label);
    let group = byKey.get(key);
    if (!group) {
      group = { spellings: new Map(), tracks: [] };
      byKey.set(key, group);
    }
    group.spellings.set(label, (group.spellings.get(label) ?? 0) + 1);
    group.tracks.push(track);
  }

  const artists = [...byKey.entries()]
    .map(([key, group]) => {
      const albums = groupAlbums(group.tracks);
      return {
        key,
        name: preferredSpelling(group.spellings),
        tracks: group.tracks,
        albums,
      };
    })
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));

  return { artists, untagged };
}

function preferredSpelling(spellings: Map<string, number>): string {
  let best = "";
  let count = -1;
  for (const [name, seen] of spellings) {
    if (seen > count) {
      best = name;
      count = seen;
    }
  }
  return best;
}

function groupAlbums(tracks: Track[]): AlbumGroup[] {
  const byName = new Map<string, Track[]>();
  for (const track of tracks) {
    const name = track.album.trim();
    if (!name) continue;
    const list = byName.get(name);
    if (list) list.push(track);
    else byName.set(name, [track]);
  }
  return [...byName.entries()]
    .map(([name, albumTracks]) => ({ name, tracks: albumTracks }))
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" }));
}

const ARTIST_CAP = 15;
const SONG_MIN = 12;
const SONG_MAX = 15;

let pickedArtists: ArtistGroup[] | null = null;
const pickedSongs = new Map<string, Track[]>();

/** Chosen once per process. Later calls return the same artists and songs. */
export function rememberDiscover(artists: ArtistGroup[]): ArtistGroup[] {
  if (pickedArtists) return pickedArtists;
  if (artists.length === 0) return artists;
  const seen = new Set<string>();
  const unique = artists.filter((artist) => {
    const key = artistKey(artist.name);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  const chosen = shuffleTracks(unique).slice(0, ARTIST_CAP);
  for (const artist of chosen) {
    const count = SONG_MIN + Math.floor(Math.random() * (SONG_MAX - SONG_MIN + 1));
    pickedSongs.set(artist.key, shuffleTracks(artist.tracks).slice(0, count));
  }
  pickedArtists = chosen;
  return chosen;
}

export function discoverSongs(artist: ArtistGroup): Track[] {
  return pickedSongs.get(artist.key) ?? artist.tracks.slice(0, SONG_MAX);
}

/** Match the artist list already built from the library. No second scan. */
export function filterArtists(artists: ArtistGroup[], query: string): ArtistGroup[] {
  const needle = artistKey(query);
  if (!needle) return artists;
  return artists.filter((artist) => artist.key.includes(needle));
}

export function shuffleTracks<T>(tracks: T[]): T[] {
  const next = tracks.slice();
  for (let index = next.length - 1; index > 0; index -= 1) {
    const swap = Math.floor(Math.random() * (index + 1));
    const current = next[index];
    next[index] = next[swap];
    next[swap] = current;
  }
  return next;
}

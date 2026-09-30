import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { ChevronDown, Search, Shuffle, X } from "lucide-react";
import { PlayPauseIcon } from "@/features/shell/PlayPauseIcon";
import { TrackList } from "@/features/player/TrackList";
import {
  artistKey,
  buildCatalog,
  discoverSongs,
  filterArtists,
  rememberDiscover,
  shuffleTracks,
  type AlbumGroup,
  type ArtistGroup,
} from "@/features/player/catalog";
import { peekDiscover, setDiscover } from "@/features/player/discoverCache";
import { openBrowsePage } from "@/features/player/browse";
import { BrowseBack } from "@/features/player/Playlists";
import { api, pickImageFile } from "@/lib/api";
import { CoverPicture, useNearView } from "@/lib/covers";
import { errorMessage, pictureSrc } from "@/lib/format";
import type { Track } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

function sourceKeyOf(roots: string[], playlists: { id: string }[]): string {
  return `${roots.join("\n")}\n${playlists.map((playlist) => playlist.id).join("\n")}`;
}

async function readDiscoverTracks(roots: string[], playlistIds: string[]): Promise<Track[]> {
  const seen = new Set<string>();
  const next: Track[] = [];
  const take = (page: Track[]) => {
    for (const track of page) {
      if (seen.has(track.path)) continue;
      seen.add(track.path);
      next.push(track);
    }
  };
  for (const path of roots) take(await api.scanTracks(path, false));
  for (const id of playlistIds) take(await api.scanPlaylist(id, false));
  return next;
}

export function DiscoverView({
  onPlay,
  onContext,
}: {
  onPlay: (tracks: Track[], startPath?: string) => void;
  onContext: (event: MouseEvent, track: Track, queue: Track[]) => void;
}) {
  const browse = useAppStore((state) => state.browse);
  const libraryRoots = useAppStore((state) => state.libraryRoots);
  const playlists = useAppStore((state) => state.playlists);
  const setStatus = useAppStore((state) => state.setStatus);
  const [tracks, setTracks] = useState<Track[] | null>(() =>
    peekDiscover(sourceKeyOf(libraryRoots, playlists)),
  );
  const [failed, setFailed] = useState(false);

  const sourceKey = sourceKeyOf(libraryRoots, playlists);

  useEffect(() => {
    const cached = peekDiscover(sourceKey);
    if (cached) {
      setTracks(cached);
      return;
    }
    if (libraryRoots.length === 0 && playlists.length === 0) {
      setTracks([]);
      setFailed(false);
      return;
    }
    let cancelled = false;
    setFailed(false);
    void (async () => {
      try {
        const next = await readDiscoverTracks(
          libraryRoots,
          playlists.map((playlist) => playlist.id),
        );
        if (cancelled) return;
        setDiscover(sourceKey, next);
        setTracks(next);
      } catch (error) {
        if (cancelled) return;
        setFailed(true);
        setTracks([]);
        setStatus(errorMessage(error, "Could not read your music"));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [sourceKey, libraryRoots, playlists, setStatus]);

  const catalog = useMemo(() => buildCatalog(tracks ?? []), [tracks]);
  const artistName =
    browse.kind === "artist" ? browse.name : browse.kind === "album" ? browse.artist : null;
  const artist = artistName
    ? (catalog.artists.find(
        (item) => item.name === artistName || item.key === artistKey(artistName),
      ) ?? null)
    : null;
  const album =
    browse.kind === "album" && artist
      ? (artist.albums.find((item) => item.name === browse.album) ?? null)
      : null;

  if (libraryRoots.length === 0 && playlists.length === 0) {
    return (
      <div className="px-5 py-8">
        <h1 className="text-[28px] font-semibold tracking-tight text-app-text">Discover</h1>
        <p className="mt-3 max-w-lg text-[15px] font-medium leading-6 text-app-muted">
          Add a music folder or a playlist. Artists come from the tags on those songs.
        </p>
      </div>
    );
  }

  if (tracks === null) {
    return (
      <div className="px-5 py-8">
        <h1 className="text-[28px] font-semibold tracking-tight text-app-text">Discover</h1>
        <p className="mt-3 text-[15px] font-medium text-app-muted">Reading your library…</p>
      </div>
    );
  }

  if (failed) {
    return (
      <div className="px-5 py-8">
        <h1 className="text-[28px] font-semibold tracking-tight text-app-text">Discover</h1>
        <p className="mt-3 text-[15px] font-medium text-app-muted">
          The library could not be read.
        </p>
      </div>
    );
  }

  if (browse.kind === "album") {
    if (!artist || !album) {
      return (
        <MissingPage
          title="That album is not in the library."
          action="Artists"
          onBack={() => void openBrowsePage({ kind: "discover" })}
        />
      );
    }
    return <AlbumPage artist={artist} album={album} onPlay={onPlay} onContext={onContext} />;
  }

  if (browse.kind === "artist") {
    if (!artist) {
      return (
        <MissingPage
          title="That artist is not in the library."
          action="Artists"
          onBack={() => void openBrowsePage({ kind: "discover" })}
        />
      );
    }
    return <ArtistPage artist={artist} onPlay={onPlay} onContext={onContext} />;
  }

  return (
    <ArtistIndex
      featured={rememberDiscover(catalog.artists)}
      artists={catalog.artists}
      untagged={catalog.untagged}
    />
  );
}

function ArtistIndex({
  featured,
  artists,
  untagged,
}: {
  featured: ArtistGroup[];
  artists: ArtistGroup[];
  untagged: number;
}) {
  const [query, setQuery] = useState("");
  const searching = query.trim().length > 0;
  const matches = useMemo(
    () => (searching ? filterArtists(artists, query) : featured),
    [artists, featured, query, searching],
  );

  function openMatch(name: string) {
    void openBrowsePage({ kind: "artist", name, all: true });
  }

  return (
    <div className="min-h-0 flex-1 overflow-auto px-6 py-6">
      <BrowseBack />
      <h1 className="text-[28px] font-semibold tracking-tight text-app-text">Discover</h1>
      <p className="mt-1 text-[14px] font-medium text-app-muted">
        {artists.length === 0
          ? "No artist tags in this library."
          : searching
            ? `${matches.length} artist${matches.length === 1 ? "" : "s"}`
            : `${featured.length} artists`}
      </p>
      {artists.length > 0 ? (
        <form
          className="mt-4"
          onSubmit={(event) => {
            event.preventDefault();
            const exact = matches.find((artist) => artist.key === artistKey(query));
            const pick = exact ?? (matches.length === 1 ? matches[0] : null);
            if (pick) openMatch(pick.name);
          }}
        >
          <label className="relative block">
            <Search
              size={16}
              className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-app-muted"
            />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Search artists"
              className="w-full rounded-lg border border-app-border bg-app-raised py-2.5 pl-9 pr-9 text-[15px] text-app-text"
            />
            {query ? (
              <button
                type="button"
                title="Clear search"
                onClick={() => setQuery("")}
                className="absolute right-2 top-1/2 -translate-y-1/2 rounded-md p-1 text-app-muted hover:bg-app-hover hover:text-app-text"
              >
                <X size={14} />
              </button>
            ) : null}
          </label>
        </form>
      ) : null}
      {artists.length === 0 ? (
        <p className="mt-6 max-w-lg text-[15px] font-medium leading-6 text-app-muted">
          Songs still play from Library. Artist pages need those tags on the files.
        </p>
      ) : matches.length === 0 ? (
        <p className="mt-6 max-w-lg text-[15px] font-medium leading-6 text-app-muted">
          No artists match that search.
        </p>
      ) : (
        <div className="cover-grid mt-6">
          {matches.map((artist) => (
            <button
              key={artist.key}
              type="button"
              onClick={() =>
                searching
                  ? openMatch(artist.name)
                  : void openBrowsePage({ kind: "artist", name: artist.name })
              }
              className="rounded-2xl p-3 text-center hover:bg-app-hover"
            >
              <ArtistFace artist={artist} className="aspect-square h-auto w-full rounded-xl" />
              <span className="mt-3 block truncate text-center text-[16px] font-semibold text-app-text">
                {artist.name}
              </span>
              <span className="block truncate text-center text-[14px] font-medium text-app-muted">
                {artist.albums.length > 0
                  ? `${artist.albums.length} album${artist.albums.length === 1 ? "" : "s"}`
                  : `${artist.tracks.length} song${artist.tracks.length === 1 ? "" : "s"}`}
              </span>
            </button>
          ))}
        </div>
      )}
      {untagged > 0 && artists.length > 0 && !searching ? (
        <p className="mt-6 text-[13px] font-medium text-app-muted">
          {untagged} song{untagged === 1 ? "" : "s"} with no artist tag stay in the folder list.
        </p>
      ) : null}
    </div>
  );
}

function ArtistFace({ artist, className }: { artist: ArtistGroup; className?: string }) {
  const near = useNearView();
  const [custom, setCustom] = useState<string | null | undefined>(undefined);
  const revision = useArtistImageRevision(artist.key);

  useEffect(() => {
    if (!near.ready) return;
    let cancelled = false;
    setCustom(undefined);
    void api
      .artistImage(artist.key)
      .then((cover) => {
        if (!cancelled) setCustom(cover ? pictureSrc(cover.mime, cover.dataBase64) : null);
      })
      .catch(() => {
        if (!cancelled) setCustom(null);
      });
    return () => {
      cancelled = true;
    };
  }, [artist.key, revision, near.ready]);

  if (!near.ready || custom === undefined) {
    return (
      <div
        ref={near.ref}
        className={`bg-gradient-to-br from-app-hover to-app ${className ?? ""}`}
      />
    );
  }
  if (custom) {
    return (
      <div className={`overflow-hidden bg-app-hover ${className ?? ""}`}>
        <img src={custom} alt="" className="h-full w-full object-cover" />
      </div>
    );
  }
  return <CoverPicture path={artist.tracks[0].path} className={className} />;
}

let artistImageRevision = 0;
const artistImageListeners = new Set<() => void>();

function bumpArtistImage() {
  artistImageRevision += 1;
  artistImageListeners.forEach((listener) => listener());
}

function useArtistImageRevision(key: string): number {
  const [revision, setRevision] = useState(artistImageRevision);
  useEffect(() => {
    const listener = () => setRevision(artistImageRevision);
    artistImageListeners.add(listener);
    return () => {
      artistImageListeners.delete(listener);
    };
  }, [key]);
  return revision;
}

function ArtistImageMenu({ artistKey }: { artistKey: string }) {
  const [open, setOpen] = useState(false);
  const setStatus = useAppStore((state) => state.setStatus);

  async function choose() {
    setOpen(false);
    const path = await pickImageFile();
    if (!path) return;
    try {
      await api.setArtistImage(artistKey, path);
      bumpArtistImage();
    } catch (error) {
      setStatus(errorMessage(error, "Could not save that image"));
    }
  }

  return (
    <div className="relative mt-3">
      <button
        type="button"
        title="Artist image"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="flex h-8 w-8 items-center justify-center rounded-full text-app-muted hover:bg-app-hover hover:text-app-text"
      >
        <ChevronDown size={18} />
      </button>
      {open ? (
        <div className="absolute left-1/2 z-20 mt-1 w-44 -translate-x-1/2 rounded-lg border border-app-border bg-app-raised py-1 text-left shadow-lg">
          <button
            type="button"
            onClick={() => void choose()}
            className="block w-full px-3 py-2 text-[14px] font-medium text-app-text hover:bg-app-hover"
          >
            Add artist image
          </button>
        </div>
      ) : null}
    </div>
  );
}

function ArtistPage({
  artist,
  onPlay,
  onContext,
}: {
  artist: ArtistGroup;
  onPlay: (tracks: Track[], startPath?: string) => void;
  onContext: (event: MouseEvent, track: Track, queue: Track[]) => void;
}) {
  const browse = useAppStore((state) => state.browse);
  const songs = browse.kind === "artist" && browse.all ? artist.tracks : discoverSongs(artist);
  const currentPath = useAppStore((state) => state.snapshot?.current?.path ?? null);
  const playing = useAppStore((state) => state.snapshot?.playing ?? false);
  const inPage = Boolean(currentPath && songs.some((track) => track.path === currentPath));
  const showPause = inPage && playing;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-col items-center px-5 pt-4 text-center">
        <div className="self-start">
          <BrowseBack />
        </div>
        <ArtistFace artist={artist} className="h-32 w-32 rounded-2xl" />
        <ArtistImageMenu artistKey={artist.key} />
        <div className="mt-4">
          <button
            type="button"
            onClick={() => void openBrowsePage({ kind: "discover" })}
            className="text-[13px] font-semibold uppercase tracking-[0.06em] text-app-muted hover:text-app-text"
          >
            Discover
          </button>
          <p className="mt-1 truncate text-[28px] font-semibold text-app-text">{artist.name}</p>
          <p className="text-[14px] font-medium text-app-muted">
            {artist.albums.length > 0
              ? `${artist.albums.length} album${artist.albums.length === 1 ? "" : "s"} · `
              : ""}
            {songs.length} song{songs.length === 1 ? "" : "s"}
          </p>
        </div>
        <div className="mt-3 flex justify-center gap-2">
          <button
            type="button"
            title={showPause ? "Pause" : "Play"}
            onClick={() => void playOrToggle(songs, onPlay)}
            className="flex items-center gap-1.5 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
          >
            <PlayPauseIcon playing={showPause} size={14} />
            {showPause ? "Pause" : "Play"}
          </button>
          <button
            type="button"
            title="Shuffle"
            onClick={() => onPlay(shuffleTracks(songs))}
            className="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
          >
            <Shuffle size={14} />
            Shuffle
          </button>
        </div>
      </div>
      {artist.albums.length > 0 ? (
        <div className="px-5 pb-2">
          <p className="text-[13px] font-semibold uppercase tracking-[0.06em] text-app-muted">
            Albums
          </p>
          <div className="mt-2 flex gap-3 overflow-x-auto pb-2">
            {artist.albums.map((album) => (
              <button
                key={album.name}
                type="button"
                onClick={() =>
                  void openBrowsePage({ kind: "album", artist: artist.name, album: album.name })
                }
                className="w-36 shrink-0 rounded-xl p-2 text-left hover:bg-app-hover"
              >
                <CoverPicture path={album.tracks[0].path} className="h-32 w-32 rounded-lg" />
                <span className="mt-2 block truncate text-[14px] font-semibold text-app-text">
                  {album.name}
                </span>
                <span className="block text-[13px] font-medium text-app-muted">
                  {album.tracks.length} song{album.tracks.length === 1 ? "" : "s"}
                </span>
              </button>
            ))}
          </div>
        </div>
      ) : null}
      <p className="px-5 pb-1 text-[13px] font-semibold uppercase tracking-[0.06em] text-app-muted">
        Songs
      </p>
      <TrackList
        tracks={songs}
        onPlay={(track) => onPlay(songs, track.path)}
        onContext={(event, track) => onContext(event, track, songs)}
      />
    </div>
  );
}

function AlbumPage({
  artist,
  album,
  onPlay,
  onContext,
}: {
  artist: ArtistGroup;
  album: AlbumGroup;
  onPlay: (tracks: Track[], startPath?: string) => void;
  onContext: (event: MouseEvent, track: Track, queue: Track[]) => void;
}) {
  const currentPath = useAppStore((state) => state.snapshot?.current?.path ?? null);
  const playing = useAppStore((state) => state.snapshot?.playing ?? false);
  const inPage = Boolean(currentPath && album.tracks.some((track) => track.path === currentPath));
  const showPause = inPage && playing;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-col items-center px-5 pt-4 text-center">
        <div className="self-start">
          <BrowseBack />
        </div>
        <CoverPicture path={album.tracks[0].path} className="h-32 w-32 rounded-2xl" />
        <div className="mt-4">
          <button
            type="button"
            onClick={() => void openBrowsePage({ kind: "artist", name: artist.name })}
            className="text-[13px] font-semibold uppercase tracking-[0.06em] text-app-muted hover:text-app-text"
          >
            {artist.name}
          </button>
          <p className="mt-1 truncate text-[28px] font-semibold text-app-text">{album.name}</p>
          <p className="text-[14px] font-medium text-app-muted">
            {album.tracks.length} song{album.tracks.length === 1 ? "" : "s"}
          </p>
        </div>
        <button
          type="button"
          title={showPause ? "Pause" : "Play"}
          onClick={() => void playOrToggle(album.tracks, onPlay)}
          className="mt-3 flex items-center gap-1.5 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
        >
          <PlayPauseIcon playing={showPause} size={14} />
          {showPause ? "Pause" : "Play"}
        </button>
      </div>
      <TrackList
        tracks={album.tracks}
        onPlay={(track) => onPlay(album.tracks, track.path)}
        onContext={(event, track) => onContext(event, track, album.tracks)}
      />
    </div>
  );
}

function MissingPage({
  title,
  action,
  onBack,
}: {
  title: string;
  action: string;
  onBack: () => void;
}) {
  return (
    <div className="px-5 py-8">
      <p className="text-[15px] font-medium text-app-muted">{title}</p>
      <button
        type="button"
        onClick={onBack}
        className="mt-4 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
      >
        {action}
      </button>
    </div>
  );
}

async function playOrToggle(
  tracks: Track[],
  onPlay: (tracks: Track[], startPath?: string) => void,
) {
  const store = useAppStore.getState();
  const current = store.snapshot?.current?.path ?? null;
  if (current && tracks.some((track) => track.path === current)) {
    try {
      await api.toggle();
    } catch (error) {
      store.setStatus(errorMessage(error, "Could not control playback"));
    }
    return;
  }
  onPlay(tracks);
}

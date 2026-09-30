import { useEffect, useRef, useState, type FormEvent, type MouseEvent } from "react";
import { Download, Search } from "lucide-react";
import { PlayPauseIcon } from "@/features/shell/PlayPauseIcon";
import { api, openExternal, pickSavePath } from "@/lib/api";
import { errorMessage, safeFileName } from "@/lib/format";
import type { MediaHit, Track } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

export function SearchView() {
  const setStatus = useAppStore((state) => state.setStatus);
  const applySnapshot = useAppStore((state) => state.applySnapshot);
  const current = useAppStore((state) => state.snapshot?.current ?? null);
  const playing = useAppStore((state) => state.snapshot?.playing ?? false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<MediaHit[]>([]);
  const [loading, setLoading] = useState(false);
  const [fetchingUrl, setFetchingUrl] = useState<string | null>(null);
  const [savingUrl, setSavingUrl] = useState<string | null>(null);
  const playReq = useRef(0);
  const fetchStartedPath = useRef<string | null>(null);

  useEffect(() => {
    if (!fetchingUrl) return;
    const path = current?.path ?? null;
    if (path === fetchStartedPath.current) return;
    const id = videoIdFromUrl(fetchingUrl);
    if (id && path?.includes(id)) return;
    playReq.current += 1;
    setFetchingUrl(null);
  }, [current?.path, fetchingUrl]);

  async function onSearch(event: FormEvent) {
    event.preventDefault();
    const next = query.trim();
    if (!next) return;
    setLoading(true);
    setStatus(null);
    try {
      setResults(await api.searchMedia(next));
    } catch (error) {
      setResults([]);
      setStatus(errorMessage(error, "Search failed"));
    } finally {
      setLoading(false);
    }
  }

  async function playHit(hit: MediaHit) {
    if (fetchingUrl === hit.url) return;
    if (hitIsCurrent(hit, current)) {
      try {
        await api.toggle();
      } catch (error) {
        setStatus(errorMessage(error, "Could not control playback"));
      }
      return;
    }
    const gen = ++playReq.current;
    fetchStartedPath.current = current?.path ?? null;
    setFetchingUrl(hit.url);
    setStatus("Getting audio…", "info");
    try {
      const snapshot = await api.playMedia(hit.title, hit.url, hit.pageUrl);
      if (gen !== playReq.current) return;
      applySnapshot(snapshot);
      setStatus(null);
    } catch (error) {
      if (gen !== playReq.current) return;
      setStatus(errorMessage(error, "Could not play"));
    } finally {
      if (gen === playReq.current) setFetchingUrl(null);
    }
  }

  async function playFirst() {
    const next = query.trim();
    if (!next) return;
    setLoading(true);
    setStatus(null);
    try {
      const hit = await api.searchStream(next);
      setResults([hit]);
      await playHit(hit);
    } catch (error) {
      setStatus(errorMessage(error, "Could not play"));
    } finally {
      setLoading(false);
    }
  }

  async function downloadHit(event: MouseEvent, hit: MediaHit) {
    event.preventDefault();
    event.stopPropagation();
    const dest = await pickSavePath(safeFileName(hit.title), "Save audio");
    if (!dest) return;
    setSavingUrl(hit.url);
    setStatus(null);
    try {
      await api.saveMedia(hit.url, dest, hit.pageUrl);
    } catch (error) {
      setStatus(errorMessage(error, "Could not save"));
    } finally {
      setSavingUrl(null);
    }
  }

  return (
    <section className="page-scroll flex flex-col">
      <div className="flex w-full flex-col gap-6">
        <div>
          <h1 className="text-[28px] font-semibold tracking-tight">Search</h1>
          <p className="mt-2 max-w-2xl text-[15px] font-medium leading-6 text-app-muted">
            Find songs on YouTube and SoundCloud. Paste a link from either, or from another site
            yt-dlp supports. Play starts right away. Save a copy if you want to keep it.
          </p>
        </div>

        <form
          onSubmit={(event) => void onSearch(event)}
          className="flex gap-2 rounded-xl border border-app-border bg-app-raised/80 p-3"
        >
          <label className="relative min-w-0 flex-1">
            <Search
              size={16}
              className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-app-muted"
            />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Song, artist, or link"
              autoFocus
              className="w-full rounded-lg border border-app-border bg-app py-2.5 pl-9 pr-3 text-[15px] text-app-text"
            />
          </label>
          <button
            type="submit"
            disabled={loading || !query.trim()}
            className="rounded-lg border border-app-border px-4 py-2 text-[14px] font-semibold text-app-subtle hover:bg-app-hover disabled:opacity-50"
          >
            {loading ? "Searching" : "Search"}
          </button>
          <button
            type="button"
            disabled={loading || !query.trim()}
            onClick={() => void playFirst()}
            className="rounded-lg bg-app-play px-4 py-2 text-[14px] font-semibold text-app-play-fg disabled:opacity-50"
          >
            Play
          </button>
        </form>

        {results.length === 0 ? (
          <p className="text-[15px] text-app-subtle">
            {loading
              ? "Searching…"
              : "Type a search and press Enter for results, or Play to start the first match."}
          </p>
        ) : null}

        <ul className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,320px),1fr))] gap-2">
          {results.map((hit) => {
            const fetching = fetchingUrl === hit.url;
            const currentHit = hitIsCurrent(hit, current);
            const showPause = currentHit && playing;
            return (
              <li key={`${hit.title}:${hit.url}`}>
                <div
                  className={`flex w-full items-center gap-2 rounded-lg px-2 py-2 ${
                    fetching || currentHit ? "bg-app-hover" : "hover:bg-app-hover/70"
                  }`}
                >
                  <div className="flex min-w-0 flex-1 items-center gap-3 px-1 py-1">
                    <button
                      type="button"
                      title={showPause ? "Pause" : "Play"}
                      onClick={() => void playHit(hit)}
                      className="relative h-12 w-12 shrink-0 overflow-hidden rounded-md bg-app-raised text-white"
                    >
                      <ResultThumb url={hit.thumbnailUrl} />
                      <span className="absolute inset-0 flex items-center justify-center bg-black/45">
                        <PlayPauseIcon playing={showPause} size={16} />
                      </span>
                    </button>
                    <div className="min-w-0 flex-1">
                      <button
                        type="button"
                        onClick={() => void playHit(hit)}
                        className="block w-full truncate text-left text-[15px] font-semibold text-app-text"
                      >
                        {hit.title}
                      </button>
                      <span className="block truncate text-[13px] font-medium text-app-muted">
                        {fetching ? "Loading…" : showPause ? "Playing" : (hit.channel ?? "Play")}
                      </span>
                      <button
                        type="button"
                        title="Open source"
                        onClick={() => void openExternal(sourceUrl(hit))}
                        className="mt-0.5 block w-full truncate text-left text-[12px] font-medium text-app-accent hover:underline"
                      >
                        {sourceUrl(hit)}
                      </button>
                    </div>
                  </div>
                  <button
                    type="button"
                    title="Download"
                    aria-label={`Download ${hit.title}`}
                    disabled={savingUrl === hit.url}
                    onClick={(event) => void downloadHit(event, hit)}
                    className="mr-1 flex h-10 w-10 items-center justify-center rounded-md text-app-muted hover:bg-app-raised hover:text-app-text disabled:opacity-50"
                  >
                    <Download size={16} />
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      </div>
    </section>
  );
}

function ResultThumb({ url }: { url?: string }) {
  const [failed, setFailed] = useState(false);
  if (!url || failed) {
    return <span className="block h-full w-full bg-app-hover" />;
  }
  return (
    <img
      src={url}
      alt=""
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
      className="h-full w-full object-cover"
      loading="lazy"
    />
  );
}

function sourceUrl(hit: MediaHit): string {
  return hit.pageUrl || hit.url;
}

function videoIdFromUrl(url: string): string | null {
  const watch = /[?&]v=([A-Za-z0-9_-]{11})/.exec(url);
  if (watch) return watch[1];
  const short = /youtu\.be\/([A-Za-z0-9_-]{11})/.exec(url);
  return short?.[1] ?? null;
}

function hitIsCurrent(hit: MediaHit, track: Track | null): boolean {
  if (!track) return false;
  const id = videoIdFromUrl(hit.pageUrl || hit.url);
  if (id && track.path.includes(id)) return true;
  return track.album === "Search" && track.title === hit.title;
}

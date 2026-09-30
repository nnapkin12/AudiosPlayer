import { useEffect, useRef, useState } from "react";
import { ListMusic } from "lucide-react";
import { api, isTauri } from "./api";
import { pictureSrc } from "./format";

const THUMB_CAP = 80;
const PICTURE_CAP = 48;
const PLAYLIST_CAP = 24;
const cache = new Map<string, string | null>();
const inflight = new Map<string, Promise<string | null>>();

function lruGet<T>(map: Map<string, T>, key: string): T | undefined {
  const hit = map.get(key);
  if (hit === undefined) return undefined;
  map.delete(key);
  map.set(key, hit);
  return hit;
}

const LOAD_LIMIT = 2;
let loadsActive = 0;
const loadWaiters: Array<() => void> = [];

function acquireLoad(): Promise<void> {
  if (loadsActive < LOAD_LIMIT) {
    loadsActive += 1;
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    loadWaiters.push(() => {
      loadsActive += 1;
      resolve();
    });
  });
}

function releaseLoad(): void {
  loadsActive = Math.max(0, loadsActive - 1);
  const next = loadWaiters.shift();
  if (next) next();
}

function lruSet<T>(map: Map<string, T>, key: string, value: T, cap: number): void {
  if (map.has(key)) map.delete(key);
  map.set(key, value);
  while (map.size > cap) {
    const oldest = map.keys().next().value;
    if (oldest === undefined) break;
    map.delete(oldest);
  }
}

export function useNearView() {
  const ref = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (ready) return;
    const node = ref.current;
    if (!node) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        setReady(true);
      },
      { rootMargin: "160px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [ready]);

  return { ref, ready };
}

export function dropCover(path: string): void {
  cache.delete(path);
  inflight.delete(path);
  pictureCache.delete(path);
  pictureInflight.delete(path);
}

export function cachedCover(path: string): string | null | undefined {
  return cache.has(path) ? (cache.get(path) ?? null) : undefined;
}

export function loadCoverThumb(path: string): Promise<string | null> {
  const hit = lruGet(cache, path);
  if (hit !== undefined) return Promise.resolve(hit);
  const pending = inflight.get(path);
  if (pending) return pending;
  if (!isTauri()) {
    lruSet(cache, path, null, THUMB_CAP);
    return Promise.resolve(null);
  }
  const request = api
    .coverThumb(path)
    .then((cover) => {
      const url = cover ? pictureSrc(cover.mime, cover.dataBase64) : null;
      lruSet(cache, path, url, THUMB_CAP);
      return url;
    })
    .catch(() => {
      lruSet(cache, path, null, THUMB_CAP);
      return null;
    })
    .finally(() => {
      inflight.delete(path);
    });
  inflight.set(path, request);
  return request;
}

const pictureCache = new Map<string, string | null>();
const pictureInflight = new Map<string, Promise<string | null>>();

export function loadCoverPicture(path: string): Promise<string | null> {
  const hit = lruGet(pictureCache, path);
  if (hit !== undefined) return Promise.resolve(hit);
  const pending = pictureInflight.get(path);
  if (pending) return pending;
  if (!isTauri()) {
    lruSet(pictureCache, path, null, PICTURE_CAP);
    return Promise.resolve(null);
  }
  const request = acquireLoad()
    .then(() => api.coverArt(path))
    .then((cover) => {
      const url = cover ? pictureSrc(cover.mime, cover.dataBase64) : null;
      lruSet(pictureCache, path, url, PICTURE_CAP);
      return url;
    })
    .catch(() => {
      lruSet(pictureCache, path, null, PICTURE_CAP);
      return null;
    })
    .finally(() => {
      pictureInflight.delete(path);
      releaseLoad();
    });
  pictureInflight.set(path, request);
  return request;
}

export function CoverPicture({ path, className }: { path: string; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [src, setSrc] = useState<string | null | undefined>(() =>
    pictureCache.has(path) ? (pictureCache.get(path) ?? null) : undefined,
  );

  useEffect(() => {
    setSrc(pictureCache.has(path) ? (pictureCache.get(path) ?? null) : undefined);
  }, [path]);

  useEffect(() => {
    if (src !== undefined) return;
    const node = ref.current;
    if (!node) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        void loadCoverPicture(path).then(setSrc);
      },
      { rootMargin: "240px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [path, src]);

  return (
    <div
      ref={ref}
      className={`shrink-0 overflow-hidden rounded bg-app-hover ${className ?? "h-10 w-10"}`}
    >
      {src ? (
        <img src={src} alt="" className="h-full w-full object-cover" />
      ) : (
        <div className="h-full w-full bg-gradient-to-br from-app-hover to-app" />
      )}
    </div>
  );
}

export function CoverThumb({ path, className }: { path: string; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [src, setSrc] = useState<string | null | undefined>(() => cachedCover(path));

  useEffect(() => {
    setSrc(cachedCover(path));
  }, [path]);

  useEffect(() => {
    if (src !== undefined) return;
    const node = ref.current;
    if (!node) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        void loadCoverThumb(path).then(setSrc);
      },
      { rootMargin: "160px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [path, src]);

  return (
    <div
      ref={ref}
      className={`shrink-0 overflow-hidden rounded bg-app-hover ${className ?? "h-10 w-10"}`}
    >
      {src ? (
        <img src={src} alt="" className="h-full w-full object-cover" />
      ) : (
        <div className="h-full w-full bg-gradient-to-br from-app-hover to-app" />
      )}
    </div>
  );
}

const playlistCache = new Map<string, string | null>();
const playlistInflight = new Map<string, Promise<string | null>>();

export function dropPlaylistCover(id: string): void {
  playlistCache.delete(id);
}

function loadPlaylistCover(id: string): Promise<string | null> {
  const hit = lruGet(playlistCache, id);
  if (hit !== undefined) return Promise.resolve(hit);
  const pending = playlistInflight.get(id);
  if (pending) return pending;
  if (!isTauri()) {
    lruSet(playlistCache, id, null, PLAYLIST_CAP);
    return Promise.resolve(null);
  }
  const request = acquireLoad()
    .then(() => api.playlistCover(id))
    .then((cover) => {
      const url = cover ? pictureSrc(cover.mime, cover.dataBase64) : null;
      lruSet(playlistCache, id, url, PLAYLIST_CAP);
      return url;
    })
    .catch(() => {
      lruSet(playlistCache, id, null, PLAYLIST_CAP);
      return null;
    })
    .finally(() => {
      playlistInflight.delete(id);
      releaseLoad();
    });
  playlistInflight.set(id, request);
  return request;
}

export function PlaylistCover({
  id,
  className,
  iconSize = 16,
}: {
  id: string;
  className?: string;
  iconSize?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [src, setSrc] = useState<string | null | undefined>(() =>
    playlistCache.has(id) ? (playlistCache.get(id) ?? null) : undefined,
  );

  useEffect(() => {
    setSrc(playlistCache.has(id) ? (playlistCache.get(id) ?? null) : undefined);
  }, [id]);

  useEffect(() => {
    if (src !== undefined) return;
    const node = ref.current;
    if (!node) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        void loadPlaylistCover(id).then(setSrc);
      },
      { rootMargin: "160px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [id, src]);

  return (
    <div
      ref={ref}
      className={`flex shrink-0 items-center justify-center overflow-hidden rounded-md bg-app-hover ${className ?? "h-10 w-10"}`}
    >
      {src ? (
        <img src={src} alt="" className="h-full w-full object-cover" />
      ) : (
        <ListMusic size={iconSize} className="text-app-accent" />
      )}
    </div>
  );
}

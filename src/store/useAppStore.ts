import { create } from "zustand";
import { api } from "@/lib/api";
import { pictureSrc } from "@/lib/format";
import { applyAppearance, type CustomTheme } from "@/lib/theme";
import { applyMotion } from "@/lib/motion";
import type { BrowsePage, MissingItem, Playlist, PlayerSnapshot, Tick, Track } from "@/lib/types";

export type AppTab = "player" | "search" | "tags" | "settings";
export type StatusTone = "error" | "info";

interface AppState {
  tab: AppTab;
  nowPlayingOpen: boolean;
  snapshot: PlayerSnapshot | null;
  coverUrl: string | null;
  status: string | null;
  statusTone: StatusTone;
  playlists: Playlist[];
  libraryRoots: string[];
  libraryEpoch: number;
  missing: MissingItem[];
  browse: BrowsePage;
  pageTracks: Track[];
  pageLoading: boolean;
  canGoBack: boolean;
  positionMs: number;
  durationMs: number;
  theme: string;
  accent: string;
  customThemes: CustomTheme[];
  minimizeMovement: boolean;
  visualizer: boolean;
  vizMain: string;
  vizBorder: string;
  vizGlow: string;
  tagFocusPath: string | null;
  setTab: (tab: AppTab) => void;
  setNowPlayingOpen: (open: boolean) => void;
  setStatus: (status: string | null, tone?: StatusTone) => void;
  setPlaylists: (playlists: Playlist[]) => void;
  setLibraryRoots: (roots: string[]) => void;
  bumpLibrary: () => void;
  setMissing: (missing: MissingItem[]) => void;
  setBrowse: (browse: BrowsePage) => void;
  setPageTracks: (tracks: Track[]) => void;
  setPageLoading: (loading: boolean) => void;
  setCanGoBack: (canGoBack: boolean) => void;
  setTagFocusPath: (path: string | null) => void;
  setAppearance: (theme: string, accent: string, customThemes?: CustomTheme[]) => void;
  setMinimizeMovement: (enabled: boolean) => void;
  setVisualizer: (settings: {
    enabled: boolean;
    main: string;
    border: string;
    glow: string;
  }) => void;
  applySnapshot: (snapshot: PlayerSnapshot) => void;
  applyTick: (tick: Tick) => void;
  refreshCover: (path: string | null) => Promise<void>;
}

export const useAppStore = create<AppState>((set, get) => ({
  tab: "player",
  nowPlayingOpen: false,
  snapshot: null,
  coverUrl: null,
  status: null,
  statusTone: "error",
  playlists: [],
  libraryRoots: [],
  libraryEpoch: 0,
  missing: [],
  browse: { kind: "home" },
  pageTracks: [],
  pageLoading: false,
  canGoBack: false,
  positionMs: 0,
  durationMs: 0,
  theme: "dusk",
  accent: "blue",
  customThemes: [],
  minimizeMovement: false,
  visualizer: false,
  vizMain: "#8ec8ff",
  vizBorder: "#e8f3ff",
  vizGlow: "#4aa3ff",
  tagFocusPath: null,
  setTab: (tab) => set({ tab }),
  setNowPlayingOpen: (nowPlayingOpen) => {
    set({ nowPlayingOpen });
    if (nowPlayingOpen) {
      void get().refreshCover(get().snapshot?.current?.path ?? null);
    }
  },
  setStatus: (status, tone = "error") =>
    set({ status, statusTone: status ? tone : "error" }),
  setPlaylists: (playlists) => set({ playlists }),
  setLibraryRoots: (libraryRoots) => set({ libraryRoots }),
  bumpLibrary: () => set({ libraryEpoch: get().libraryEpoch + 1 }),
  setMissing: (missing) => set({ missing }),
  setBrowse: (browse) => set({ browse }),
  setPageTracks: (pageTracks) => set({ pageTracks }),
  setPageLoading: (pageLoading) => set({ pageLoading }),
  setCanGoBack: (canGoBack) => set({ canGoBack }),
  setTagFocusPath: (tagFocusPath) => set({ tagFocusPath }),
  setAppearance: (theme, accent, customThemes) => {
    const nextThemes = customThemes ?? get().customThemes;
    applyAppearance(theme, accent, nextThemes);
    set({ theme, accent, customThemes: nextThemes });
  },
  setMinimizeMovement: (enabled) => {
    applyMotion(enabled);
    set({ minimizeMovement: enabled });
  },
  setVisualizer: ({ enabled, main, border, glow }) => {
    set({ visualizer: enabled, vizMain: main, vizBorder: border, vizGlow: glow });
  },
  applySnapshot: (snapshot) => {
    const previous = get().snapshot?.current?.path ?? null;
    const prevEq = get().snapshot?.eq;
    const eq = snapshot.eq
      ? {
          ...snapshot.eq,
          builtins:
            snapshot.eq.builtins.length > 0
              ? snapshot.eq.builtins
              : (prevEq?.builtins ?? []),
        }
      : snapshot.eq;
    set({
      snapshot: { ...snapshot, eq },
      status: snapshot.error,
      statusTone: "error",
      positionMs: snapshot.positionMs,
      durationMs: snapshot.durationMs,
    });
    const next = snapshot.current?.path ?? null;
    if (next !== previous) {
      if (get().nowPlayingOpen) void get().refreshCover(next);
      else set({ coverUrl: null });
    }
  },
  applyTick: (tick) => {
    set({
      positionMs: tick.positionMs,
      durationMs: tick.durationMs || get().durationMs,
    });
  },
  refreshCover: async (path) => {
    if (!path) {
      set({ coverUrl: null });
      return;
    }
    try {
      const cover = await api.coverArt(path);
      set({
        coverUrl: cover ? pictureSrc(cover.mime, cover.dataBase64) : null,
      });
    } catch {
      set({ coverUrl: null });
    }
  },
}));

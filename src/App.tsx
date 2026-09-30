import { useEffect, useState } from "react";
import { NowPlayingBar } from "@/features/shell/NowPlayingBar";
import { NowPlayingFull } from "@/features/shell/NowPlayingFull";
import { ShortcutSheet } from "@/features/shell/ShortcutSheet";
import { Sidebar } from "@/features/shell/Sidebar";
import { Titlebar } from "@/features/shell/Titlebar";
import { useWindowDrop } from "@/features/shell/useWindowDrop";
import { WindowEdges } from "@/features/shell/WindowEdges";
import { PlayerView } from "@/features/player/PlayerView";
import { SearchView } from "@/features/search/SearchView";
import { SettingsView } from "@/features/settings/SettingsView";
import { TagsView } from "@/features/tags/TagsView";
import { refreshOpenBrowse } from "@/features/player/browse";
import { api, isTauri, listen } from "@/lib/api";
import { errorMessage } from "@/lib/format";
import { applyAppearance } from "@/lib/theme";
import { applyMotion } from "@/lib/motion";
import type { PlayerSnapshot, Tick } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";
import { ConfirmHost } from "@/ui/ConfirmDialog";
import { ErrorBoundary } from "@/ui/ErrorBoundary";

export default function App() {
  const tab = useAppStore((state) => state.tab);
  const status = useAppStore((state) => state.status);
  const statusTone = useAppStore((state) => state.statusTone);
  const error = useAppStore((state) => state.error);
  const clearError = useAppStore((state) => state.clearError);
  const nowPlayingOpen = useAppStore((state) => state.nowPlayingOpen);
  const applySnapshot = useAppStore((state) => state.applySnapshot);
  const applyTick = useAppStore((state) => state.applyTick);
  const setStatus = useAppStore((state) => state.setStatus);
  const setNowPlayingOpen = useAppStore((state) => state.setNowPlayingOpen);
  const setPlaylists = useAppStore((state) => state.setPlaylists);
  const setLibraryRoots = useAppStore((state) => state.setLibraryRoots);
  const bumpLibrary = useAppStore((state) => state.bumpLibrary);
  const hasTrack = useAppStore((state) => Boolean(state.snapshot?.current));
  const setAppearance = useAppStore((state) => state.setAppearance);
  const setMinimizeMovement = useAppStore((state) => state.setMinimizeMovement);
  const setVisualizer = useAppStore((state) => state.setVisualizer);
  const { hot: dropHot } = useWindowDrop();
  const [shortcutsOpen, setShortcutsOpen] = useState(false);

  useEffect(() => {
    if (!isTauri()) {
      applyAppearance("dusk", "blue");
      applyMotion(false);
      setStatus("Preview only. Run npm run tauri dev for playback and tags in Audios!.", "info");
      return;
    }
    let disposed = false;
    const stop: Array<() => void> = [];

    void (async () => {
      try {
        const [snapshot, playlists, appearance, roots, missing] = await Promise.all([
          api.state(),
          api.listPlaylists(),
          api.getAppearance(),
          api.listLibraryRoots(),
          api.listMissing(),
        ]);
        if (disposed) return;
        applySnapshot(snapshot);
        setPlaylists(playlists);
        setLibraryRoots(roots);
        useAppStore.getState().setMissing(missing);
        setAppearance(appearance.theme, appearance.accent, appearance.customThemes ?? []);
        setMinimizeMovement(appearance.minimizeMovement ?? false);
        setVisualizer({
          enabled: appearance.visualizer ?? false,
          main: appearance.visualizerMain ?? "#8ec8ff",
          border: appearance.visualizerBorder ?? "#e8f3ff",
          glow: appearance.visualizerGlow ?? "#4aa3ff",
        });
      } catch (error) {
        if (!disposed) {
          applyAppearance("dusk", "blue");
          applyMotion(false);
          setStatus(errorMessage(error, "Could not load player"));
        }
      }
      const stateUnlisten = await listen<PlayerSnapshot>("player://state", applySnapshot);
      if (disposed) {
        stateUnlisten();
        return;
      }
      stop.push(stateUnlisten);
      const tickUnlisten = await listen<Tick>("player://tick", applyTick);
      if (disposed) {
        tickUnlisten();
        return;
      }
      stop.push(tickUnlisten);
      let refreshTimer = 0;
      const refreshFromDisk = async () => {
        try {
          const [playlists, roots, missing] = await Promise.all([
            api.listPlaylists(),
            api.listLibraryRoots(),
            api.listMissing(),
          ]);
          if (disposed) return;
          setPlaylists(playlists);
          setLibraryRoots(roots);
          useAppStore.getState().setMissing(missing);
          useAppStore.getState().bumpLibrary();
          const browse = useAppStore.getState().browse;
          if (browse.kind === "folder" && !roots.includes(browse.path)) {
            useAppStore.getState().setBrowse({ kind: "home" });
            useAppStore.getState().setPageTracks([]);
            useAppStore.getState().setPageLoading(false);
            return;
          }
          if (browse.kind !== "home") await refreshOpenBrowse();
        } catch {
          // The next open reads the disk again.
        }
      };
      const scheduleRefresh = () => {
        window.clearTimeout(refreshTimer);
        refreshTimer = window.setTimeout(() => {
          void refreshFromDisk();
        }, 400);
      };
      const libraryUnlisten = await listen("library://changed", () => {
        scheduleRefresh();
      });
      if (disposed) {
        libraryUnlisten();
        return;
      }
      stop.push(libraryUnlisten);
      const onVisible = () => {
        if (document.visibilityState === "hidden") return;
        scheduleRefresh();
      };
      document.addEventListener("visibilitychange", onVisible);
      stop.push(() => {
        window.clearTimeout(refreshTimer);
        document.removeEventListener("visibilitychange", onVisible);
      });
    })();

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setNowPlayingOpen(false);
        setShortcutsOpen(false);
      }
      if (shortcutBlocked(event)) return;
      if (event.key === "?" || (event.shiftKey && event.key === "/")) {
        event.preventDefault();
        setShortcutsOpen((open) => !open);
        return;
      }
      const playingNow = Boolean(useAppStore.getState().snapshot?.current);
      if (event.code === "Space") {
        if (!playingNow) return;
        event.preventDefault();
        void api.toggle();
      }
      if (event.key === "f" && playingNow) setNowPlayingOpen(true);
      if (event.key === "ArrowRight")
        void api.seek((useAppStore.getState().snapshot?.positionMs ?? 0) + 5000);
      if (event.key === "ArrowLeft")
        void api.seek(Math.max(0, (useAppStore.getState().snapshot?.positionMs ?? 0) - 5000));
      if (event.key === "n") void api.next();
      if (event.key === "p") void api.previous();
    };
    window.addEventListener("keydown", onKey);

    return () => {
      disposed = true;
      stop.forEach((fn) => fn());
      window.removeEventListener("keydown", onKey);
    };
  }, [
    applySnapshot,
    applyTick,
    bumpLibrary,
    setAppearance,
    setLibraryRoots,
    setMinimizeMovement,
    setNowPlayingOpen,
    setPlaylists,
    setStatus,
    setVisualizer,
  ]);

  return (
    <div className="app-frame relative flex h-full flex-col overflow-hidden bg-app">
      <WindowEdges />
      <Titlebar />
      {/* Everything below the title bar is inside a boundary, so a crash
          leaves minimize, maximize and close working. */}
      <ErrorBoundary name="Audios!" fill>
        <div className="relative flex min-h-0 flex-1">
          <Sidebar />
          <main className="flex min-w-0 flex-1 flex-col">
            <div
              className={
                tab === "player" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"
              }
            >
              <ErrorBoundary name="the library" fill>
                <PlayerView />
              </ErrorBoundary>
            </div>
            <div
              className={
                tab === "search" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"
              }
            >
              <ErrorBoundary name="Search" fill>
                <SearchView />
              </ErrorBoundary>
            </div>
            <div
              className={
                tab === "tags" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"
              }
            >
              <ErrorBoundary name="Tags" fill>
                <TagsView />
              </ErrorBoundary>
            </div>
            <div
              className={
                tab === "settings" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"
              }
            >
              <ErrorBoundary name="Settings" fill>
                <SettingsView />
              </ErrorBoundary>
            </div>
            {status || error ? (
              <p
                role="status"
                aria-live="polite"
                className={`flex shrink-0 items-center justify-between gap-3 border-t border-app-line px-4 py-1.5 text-[13px] font-semibold ${
                  status && statusTone === "info" ? "text-app-muted" : "text-app-danger"
                }`}
              >
                <span>{status ?? error}</span>
                {status && statusTone === "info" ? null : (
                  <button
                    type="button"
                    aria-label="Dismiss"
                    className="text-app-muted hover:text-app-text"
                    onClick={() => {
                      if (status) setStatus(null);
                      else clearError();
                    }}
                  >
                    ×
                  </button>
                )}
              </p>
            ) : null}
          </main>
          {nowPlayingOpen && hasTrack ? (
            <ErrorBoundary name="Now Playing">
              <NowPlayingFull />
            </ErrorBoundary>
          ) : null}
        </div>
      </ErrorBoundary>
      {dropHot ? (
        <div className="pointer-events-none absolute inset-0 z-40 flex items-center justify-center bg-black/35 text-[17px] font-semibold text-white">
          Drop to {tab === "tags" ? "edit tags" : "play"}
        </div>
      ) : null}
      <ConfirmHost />
      {shortcutsOpen ? <ShortcutSheet onClose={() => setShortcutsOpen(false)} /> : null}
      {nowPlayingOpen ? null : <NowPlayingBar />}
    </div>
  );
}

function shortcutBlocked(event: KeyboardEvent): boolean {
  if (event.altKey || event.ctrlKey || event.metaKey) return true;
  if (event.shiftKey && event.key !== "?") return true;
  const target = event.target as HTMLElement | null;
  if (!target) return false;
  if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) {
    return true;
  }
  if (target.tagName === "BUTTON" || target.tagName === "SELECT" || target.tagName === "A") {
    return true;
  }
  return Boolean(target.closest("[role=dialog], [role=alertdialog], [role=menu]"));
}

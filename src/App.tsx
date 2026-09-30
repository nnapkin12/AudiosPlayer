import { useEffect } from "react";
import { NowPlayingBar } from "@/features/shell/NowPlayingBar";
import { NowPlayingFull } from "@/features/shell/NowPlayingFull";
import { Sidebar } from "@/features/shell/Sidebar";
import { Titlebar } from "@/features/shell/Titlebar";
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

export default function App() {
  const tab = useAppStore((state) => state.tab);
  const status = useAppStore((state) => state.status);
  const statusTone = useAppStore((state) => state.statusTone);
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
      stop.push(await listen<PlayerSnapshot>("player://state", applySnapshot));
      stop.push(await listen<Tick>("player://tick", applyTick));
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
      stop.push(await listen("library://changed", () => {
        void refreshFromDisk();
      }));
      const onFocus = () => {
        if (document.visibilityState === "hidden") return;
        void refreshFromDisk();
      };
      window.addEventListener("focus", onFocus);
      document.addEventListener("visibilitychange", onFocus);
      stop.push(() => {
        window.removeEventListener("focus", onFocus);
        document.removeEventListener("visibilitychange", onFocus);
      });
    })();

    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setNowPlayingOpen(false);
      }
      const target = event.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) {
        return;
      }
      const playingNow = Boolean(useAppStore.getState().snapshot?.current);
      if (event.code === "Space") {
        if (!playingNow) return;
        event.preventDefault();
        void api.toggle();
      }
      if (event.key === "f" && playingNow) setNowPlayingOpen(true);
      if (event.key === "ArrowRight") void api.seek((useAppStore.getState().snapshot?.positionMs ?? 0) + 5000);
      if (event.key === "ArrowLeft") void api.seek(Math.max(0, (useAppStore.getState().snapshot?.positionMs ?? 0) - 5000));
      if (event.key === "n") void api.next();
      if (event.key === "p") void api.previous();
    };
    window.addEventListener("keydown", onKey);

    return () => {
      disposed = true;
      stop.forEach((fn) => fn());
      window.removeEventListener("keydown", onKey);
    };
  }, [applySnapshot, applyTick, bumpLibrary, setAppearance, setLibraryRoots, setMinimizeMovement, setNowPlayingOpen, setPlaylists, setStatus, setVisualizer]);

  return (
    <div className="app-frame relative flex h-full flex-col overflow-hidden bg-app">
      <WindowEdges />
      <Titlebar />
      <div className="relative flex min-h-0 flex-1">
        <Sidebar />
        <main className="flex min-w-0 flex-1 flex-col">
          <div className={tab === "player" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"}>
            <PlayerView />
          </div>
          <div className={tab === "search" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"}>
            <SearchView />
          </div>
          <div className={tab === "tags" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"}>
            <TagsView />
          </div>
          <div className={tab === "settings" ? "tab-panel flex min-h-0 min-w-0 flex-1 flex-col" : "hidden"}>
            <SettingsView />
          </div>
          {status ? (
            <p
              className={`shrink-0 border-t border-app-line px-4 py-1.5 text-[13px] font-semibold ${
                statusTone === "info" ? "text-app-muted" : "text-app-danger"
              }`}
            >
              {status}
            </p>
          ) : null}
        </main>
        {nowPlayingOpen && hasTrack ? <NowPlayingFull /> : null}
      </div>
      {nowPlayingOpen || !hasTrack ? null : <NowPlayingBar />}
    </div>
  );
}

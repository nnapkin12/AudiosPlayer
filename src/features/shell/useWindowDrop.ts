import { useEffect, useState } from "react";
import { api, isTauri } from "@/lib/api";
import { errorMessage } from "@/lib/format";
import { useAppStore } from "@/store/useAppStore";

/** Window-wide file/folder drop. Player plays; Tags opens the metadata editor. */
export function useWindowDrop() {
  const [hot, setHot] = useState(false);
  const setStatus = useAppStore((state) => state.setStatus);
  const applySnapshot = useAppStore((state) => state.applySnapshot);
  const setDroppedPaths = useAppStore((state) => state.setDroppedPaths);
  const setTab = useAppStore((state) => state.setTab);

  useEffect(() => {
    if (!isTauri()) return;
    let gone = false;
    let stop: (() => void) | undefined;
    void (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        if (gone) return;
        const payload = event.payload;
        if (payload.type === "leave") {
          setHot(false);
          return;
        }
        if (payload.type === "over" || payload.type === "enter") {
          setHot(true);
          return;
        }
        if (payload.type !== "drop") return;
        setHot(false);
        const paths = payload.paths.filter(Boolean);
        if (paths.length === 0) return;
        const currentTab = useAppStore.getState().tab;
        if (currentTab === "tags") {
          setDroppedPaths(paths);
          return;
        }
        void (async () => {
          try {
            const snap =
              paths.length === 1 ? await api.openPath(paths[0]) : await api.playQueuePaths(paths);
            applySnapshot(snap);
            if (currentTab !== "player") setTab("player");
          } catch (error) {
            setStatus(errorMessage(error, "Couldn't open that drop"));
          }
        })();
      });
      if (gone) unlisten();
      else stop = unlisten;
    })();
    return () => {
      gone = true;
      stop?.();
    };
  }, [applySnapshot, setDroppedPaths, setStatus, setTab]);

  return { hot };
}

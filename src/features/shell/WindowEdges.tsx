import { useEffect, useRef, useState } from "react";
import { isTauri } from "@/lib/api";

type Edge = "East" | "North" | "NorthWest" | "South" | "SouthEast" | "SouthWest" | "West";

const EDGES: Array<{ direction: Edge; className: string }> = [
  { direction: "North", className: "top-0 left-2 right-36 h-1 cursor-ns-resize" },
  { direction: "South", className: "bottom-0 left-2 right-2 h-1 cursor-ns-resize" },
  { direction: "West", className: "left-0 top-2 bottom-2 w-1 cursor-ew-resize" },
  { direction: "East", className: "right-0 top-10 bottom-2 w-1 cursor-ew-resize" },
  { direction: "NorthWest", className: "left-0 top-0 h-3 w-3 cursor-nwse-resize" },
  { direction: "SouthWest", className: "bottom-0 left-0 h-3 w-3 cursor-nesw-resize" },
  { direction: "SouthEast", className: "bottom-0 right-0 h-3 w-3 cursor-nwse-resize" },
];

export function WindowEdges() {
  const resize = useRef<((direction: Edge) => Promise<void>) | null>(null);
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    let gone = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      if (gone) return;
      const win = getCurrentWindow();
      resize.current = (direction) => win.startResizeDragging(direction);
      const sync = () => {
        void win.isMaximized().then((value) => {
          if (!gone) setMaximized(value);
        });
      };
      sync();
      const stop = await win.onResized(() => {
        sync();
      });
      if (gone) stop();
      else unlisten = stop;
    })();
    return () => {
      gone = true;
      unlisten?.();
    };
  }, []);

  if (!isTauri() || maximized) return null;

  return (
    <>
      {EDGES.map((edge) => (
        <div
          key={edge.direction}
          aria-hidden
          className={`no-drag absolute z-50 ${edge.className}`}
          onPointerDown={(event) => {
            if (event.button !== 0) return;
            event.preventDefault();
            event.stopPropagation();
            void resize.current?.(edge.direction);
          }}
        />
      ))}
    </>
  );
}

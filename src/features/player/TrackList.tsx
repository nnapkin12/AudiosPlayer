import { useEffect, useState, type MouseEvent } from "react";
import { MoreHorizontal } from "lucide-react";
import { PlayPauseIcon } from "@/features/shell/PlayPauseIcon";
import { useTransport } from "@/features/shell/useTransport";
import { CoverThumb } from "@/lib/covers";
import { displayArtist, displayTitle, formatTime } from "@/lib/format";
import { VirtualList } from "@/lib/virtualList";
import type { Track } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

const ROW = 58;

export function TrackList({
  tracks,
  onPlay,
  onContext,
}: {
  tracks: Track[];
  onPlay: (track: Track, index: number) => void;
  onContext: (event: MouseEvent, track: Track, index: number) => void;
}) {
  const currentPath = useAppStore((state) => state.snapshot?.current?.path ?? null);
  const playing = useAppStore((state) => state.snapshot?.playing ?? false);
  const transport = useTransport();
  const [hoveredKey, setHoveredKey] = useState<string | null>(null);

  useEffect(() => {
    const clear = () => setHoveredKey(null);
    const onPointerOut = (event: PointerEvent) => {
      if (!event.relatedTarget) clear();
    };
    window.addEventListener("blur", clear);
    document.addEventListener("mouseleave", clear);
    document.addEventListener("pointerleave", clear);
    window.addEventListener("pointerout", onPointerOut);
    return () => {
      window.removeEventListener("blur", clear);
      document.removeEventListener("mouseleave", clear);
      document.removeEventListener("pointerleave", clear);
      window.removeEventListener("pointerout", onPointerOut);
    };
  }, []);

  if (tracks.length === 0) return null;

  async function activate(track: Track, index: number) {
    if (track.path === currentPath) {
      transport.toggle();
      return;
    }
    onPlay(track, index);
  }

  return (
    <VirtualList
      items={tracks}
      rowHeight={ROW}
      className="min-h-0 flex-1 overflow-auto"
      onPointerLeave={() => setHoveredKey(null)}
      getKey={(track, index) => `${track.path}:${index}`}
      renderRow={(track, index) => {
        const key = `${track.path}:${index}`;
        const active = track.path === currentPath;
        const hovered = hoveredKey === key;
        const showPause = active && playing;
        return (
          <div
            onPointerEnter={() => setHoveredKey(key)}
            onContextMenu={(event) => onContext(event, track, index)}
            className={`flex h-full w-full items-center gap-3 px-5 ${
              active ? "bg-app-hover" : hovered ? "bg-app-hover/40" : ""
            }`}
          >
            <button
              type="button"
              title={showPause ? "Pause" : "Play"}
              aria-label={showPause ? "Pause" : "Play"}
              onClick={() => void activate(track, index)}
              className="relative shrink-0"
            >
              <CoverThumb path={track.path} className="h-11 w-11 rounded-md" />
              <span
                className={`absolute inset-0 flex items-center justify-center rounded-md bg-black/50 text-white transition-opacity ${
                  hovered ? "opacity-100" : "opacity-0"
                }`}
              >
                <PlayPauseIcon playing={showPause} size={16} />
              </span>
            </button>
            <span className="w-7 shrink-0 text-right text-[13px] font-semibold tabular-nums text-app-muted">
              {track.track ?? index + 1}
            </span>
            <button
              type="button"
              onClick={() => void activate(track, index)}
              className="min-w-0 flex-1 text-left"
            >
              <span className="block truncate text-[15px] font-semibold text-app-text">
                {displayTitle(track.title, track.path)}
              </span>
              <span className="block truncate text-[13px] font-medium text-app-muted">
                {displayArtist(track.artist, track.albumArtist)}
                {track.album ? `  ·  ${track.album}` : ""}
              </span>
            </button>
            <span className="shrink-0 text-[13px] font-semibold tabular-nums text-app-muted">
              {formatTime(track.durationMs)}
            </span>
            <button
              type="button"
              title="Song menu"
              aria-label="Song menu"
              onClick={(event) => onContext(event, track, index)}
              className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-app-muted hover:bg-app-hover hover:text-app-text"
            >
              <MoreHorizontal size={16} />
            </button>
          </div>
        );
      }}
    />
  );
}

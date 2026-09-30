import type { ReactNode } from "react";
import {
  Maximize2,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
} from "lucide-react";
import { PlayPauseIcon } from "@/features/shell/PlayPauseIcon";
import { TransportSeek } from "@/features/shell/SeekBar";
import { SpeedControl } from "@/features/shell/SpeedControl";
import { Visualizer } from "@/features/shell/Visualizer";
import { api } from "@/lib/api";
import { CoverThumb } from "@/lib/covers";
import { displayArtist, displayTitle } from "@/lib/format";
import type { RepeatMode } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

export function NowPlayingBar() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setNowPlayingOpen = useAppStore((state) => state.setNowPlayingOpen);
  const current = snapshot?.current ?? null;
  const playing = snapshot?.playing ?? false;
  const volume = snapshot?.volume ?? 0.85;
  const muted = snapshot?.muted ?? false;
  const repeat = snapshot?.repeat ?? "off";
  const shuffle = snapshot?.shuffle ?? false;
  const visualizer = useAppStore((state) => state.visualizer);

  return (
    <footer className="now-playing-bar relative z-10 h-[92px] shrink-0 items-center gap-4 border-t border-app-bar-line bg-app-bar px-4 shadow-[inset_0_1px_0_rgb(255_255_255_/_0.06)]">
      <div className="flex min-w-0 items-center gap-3">
      <button
        type="button"
        onClick={() => setNowPlayingOpen(true)}
        className="flex min-w-0 max-w-[16rem] items-center gap-3 text-left"
      >
        <div className="h-12 w-12 shrink-0 overflow-hidden rounded-md bg-app-hover">
          {current ? (
            <CoverThumb path={current.path} className="h-12 w-12 rounded-md" />
          ) : (
            <div className="h-full w-full bg-gradient-to-br from-app-hover to-app" />
          )}
        </div>
        <div className="min-w-0">
          <p className="truncate text-[15px] font-semibold text-app-text">
            {current ? displayTitle(current.title, current.path) : "Nothing playing"}
          </p>
          <p className="truncate text-[13px] font-medium text-app-muted">
            {current ? displayArtist(current.artist, current.albumArtist) : "Nothing in Audios! yet"}
          </p>
        </div>
      </button>
      {visualizer ? <Visualizer variant="bar" /> : null}
      </div>

      <div className="flex min-w-0 flex-col items-center gap-1.5">
        <div className="relative flex items-center justify-center">
          <div className="absolute right-full mr-2">
            <SpeedControl />
          </div>
          <BarTransport shuffle={shuffle} repeat={repeat} playing={playing} />
        </div>
        <TransportSeek
          tone="bar"
          onSeek={(ms) => {
            void api.seek(ms).catch(() => undefined);
          }}
        />
      </div>

      <div className="flex min-w-0 items-center justify-end gap-1">
        <IconButton
          label={muted ? "Unmute" : "Mute"}
          onClick={() => void api.setMuted(!muted).catch(() => undefined)}
        >
          {muted || volume === 0 ? <VolumeX size={15} /> : <Volume2 size={15} />}
        </IconButton>
        <input
          type="range"
          min={0}
          max={100}
          value={muted ? 0 : Math.round(volume * 100)}
          onChange={(event) => {
            void api.setVolume(Number(event.target.value) / 100).catch(() => undefined);
          }}
          className="bar-range bar-volume w-24"
        />
        <IconButton label="Fullscreen" onClick={() => setNowPlayingOpen(true)}>
          <Maximize2 size={16} />
        </IconButton>
      </div>
    </footer>
  );
}

function BarTransport({
  shuffle,
  repeat,
  playing,
}: {
  shuffle: boolean;
  repeat: RepeatMode;
  playing: boolean;
}) {
  return (
    <div className="flex items-center justify-center gap-2">
      <IconButton
        label="Shuffle"
        active={shuffle}
        onClick={() => void api.setShuffle(!shuffle).catch(() => undefined)}
      >
        <Shuffle key={String(shuffle)} size={15} className="t-pop" />
      </IconButton>
      <IconButton
        label="Previous"
        nudge="prev"
        onClick={() => void api.previous().catch(() => undefined)}
      >
        <SkipBack size={16} />
      </IconButton>
      <button
        type="button"
        title={playing ? "Pause" : "Play"}
        onClick={() => void api.toggle().catch(() => undefined)}
        className="t-btn flex h-10 w-10 items-center justify-center rounded-full bg-app-play text-app-play-fg"
      >
        <PlayPauseIcon playing={playing} size={16} />
      </button>
      <IconButton
        label="Next"
        nudge="next"
        onClick={() => void api.next().catch(() => undefined)}
      >
        <SkipForward size={16} />
      </IconButton>
      <IconButton
        label="Repeat"
        active={repeat !== "off"}
        onClick={() => void api.setRepeat(nextRepeat(repeat)).catch(() => undefined)}
      >
        {repeat === "one" ? (
          <Repeat1 key="one" size={15} className="t-pop" />
        ) : (
          <Repeat key={repeat} size={15} className="t-pop" />
        )}
      </IconButton>
    </div>
  );
}

function nextRepeat(mode: RepeatMode): RepeatMode {
  if (mode === "off") return "all";
  if (mode === "all") return "one";
  return "off";
}

function IconButton({
  label,
  active,
  nudge,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  nudge?: "next" | "prev";
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={`t-btn flex h-8 w-8 items-center justify-center rounded-md ${
        nudge === "next" ? "t-btn-next" : nudge === "prev" ? "t-btn-prev" : ""
      } ${active ? "text-app-accent" : "text-app-subtle hover:text-app-text"}`}
    >
      {children}
    </button>
  );
}

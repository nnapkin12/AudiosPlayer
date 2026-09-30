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
import { nextRepeat, useTransport } from "@/features/shell/useTransport";
import { Visualizer } from "@/features/shell/Visualizer";
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
  const transport = useTransport();

  return (
    <footer className="now-playing-bar relative z-10 shrink-0 items-center gap-4 border-t border-app-bar-line bg-app-bar px-4 shadow-[inset_0_1px_0_rgb(255_255_255_/_0.06)]">
      <div className="flex min-w-0 items-center gap-3">
        <button
          type="button"
          onClick={() => current && setNowPlayingOpen(true)}
          disabled={!current}
          className="flex min-w-0 max-w-[16rem] items-center gap-3 text-left disabled:opacity-70"
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
              {current
                ? displayArtist(current.artist, current.albumArtist)
                : "Nothing in Audios! yet"}
            </p>
          </div>
        </button>
        {visualizer ? <Visualizer variant="bar" /> : null}
      </div>

      <div className="flex min-w-0 flex-col items-center gap-1.5">
        <div className="relative flex items-center justify-center">
          <div className="bar-speed absolute right-full mr-2">
            <SpeedControl />
          </div>
          <BarTransport
            shuffle={shuffle}
            repeat={repeat}
            playing={playing}
            enabled={Boolean(current)}
            transport={transport}
          />
        </div>
        <TransportSeek tone="bar" onSeek={transport.seek} />
      </div>

      <div className="flex min-w-0 items-center justify-end gap-1">
        <IconButton label={muted ? "Unmute" : "Mute"} onClick={() => transport.setMuted(!muted)}>
          {muted || volume === 0 ? <VolumeX size={15} /> : <Volume2 size={15} />}
        </IconButton>
        <input
          type="range"
          min={0}
          max={100}
          aria-label="Volume"
          value={muted ? 0 : Math.round(volume * 100)}
          onChange={(event) => {
            transport.setVolume(Number(event.target.value) / 100);
          }}
          className="bar-range bar-volume w-24"
        />
        <IconButton label="Fullscreen" disabled={!current} onClick={() => setNowPlayingOpen(true)}>
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
  enabled,
  transport,
}: {
  shuffle: boolean;
  repeat: RepeatMode;
  playing: boolean;
  enabled: boolean;
  transport: ReturnType<typeof useTransport>;
}) {
  return (
    <div className="flex items-center justify-center gap-2">
      <IconButton label="Shuffle" active={shuffle} onClick={() => transport.setShuffle(!shuffle)}>
        <Shuffle key={String(shuffle)} size={15} className="t-pop" />
      </IconButton>
      <IconButton label="Previous" nudge="prev" disabled={!enabled} onClick={transport.previous}>
        <SkipBack size={16} />
      </IconButton>
      <button
        type="button"
        title={playing ? "Pause" : "Play"}
        aria-label={playing ? "Pause" : "Play"}
        disabled={!enabled}
        onClick={transport.toggle}
        className="t-btn flex h-10 w-10 items-center justify-center rounded-full bg-app-play text-app-play-fg disabled:opacity-40"
      >
        <PlayPauseIcon playing={playing} size={16} />
      </button>
      <IconButton label="Next" nudge="next" disabled={!enabled} onClick={transport.next}>
        <SkipForward size={16} />
      </IconButton>
      <IconButton
        label="Repeat"
        active={repeat !== "off"}
        onClick={() => transport.setRepeat(nextRepeat(repeat))}
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

function IconButton({
  label,
  active,
  nudge,
  disabled,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  nudge?: "next" | "prev";
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={`t-btn flex h-8 w-8 items-center justify-center rounded-md disabled:opacity-40 ${
        nudge === "next" ? "t-btn-next" : nudge === "prev" ? "t-btn-prev" : ""
      } ${active ? "text-app-accent" : "text-app-subtle hover:text-app-text"}`}
    >
      {children}
    </button>
  );
}

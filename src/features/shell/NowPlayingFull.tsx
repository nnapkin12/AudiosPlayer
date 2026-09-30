import { useEffect, useRef, useState, type CSSProperties } from "react";
import { ChevronDown, Repeat, Repeat1, Shuffle, SkipBack, SkipForward } from "lucide-react";
import { PlayPauseIcon } from "@/features/shell/PlayPauseIcon";
import { TransportSeek } from "@/features/shell/SeekBar";
import { SpeedControl } from "@/features/shell/SpeedControl";
import { nextRepeat, useTransport } from "@/features/shell/useTransport";
import { Visualizer } from "@/features/shell/Visualizer";
import { paletteFromPixels, type ArtColors } from "@/lib/artworkColor";
import { displayArtist, displayTitle } from "@/lib/format";
import { useAppStore } from "@/store/useAppStore";
import { useFocusTrap } from "@/ui/useFocusTrap";

export function NowPlayingFull() {
  const snapshot = useAppStore((state) => state.snapshot);
  const coverUrl = useAppStore((state) => state.coverUrl);
  const setNowPlayingOpen = useAppStore((state) => state.setNowPlayingOpen);
  const current = snapshot?.current ?? null;
  const playing = snapshot?.playing ?? false;
  const repeat = snapshot?.repeat ?? "off";
  const shuffle = snapshot?.shuffle ?? false;
  const visualizer = useAppStore((state) => state.visualizer);
  const palette = useArtPalette(coverUrl);
  const transport = useTransport();
  const overlayRef = useRef<HTMLDivElement>(null);
  useFocusTrap(overlayRef, true);

  return (
    <div
      ref={overlayRef}
      role="dialog"
      aria-modal="true"
      aria-label="Now playing"
      className="np-overlay absolute inset-0 z-30 flex flex-col overflow-auto bg-app"
      style={paletteStyle(palette)}
    >
      <div className="flex items-center justify-between px-6 py-4">
        <button
          type="button"
          aria-label="Close now playing"
          onClick={() => setNowPlayingOpen(false)}
          className="flex items-center gap-2 rounded-lg px-2 py-1 text-[15px] font-semibold text-app-subtle hover:bg-app-hover"
        >
          <ChevronDown size={22} />
          Now playing
        </button>
      </div>

      <div className="flex min-h-0 flex-1 items-center justify-center px-[clamp(1rem,4vw,3rem)] pb-8">
        <div className={`np-stage flex flex-col items-center ${visualizer ? "np-stage-viz" : ""}`}>
          <div className={visualizer ? "flex items-stretch gap-6" : undefined}>
            <div className="np-cover shrink-0 overflow-hidden rounded-2xl bg-app-hover shadow-[0_18px_50px_rgb(0_0_0_/_0.28)]">
              {coverUrl ? (
                <img
                  key={coverUrl}
                  src={coverUrl}
                  alt=""
                  className="cover-swap aspect-square w-full object-cover"
                />
              ) : (
                <div className="aspect-square w-full bg-gradient-to-br from-app-hover to-app" />
              )}
            </div>
            {visualizer ? <Visualizer variant="stage" /> : null}
          </div>

          <div className="np-copy mt-8 text-center">
            <h1 className="truncate text-[clamp(1.4rem,3vw,2rem)] font-semibold tracking-tight">
              {current ? displayTitle(current.title, current.path) : "Nothing playing"}
            </h1>
            <p className="mt-2 truncate text-[17px] font-medium text-app-muted">
              {current
                ? `${displayArtist(current.artist, current.albumArtist)}${current.album ? `  ·  ${current.album}` : ""}`
                : "Play something from your library"}
            </p>
          </div>

          <div className="np-copy mt-8">
            <TransportSeek onSeek={transport.seek} />
          </div>

          <div className="np-copy mt-6 grid grid-cols-[minmax(3.5rem,1fr)_auto_minmax(3.5rem,1fr)] items-center">
            <div className="justify-self-end pr-4">
              <SpeedControl />
            </div>
            <div className="flex items-center justify-center gap-5">
              <button
                type="button"
                title="Shuffle"
                aria-label="Shuffle"
                onClick={() => transport.setShuffle(!shuffle)}
                className={`t-btn ${shuffle ? "text-app-accent" : "text-app-muted hover:text-app-text"}`}
              >
                <Shuffle key={String(shuffle)} size={22} className="t-pop" />
              </button>
              <button
                type="button"
                title="Previous"
                aria-label="Previous"
                onClick={transport.previous}
                className="t-btn t-btn-prev text-app-text"
              >
                <SkipBack size={28} fill="currentColor" />
              </button>
              <button
                type="button"
                title={playing ? "Pause" : "Play"}
                aria-label={playing ? "Pause" : "Play"}
                onClick={transport.toggle}
                className="t-btn flex h-16 w-16 items-center justify-center rounded-full bg-app-play text-app-play-fg"
              >
                <PlayPauseIcon playing={playing} size={28} />
              </button>
              <button
                type="button"
                title="Next"
                aria-label="Next"
                onClick={transport.next}
                className="t-btn t-btn-next text-app-text"
              >
                <SkipForward size={28} fill="currentColor" />
              </button>
              <button
                type="button"
                title="Repeat"
                aria-label="Repeat"
                onClick={() => transport.setRepeat(nextRepeat(repeat))}
                className={`t-btn ${repeat !== "off" ? "text-app-accent" : "text-app-muted hover:text-app-text"}`}
              >
                {repeat === "one" ? (
                  <Repeat1 key="one" size={22} className="t-pop" />
                ) : (
                  <Repeat key={repeat} size={22} className="t-pop" />
                )}
              </button>
            </div>
            <span />
          </div>
        </div>
      </div>
    </div>
  );
}

function useArtPalette(coverUrl: string | null): ArtColors | null {
  const [palette, setPalette] = useState<ArtColors | null>(null);
  useEffect(() => {
    if (!coverUrl) {
      setPalette(null);
      return;
    }
    let gone = false;
    const image = new Image();
    image.onload = () => {
      const size = 48;
      const canvas = document.createElement("canvas");
      canvas.width = size;
      canvas.height = size;
      const context = canvas.getContext("2d", { willReadFrequently: true });
      if (!context) return;
      context.drawImage(image, 0, 0, size, size);
      const next = paletteFromPixels(context.getImageData(0, 0, size, size).data);
      if (!gone) setPalette(next);
    };
    image.onerror = () => {
      if (!gone) setPalette(null);
    };
    image.src = coverUrl;
    return () => {
      gone = true;
    };
  }, [coverUrl]);
  return palette;
}

function paletteStyle(palette: ArtColors | null): CSSProperties | undefined {
  if (!palette) return undefined;
  return {
    "--app": palette.app,
    "--app-raised": palette.raised,
    "--app-hover": palette.hover,
    "--app-accent": palette.accent,
    "--app-accent-dim": palette.accentDim,
    "--app-play": palette.play,
    "--app-play-fg": palette.playFg,
    "--app-text": palette.text,
    "--app-muted": palette.muted,
    "--app-subtle": palette.subtle,
    "--app-border": palette.border,
    "--app-line": palette.line,
  } as CSSProperties;
}

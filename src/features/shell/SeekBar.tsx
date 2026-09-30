import { useEffect, useRef, useState } from "react";
import { formatTime } from "@/lib/format";
import { useSmoothPosition } from "@/lib/motion";
import { useAppStore } from "@/store/useAppStore";

export function SeekBar({
  position,
  duration,
  onSeek,
  tone = "page",
}: {
  position: number;
  duration: number;
  onSeek: (ms: number) => void;
  tone?: "page" | "bar";
}) {
  const trackPath = useAppStore((state) => state.snapshot?.current?.path ?? null);
  const holding = useRef(false);
  const dragRef = useRef<number | null>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const max = Math.max(duration, 1);
  const shown = drag ?? Math.min(position, duration);
  const ratio = duration > 0 ? Math.min(1, Math.max(0, shown / max)) : 0;

  function commitDrag() {
    const next = dragRef.current;
    holding.current = false;
    dragRef.current = null;
    setDrag(null);
    if (next == null) return;
    useAppStore.setState({ positionMs: next });
    onSeek(next);
  }

  useEffect(() => {
    holding.current = false;
    dragRef.current = null;
    setDrag(null);
  }, [trackPath]);

  useEffect(() => {
    if (drag === null) return;
    const end = () => commitDrag();
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
    window.addEventListener("mouseup", end);
    return () => {
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("mouseup", end);
    };
  }, [drag]); // eslint-disable-line react-hooks/exhaustive-deps -- commit uses dragRef

  return (
    <div className="relative h-5 min-w-0 flex-1">
      <div
        className={`pointer-events-none absolute inset-x-0 top-1/2 h-[5px] -translate-y-1/2 overflow-hidden rounded-full ${
          tone === "bar" ? "bg-app-bar-line" : "bg-app-border"
        }`}
      >
        <div
          className="seek-fill h-full w-full bg-app-text"
          style={{ transform: `scaleX(${ratio})` }}
        />
      </div>
      <div
        className="pointer-events-none absolute top-1/2 h-3.5 w-3.5 rounded-full bg-app-text shadow-[0_0_0_3px_rgb(var(--app-accent)_/_0.22)]"
        style={{ left: `${ratio * 100}%`, transform: "translate(-50%, -50%)" }}
      />
      <input
        type="range"
        min={0}
        max={max}
        value={shown}
        onChange={(event) => {
          const next = Number(event.target.value);
          dragRef.current = next;
          if (holding.current) setDrag(next);
          else {
            useAppStore.setState({ positionMs: next });
            onSeek(next);
          }
        }}
        onPointerDown={() => {
          holding.current = true;
        }}
        onPointerUp={commitDrag}
        onPointerCancel={commitDrag}
        className="absolute inset-0 m-0 h-full w-full cursor-pointer opacity-0"
      />
    </div>
  );
}

export function TransportSeek({
  onSeek,
  tone = "page",
}: {
  onSeek: (ms: number) => void;
  tone?: "page" | "bar";
}) {
  const playing = useAppStore((state) => state.snapshot?.playing ?? false);
  const duration = useAppStore((state) => state.durationMs);
  const position = useSmoothPosition(playing);

  return (
    <div
      className={`flex w-full items-center gap-2 text-[12px] font-semibold text-app-muted ${
        tone === "bar" ? "max-w-[520px]" : "max-w-[560px] gap-3 text-[13px]"
      }`}
    >
      <span className={`text-right tabular-nums ${tone === "bar" ? "w-10" : "w-12"}`}>
        {formatTime(position)}
      </span>
      <SeekBar position={position} duration={duration} onSeek={onSeek} tone={tone} />
      <span className={`tabular-nums ${tone === "bar" ? "w-10" : "w-12"}`}>
        {formatTime(duration)}
      </span>
    </div>
  );
}

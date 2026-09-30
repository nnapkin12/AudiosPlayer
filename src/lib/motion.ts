import { useEffect, useRef, useState } from "react";
import { useAppStore } from "@/store/useAppStore";

export function applyMotion(minimize: boolean) {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.motion = minimize ? "min" : "full";
}

export const POSITION_LEAD_MS = 350;

/** Creep ahead of the last confirmed position, but only by a fraction of a second. */
export function followPosition(lastMs: number, elapsedMs: number, speed: number): number {
  const rate = Number.isFinite(speed) && speed > 0 ? speed : 1;
  const lead = Math.min(Math.max(0, elapsedMs) * rate, POSITION_LEAD_MS);
  return lastMs + lead;
}

export function useSmoothPosition(playing: boolean): number {
  const tick = useAppStore((state) => state.positionMs);
  const duration = useAppStore((state) => state.durationMs);
  const speed = useAppStore((state) => state.snapshot?.speed ?? 1);
  const minimize = useAppStore((state) => state.minimizeMovement);
  const trackPath = useAppStore((state) => state.snapshot?.current?.path ?? null);
  const [shown, setShown] = useState(tick);
  const origin = useRef({ pos: tick, at: 0 });

  useEffect(() => {
    origin.current = { pos: tick, at: performance.now() };
    setShown(tick);
  }, [tick, duration, trackPath]);

  useEffect(() => {
    if (
      !playing ||
      minimize ||
      duration <= 0 ||
      (typeof window !== "undefined" &&
        window.matchMedia("(prefers-reduced-motion: reduce)").matches)
    ) {
      return;
    }
    let id = 0;
    const step = () => {
      setShown(followPosition(origin.current.pos, performance.now() - origin.current.at, speed));
      id = requestAnimationFrame(step);
    };
    id = requestAnimationFrame(step);
    return () => cancelAnimationFrame(id);
  }, [playing, minimize, duration, tick, speed]);

  if (!playing || minimize || duration <= 0) return tick;
  if (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  ) {
    return tick;
  }
  return shown;
}

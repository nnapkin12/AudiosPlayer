import { useEffect, useRef } from "react";
import { listen } from "@/lib/api";
import { useAppStore } from "@/store/useAppStore";

const BARS = 32;

export function Visualizer({ variant }: { variant: "bar" | "stage" }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const playing = useAppStore((state) => state.snapshot?.playing ?? false);
  const main = useAppStore((state) => state.vizMain);
  const border = useAppStore((state) => state.vizBorder);
  const glow = useAppStore((state) => state.vizGlow);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const bands = new Float32Array(BARS);
    const shown = new Float32Array(BARS);
    let unlisten = () => {};
    let frame = 0;
    let stopped = false;
    let lastTime = 0;

    void listen<number[]>("player://viz", (payload) => {
      for (let index = 0; index < BARS; index += 1) {
        const prev = index > 0 ? (payload[index - 1] ?? 0) : (payload[0] ?? 0);
        const next = index + 1 < BARS ? (payload[index + 1] ?? 0) : (payload[index] ?? 0);
        const center = payload[index] ?? 0;
        bands[index] = (prev * 0.22 + center * 0.56 + next * 0.22) / 255;
      }
    }).then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    });

    const follow = (dt: number) => {
      const rise = 1 - Math.exp(-dt / 0.09);
      const fall = 1 - Math.exp(-dt / 0.28);
      for (let index = 0; index < BARS; index += 1) {
        const target = playing ? bands[index] : 0;
        const step = target > shown[index] ? rise : fall;
        shown[index] += (target - shown[index]) * step;
      }
    };

    const paint = () => {
      const parent = canvas.parentElement;
      if (!parent) return;
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      const width = parent.clientWidth;
      const height = parent.clientHeight;
      if (width < 2 || height < 2) return;
      const pxW = Math.round(width * dpr);
      const pxH = Math.round(height * dpr);
      if (canvas.width !== pxW || canvas.height !== pxH) {
        canvas.width = pxW;
        canvas.height = pxH;
      }
      const context = canvas.getContext("2d");
      if (!context) return;
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
      context.clearRect(0, 0, width, height);
      const gap = 2;
      const barW = Math.max(1, (width - gap * (BARS - 1)) / BARS);
      context.beginPath();
      for (let index = 0; index < BARS; index += 1) {
        const level = shown[index];
        const barH = Math.max(2, level * (height - 2));
        const x = index * (barW + gap);
        const y = (height - barH) / 2;
        context.rect(x, y, barW, barH);
      }
      context.shadowColor = glow;
      context.shadowBlur = 6;
      context.fillStyle = main;
      context.fill();
      context.shadowBlur = 0;
      context.strokeStyle = border;
      context.lineWidth = 1;
      context.stroke();
    };

    const loop = (time: number) => {
      if (stopped) return;
      const dt = lastTime === 0 ? 0.016 : Math.min(0.05, (time - lastTime) / 1000);
      lastTime = time;
      if (!document.hidden) {
        follow(dt);
        paint();
      }
      frame = requestAnimationFrame(loop);
    };

    if (playing && document.visibilityState !== "hidden") {
      frame = requestAnimationFrame(loop);
    } else {
      paint();
    }

    const onVisibility = () => {
      if (document.hidden) {
        cancelAnimationFrame(frame);
        frame = 0;
        return;
      }
      if (playing && frame === 0) frame = requestAnimationFrame(loop);
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      stopped = true;
      cancelAnimationFrame(frame);
      unlisten();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [border, glow, main, playing]);

  return (
    <div className={variant === "bar" ? "viz-bar" : "viz-stage"} aria-hidden>
      <canvas ref={canvasRef} className="block h-full w-full" />
    </div>
  );
}

import React, { useEffect, useRef } from "react";

/** Seven bars like Sokki's mark, the middle one tallest. */
const SHAPE = [0.62, 0.8, 0.93, 1, 0.93, 0.8, 0.62];
/** Fast attack, slower release (time constants). */
const ATTACK_MS = 30;
const RELEASE_MS = 200;
const BAR_W = 2.5;
const GAP = 2;
const WIDTH = SHAPE.length * BAR_W + (SHAPE.length - 1) * GAP;
const HEIGHT = 18;
const MIN_H = 3; // silence: short bars, still there

/**
 * Seven level bars mirrored around the centre line, the design system's
 * RecordingPill. They follow the input level, each with its own slow sway,
 * so speech moves them and silence leaves short, still bars. Drawn on a
 * canvas at display refresh rate.
 *
 * `levelRef` holds the latest input level (0..1, on a loudness scale);
 * `dim` (while stopping) lets the bars settle and fade.
 */
export const Wave: React.FC<{
  levelRef: React.RefObject<number>;
  dim?: boolean;
}> = ({ levelRef, dim = false }) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dimRef = useRef(dim);
  dimRef.current = dim;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(WIDTH * dpr);
    canvas.height = Math.round(HEIGHT * dpr);
    canvas.style.width = `${WIDTH}px`;
    canvas.style.height = `${HEIGHT}px`;
    ctx.scale(dpr, dpr);
    const reduced = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;

    let env = 0; // smoothed level
    let frame = 0;
    let last = performance.now();

    const draw = (now: number) => {
      const dt = Math.min(100, now - last);
      last = now;
      const level = dimRef.current
        ? 0
        : Math.max(0, Math.min(1, levelRef.current ?? 0));
      const tau = level > env ? ATTACK_MS : RELEASE_MS;
      env += (level - env) * (1 - Math.exp(-dt / tau));
      ctx.clearRect(0, 0, WIDTH, HEIGHT);
      ctx.fillStyle = dimRef.current
        ? "rgba(255,255,255,0.4)"
        : "rgba(255,255,255,0.92)";
      for (let i = 0; i < SHAPE.length; i++) {
        // Each bar its own share and pace, so the pill looks alive.
        const sway = reduced
          ? 1
          : 0.82 + 0.18 * Math.sin(now / (140 + i * 23) + i * 1.7);
        const h = Math.max(MIN_H, env * HEIGHT * SHAPE[i] * sway);
        ctx.beginPath();
        ctx.roundRect(i * (BAR_W + GAP), (HEIGHT - h) / 2, BAR_W, h, BAR_W / 2);
        ctx.fill();
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [levelRef]);

  return <canvas ref={canvasRef} aria-hidden="true" data-tauri-drag-region />;
};

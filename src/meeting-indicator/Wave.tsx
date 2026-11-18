import React, { useEffect, useRef } from "react";

const BAR_W = 2;
const GAP = 2;
const STEP = BAR_W + GAP;
const WIDTH = 64;
const HEIGHT = 20;
const BARS = Math.ceil(WIDTH / STEP) + 1;
/** A new bar enters on the right this often. */
const BAR_MS = 55;
const MIN_H = 2; // silence: a dot on the centre line

/**
 * Scrolling voice waveform, after Superwhisper's recording window: many thin
 * bars mirrored around a centre line, newest on the right, sliding left
 * smoothly. Each bar is the loudness of its moment, so syllables show as
 * swells and pauses as a dotted line; louder bars are brighter. Drawn on a
 * canvas at display refresh rate.
 *
 * `levelRef` holds the latest input level (0..1, on a loudness scale).
 */
export const Wave: React.FC<{ levelRef: React.RefObject<number> }> = ({
  levelRef,
}) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);

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

    const history: number[] = new Array(BARS).fill(0);
    let peak = 0; // highest level since the last bar
    let env = 0; // smoothed level
    let lastBar = performance.now();
    let frame = 0;

    const draw = (now: number) => {
      const level = Math.max(0, Math.min(1, levelRef.current ?? 0));
      // Fast attack, slow release, so a bar shows the syllable it caught.
      env += (level - env) * (level > env ? 0.6 : 0.15);
      peak = Math.max(peak, env);
      while (now - lastBar >= BAR_MS) {
        history.shift();
        history.push(peak);
        peak = env;
        lastBar += BAR_MS;
      }
      // Sub-bar offset for a continuous slide between new bars.
      const slide = reduced ? 0 : ((now - lastBar) / BAR_MS) * STEP;
      ctx.clearRect(0, 0, WIDTH, HEIGHT);
      for (let i = 0; i < BARS; i++) {
        const v = history[i];
        // Fade out towards the left edge.
        const x = i * STEP - slide;
        if (x < -BAR_W) continue;
        const edge = Math.min(1, (x + STEP) / (STEP * 6));
        const h = Math.max(MIN_H, v * HEIGHT);
        const alpha = (0.35 + 0.65 * Math.min(1, v * 1.6)) * edge;
        ctx.fillStyle = `rgba(255,255,255,${alpha})`;
        ctx.beginPath();
        ctx.roundRect(x, (HEIGHT - h) / 2, BAR_W, h, BAR_W / 2);
        ctx.fill();
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [levelRef]);

  return <canvas ref={canvasRef} aria-hidden="true" data-tauri-drag-region />;
};

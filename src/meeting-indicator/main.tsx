import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import {
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
} from "@tauri-apps/api/window";
import { commands, events } from "@/bindings";
import { Wave } from "./Wave";
import "./indicator.css";

/** The pill's height; the window grows by TIP_SPACE below it for the tooltip. */
const PILL_HEIGHT = 40;
const TIP_SPACE = 32;
/** Hover this long before the tooltip shows. */
const TIP_DELAY_MS = 500;

/** "1:05" or "1:02:03". */
function elapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/**
 * RMS (0..1) to bar height (0..1): -55 dBFS is nothing, -15 dBFS full, with a
 * slight curve so normal speech fills most of the bars. Measured on 90 s of
 * Robin's own meeting audio (speech median -31 dBFS): the middle bar sits at
 * about 77 % for normal speech, 58 % quiet, 87 % loud (was 65/42/80 %; Robin:
 * "man müsste brüllen, damit das ausschlägt").
 */
function toHeight(rms: number): number {
  if (rms <= 0) return 0;
  const db = 20 * Math.log10(rms);
  return Math.min(1, Math.max(0, (db + 55) / 40)) ** 0.6;
}

/** Fits the window to the pill (and the tooltip), keeping its centre. */
async function fitWindow(width: number, height: number) {
  const win = getCurrentWindow();
  const scale = await win.scaleFactor();
  const pos = (await win.outerPosition()).toLogical(scale);
  const size = (await win.outerSize()).toLogical(scale);
  if (
    Math.abs(size.width - width) < 0.5 &&
    Math.abs(size.height - height) < 0.5
  )
    return;
  const centre = pos.x + size.width / 2;
  await win.setSize(new LogicalSize(width, height));
  if (Math.abs(size.width - width) >= 0.5)
    await win.setPosition(new LogicalPosition(centre - width / 2, pos.y));
}

/**
 * Floating pill while a meeting records (after Superwhisper's mini window):
 * the live waveform shows the microphone and system audio are picked up,
 * then the time and the stop button. The window is as wide as the pill.
 */
const Indicator: React.FC = () => {
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const [stopping, setStopping] = useState(false);
  const [hover, setHover] = useState(false);
  const [tip, setTip] = useState(false);
  const [pressed, setPressed] = useState(false);
  const level = useRef(0);
  const pill = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const load = async () => {
      const r = await commands.listMeetings();
      if (r.status !== "ok") return;
      const rec = r.data.find((m) => m.status.kind === "recording");
      if (rec) setStartedAt(Date.parse(rec.started_at));
    };
    void load();
    const offChanged = events.meetingChanged.listen(() => void load());
    const offLevel = listen<{ mic: number; system: number }>(
      "meeting-level",
      (e) => {
        // Smoothing happens in the canvas animation.
        level.current = toHeight(Math.max(e.payload.mic, e.payload.system));
      },
    );
    // The app tells the page when the mouse is over the stop button: macOS
    // gives a background app's page no hover of its own.
    const offHover = listen<boolean>("meeting-indicator-hover", (e) =>
      setHover(e.payload),
    );
    return () => {
      offChanged.then((fn) => fn());
      offLevel.then((fn) => fn());
      offHover.then((fn) => fn());
    };
  }, []);

  // The time stands still once stopping.
  useEffect(() => {
    if (stopping) return;
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [stopping]);

  // The tooltip after half a second of hover.
  useEffect(() => {
    if (!hover || stopping) {
      setTip(false);
      setPressed(false);
      return;
    }
    const timer = window.setTimeout(() => setTip(true), TIP_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [hover, stopping]);

  useLayoutEffect(() => {
    const el = pill.current;
    if (!el) return;
    const observer = new ResizeObserver(() =>
      setWidth(Math.ceil(el.getBoundingClientRect().width)),
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (width > 0)
      void fitWindow(width, PILL_HEIGHT + (tip ? TIP_SPACE : 0)).catch((e) =>
        console.warn("Fitting the pill window failed:", e),
      );
  }, [width, tip]);

  const stop = async () => {
    setStopping(true);
    const r = await commands.stopMeetingRecording();
    // Failed: the button must work again.
    if (r.status !== "ok") setStopping(false);
  };

  const stopClass = [
    "stop",
    hover && !stopping ? "is-hover" : "",
    pressed && !stopping ? "is-down" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className="frame">
      <div
        ref={pill}
        className={stopping ? "pill is-stopping" : "pill"}
        data-tauri-drag-region
      >
        <div className="wave" data-tauri-drag-region>
          <Wave levelRef={level} dim={stopping} />
        </div>
        <span className="time" data-tauri-drag-region>
          {startedAt ? elapsed(now - startedAt) : "0:00"}
        </span>
        <button
          type="button"
          className={stopClass}
          onClick={() => void stop()}
          onMouseDown={() => setPressed(true)}
          onMouseUp={() => setPressed(false)}
          disabled={stopping}
          aria-label={stopping ? "Wird beendet" : "Aufnahme beenden"}
        >
          {stopping ? <span className="spin" /> : <span className="square" />}
        </button>
      </div>
      {tip && <div className="tip">Aufnahme beenden</div>}
    </div>
  );
};

if (import.meta.env.PROD) {
  window.addEventListener("contextmenu", (e) => e.preventDefault());
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Indicator />
  </React.StrictMode>,
);

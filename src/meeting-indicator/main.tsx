import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { commands, events } from "@/bindings";
import { Wave } from "./Wave";
import "./indicator.css";

/** "1:05" or "1:02:03". */
function elapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** RMS (0..1) to bar height (0..1) on a dB scale, so speech fills the bars. */
function toHeight(rms: number): number {
  if (rms <= 0) return 0;
  const db = 20 * Math.log10(rms);
  return Math.min(1, Math.max(0, (db + 55) / 40));
}

/**
 * Floating pill while a meeting records (after Superwhisper's mini window):
 * a live waveform shows the microphone and system audio are picked up;
 * hovering shows the stop button.
 */
const Indicator: React.FC = () => {
  const [startedAt, setStartedAt] = useState<number | null>(null);
  const [now, setNow] = useState(Date.now());
  const [stopping, setStopping] = useState(false);
  const level = useRef(0);

  useEffect(() => {
    const load = async () => {
      const r = await commands.listMeetings();
      if (r.status !== "ok") return;
      const rec = r.data.find((m) => m.status.kind === "recording");
      setStartedAt(rec ? Date.parse(rec.started_at) : null);
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
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => {
      window.clearInterval(timer);
      offChanged.then((fn) => fn());
      offLevel.then((fn) => fn());
    };
  }, []);

  const stop = async () => {
    setStopping(true);
    await commands.stopMeetingRecording();
  };

  return (
    <div className="pill" data-tauri-drag-region>
      <span className="dot" data-tauri-drag-region />
      <div className="wave" data-tauri-drag-region>
        <Wave levelRef={level} />
      </div>
      <span className="time" data-tauri-drag-region>
        {startedAt ? elapsed(now - startedAt) : "0:00"}
      </span>
      <button
        type="button"
        className="stop"
        onClick={() => void stop()}
        disabled={stopping}
        title="Aufnahme beenden"
        aria-label="Aufnahme beenden"
      >
        <span className="square" />
      </button>
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

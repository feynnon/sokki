import { useEffect, useRef, useState } from "react";
import { commands } from "@/bindings";

/** Plays a few seconds of a speaker. One clip at a time across the app. */
let current: HTMLAudioElement | null = null;

export function useVoiceClip() {
  const [playing, setPlaying] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mine = useRef<HTMLAudioElement | null>(null);
  useEffect(
    () => () => {
      mine.current?.pause();
    },
    [],
  );

  const stop = () => {
    current?.pause();
    current = null;
    setPlaying(null);
  };

  /** `key` names what plays, so its button can show it. */
  const play = async (
    key: string,
    meetingId: string,
    speaker: string,
    startS: number | null,
  ) => {
    if (playing === key) {
      stop();
      return;
    }
    stop();
    setError(null);
    // Shown at once; the clip follows.
    setPlaying(key);
    const r = await commands.speakerClip(meetingId, speaker, startS);
    if (r.status !== "ok") {
      setPlaying(null);
      setError(r.error);
      return;
    }
    const audio = new Audio(r.data);
    current = audio;
    mine.current = audio;
    audio.onended = () => setPlaying((p) => (p === key ? null : p));
    audio.onpause = () => setPlaying((p) => (p === key ? null : p));
    try {
      await audio.play();
    } catch (e) {
      setPlaying(null);
      setError(String(e));
    }
  };

  return { playing, error, play, stop };
}

/** "Mara Keller, m@x.de; o@y.de" → addresses, trimmed, lower-case. */
export function parseEmails(text: string): string[] {
  return Array.from(
    new Set(
      text
        .split(/[,;\s]+/)
        .map((e) => e.trim().toLowerCase())
        .filter((e) => e.includes("@")),
    ),
  );
}

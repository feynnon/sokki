import { useEffect, useState } from "react";

/** Native side: MainActivity.KeyboardSetup (Android only). */
interface Bridge {
  state(): "off" | "on" | "active";
  micAllowed(): boolean;
  turnOn(): void;
  choose(): void;
  allowMic(): void;
  openSettings(): void;
}

const bridge = (): Bridge | undefined =>
  (window as unknown as { SokkiKeyboard?: Bridge }).SokkiKeyboard;

/** Voice typing in every app: turn on and choose the Sokki keyboard. */
export function KeyboardSetup() {
  const b = bridge();
  const [state, setState] = useState(() => b?.state() ?? "off");
  const [mic, setMic] = useState(() => b?.micAllowed() ?? false);

  // Android's settings and picker are separate screens: re-check on return.
  useEffect(() => {
    if (!b) return;
    const check = () => {
      setState(b.state());
      setMic(b.micAllowed());
    };
    const timer = setInterval(check, 1000);
    window.addEventListener("focus", check);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", check);
    };
  }, [b]);

  if (!b) return null;
  if (state === "active" && mic) {
    return (
      <section className="kb kb-done">
        <p className="kb-title">Sokki keyboard is on</p>
        <p className="muted small">
          Tap the mic at the top right of the keyboard to speak into any app.{" "}
          <button className="link inline" onClick={() => b.openSettings()}>
            Keyboard settings
          </button>
        </p>
      </section>
    );
  }
  return (
    <section className="kb">
      <p className="kb-title">Speak into any app</p>
      <p className="muted small">
        The Sokki keyboard types like a normal keyboard and has a mic at the top
        right. Two steps, once:
      </p>
      <button
        className={state === "off" ? "primary" : "secondary"}
        disabled={state !== "off"}
        onClick={() => b.turnOn()}
      >
        {state === "off" ? "1. Turn on the Sokki keyboard" : "1. Turned on ✓"}
      </button>
      <button
        className={state === "on" ? "primary" : "secondary"}
        disabled={state === "off"}
        onClick={() => b.choose()}
      >
        {state === "active" ? "2. Chosen ✓" : "2. Choose Sokki"}
      </button>
      {state === "active" && !mic && (
        <button className="primary" onClick={() => b.allowMic()}>
          3. Allow the microphone
        </button>
      )}
      {state === "off" && (
        <p className="muted small">
          In the list that opens, switch on "Sokki" and confirm. Android warns
          that a keyboard can read what you type: Sokki keeps everything on your
          phone.
        </p>
      )}
    </section>
  );
}

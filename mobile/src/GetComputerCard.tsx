import { useEffect, useState } from "react";

/** The download page; `s` tells sokki.org which surface the visit came from. */
const GET_URL = "https://sokki.org/get?s=android";
const DICTATIONS_NEEDED = 20;
const DISMISSED = "sokki.getComputerDismissed";

interface Links {
  share(text: string): void;
  keyboardDictations(): number;
}
const links = () => (window as unknown as { SokkiLinks?: Links }).SokkiLinks;

function dictations(): number {
  let n = 0;
  try {
    n = Number(localStorage.getItem("sokki.dictations") ?? "0");
  } catch {
    /* ignore */
  }
  return n + (links()?.keyboardDictations() ?? 0);
}

/** "Get Sokki on your computer": only for people who use Sokki already
 * (a finished meeting note, or 20 dictations in the app and keyboard
 * together), never at first launch, and not when a Mac is paired. The
 * person shares the link themselves; Sokki sends nothing. */
export function GetComputerCard({
  paired,
  finishedRecording,
}: {
  paired: boolean;
  finishedRecording: boolean;
}) {
  const [dismissed, setDismissed] = useState(() => {
    try {
      return localStorage.getItem(DISMISSED) === "1";
    } catch {
      return false;
    }
  });
  const [used, setUsed] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const check = () =>
      setUsed(finishedRecording || dictations() >= DICTATIONS_NEEDED);
    check();
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
  }, [finishedRecording]);

  if (paired || dismissed || !used) return null;

  const dismiss = () => {
    setDismissed(true);
    try {
      localStorage.setItem(DISMISSED, "1");
    } catch {
      /* ignore */
    }
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(GET_URL);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* the link is shown below */
    }
  };

  return (
    <section className="kb">
      <div className="card-head">
        <p className="kb-title">Get Sokki on your computer</p>
        <button className="close" aria-label="Don't show again" onClick={dismiss}>
          ×
        </button>
      </div>
      <p className="muted small">
        Meetings you record here become notes there, with summary and tasks.
        Free, and it runs on your own computer.
      </p>
      <div className="row">
        {links() && (
          <button className="primary" onClick={() => links()?.share(GET_URL)}>
            Send the link
          </button>
        )}
        <button className="secondary" onClick={() => void copy()}>
          {copied ? "Copied" : "Copy link"}
        </button>
      </div>
      <p className="muted small">sokki.org/get</p>
    </section>
  );
}

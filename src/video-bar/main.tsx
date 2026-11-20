import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { useTranslation } from "react-i18next";
import { Camera, Monitor, PictureInPicture2, X } from "lucide-react";
import "@/i18n";
import { commands, type VideoSettings, type VideoSources } from "@/bindings";
import "./bar.css";

type Mode = "both" | "screen" | "camera";
type Look = "bubble" | "side" | "vertical" | "files";

/**
 * The recording bar, after Loom's: what to record, which screen, the look,
 * which camera, then Record. The camera bubble is a native panel shown while
 * the camera is part of the recording.
 */
const Bar: React.FC = () => {
  const { t } = useTranslation();
  const [sources, setSources] = useState<VideoSources | null>(null);
  const [settings, setSettings] = useState<VideoSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    commands.videoSources().then((r) => r.status === "ok" && setSources(r.data));
    commands.getAppSettings().then((r) => {
      if (r.status === "ok" && r.data.videos) setSettings(r.data.videos);
    });
  }, []);

  const mode = (settings?.mode ?? "both") as Mode;
  const look = (settings?.look ?? "bubble") as Look;
  const withCamera = mode !== "screen";
  const withScreen = mode !== "camera";

  // The bubble shows while the camera is part of the recording.
  useEffect(() => {
    if (!settings) return;
    commands.videoBubble(withCamera, settings.camera ?? null);
  }, [withCamera, settings?.camera, settings !== null]);

  const update = (patch: Partial<VideoSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    commands.setVideoSettings(next);
  };

  const close = () => commands.videoBarHide();

  const record = async () => {
    if (!settings) return;
    setBusy(true);
    const r = await commands.videoStart({
      kind: "video",
      mode,
      display: withScreen ? settings.display : null,
      camera: withCamera ? settings.camera : null,
      look,
      source_app: null,
    });
    setBusy(false);
    if (r.status === "error") {
      if (r.error === "screen_not_allowed") setMessage(t("video.bar.screenNotAllowed"));
      else if (r.error === "camera_not_allowed") setMessage(t("video.bar.cameraNotAllowed"));
      else setMessage(r.error);
    }
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const displays = sources?.displays ?? [];
  const cameras = sources?.cameras ?? [];

  return (
    <div className="bar" role="toolbar" aria-label={t("video.bar.title")}>
      <button type="button" className="icon-button" onClick={close} aria-label={t("video.bar.close")}>
        <X width={16} height={16} />
      </button>
      {message ? (
        <p className="message">{message}</p>
      ) : (
        <>
          <label className="pick">
            <Monitor width={16} height={16} aria-hidden />
            <select value={mode} onChange={(e) => update({ mode: e.target.value })}>
              <option value="both">{t("video.bar.modeBoth")}</option>
              <option value="screen">{t("video.bar.modeScreen")}</option>
              <option value="camera">{t("video.bar.modeCamera")}</option>
            </select>
          </label>
          {withScreen && displays.length > 1 && (
            <label className="pick">
              <select
                value={settings?.display ?? displays.find((d) => d.main)?.id ?? displays[0].id}
                onChange={(e) => update({ display: Number(e.target.value) })}
              >
                {displays.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          {withScreen && withCamera && (
            <label className="pick">
              <PictureInPicture2 width={16} height={16} aria-hidden />
              <select value={look} onChange={(e) => update({ look: e.target.value })}>
                <option value="bubble">{t("video.bar.lookBubble")}</option>
                <option value="side">{t("video.bar.lookSide")}</option>
                <option value="vertical">{t("video.bar.lookVertical")}</option>
                <option value="files">{t("video.bar.lookFiles")}</option>
              </select>
            </label>
          )}
          {withCamera && cameras.length > 0 && (
            <>
              <span className="sep" aria-hidden />
              <label className="pick">
                <Camera width={16} height={16} aria-hidden />
                <select
                  value={settings?.camera ?? cameras[0].id}
                  onChange={(e) => update({ camera: e.target.value })}
                >
                  {cameras.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name}
                    </option>
                  ))}
                </select>
              </label>
            </>
          )}
          <span className="grow" />
        </>
      )}
      {!message && (
        <button type="button" className="record" onClick={record} disabled={busy || !settings}>
          <span className="dot" aria-hidden />
          {t("video.bar.record")}
        </button>
      )}
    </div>
  );
};

window.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Bar />
  </React.StrictMode>,
);

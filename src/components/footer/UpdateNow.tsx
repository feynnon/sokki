import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type ReadyUpdate } from "@/bindings";

/**
 * "Update now" once a newer Sokki is downloaded (src-tauri/src/auto_update.rs):
 * the new version, the button, and roughly how long Sokki is gone. Hidden
 * while something records or a meeting is processed.
 */
const UpdateNow: React.FC = () => {
  const { t, i18n } = useTranslation();
  const [update, setUpdate] = useState<ReadyUpdate | null>(null);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    const refresh = () => commands.getReadyUpdate().then(setUpdate);
    refresh();
    const timer = setInterval(refresh, 15_000);
    const unlisten = listen("update-ready", refresh);
    return () => {
      clearInterval(timer);
      unlisten.then((fn) => fn());
    };
  }, []);

  if (!update) return null;

  const seconds = new Intl.NumberFormat(i18n.language, {
    style: "unit",
    unit: "second",
    unitDisplay: "narrow",
  }).format(update.seconds);

  const detail = `${update.version} · ~${seconds}`;

  const install = async () => {
    setInstalling(true);
    const result = await commands.updateNow();
    // On success the app quits and the new version starts.
    if (result.status === "error") {
      console.error("Update failed:", result.error);
      setInstalling(false);
    }
  };

  return (
    <div className="flex shrink-0 items-center gap-1.5 whitespace-nowrap">
      <button
        type="button"
        disabled={installing}
        onClick={install}
        className="cursor-pointer rounded-sm font-medium text-logo-primary transition-colors hover:text-text disabled:cursor-default disabled:opacity-60 focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
      >
        {t("footer.updateNow")}
      </button>
      <span>{detail}</span>
      <span>•</span>
    </div>
  );
};

export default UpdateNow;

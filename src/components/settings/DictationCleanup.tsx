import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { commands, type CleanupModel, type LlmDownload } from "@/bindings";
import { Button } from "../ui/Button";
import { SettingContainer } from "../ui/SettingContainer";
import { Switch } from "../ui/Switch";
import { useSettings } from "../../hooks/useSettings";
import { gigabytes } from "../../hooks/useSummaryModel";

interface DictationCleanupProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Cleans up each dictation with the built-in cleanup model (Gemma 4 E4B,
 * `src-tauri/src/dictation_cleanup.rs`). Off by default; when on, it works
 * once the model is downloaded, which this row offers. */
export const DictationCleanup: React.FC<DictationCleanupProps> = React.memo(
  ({ descriptionMode = "inline", grouped = false }) => {
    const { t, i18n } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const enabled = getSetting("dictation_cleanup_enabled") ?? false;
    const [model, setModel] = useState<CleanupModel | null>(null);
    const [download, setDownload] = useState<LlmDownload | null>(null);
    const [failed, setFailed] = useState(false);

    const refresh = useCallback(async () => {
      const m = await commands.dictationCleanupModel();
      setModel(m);
      setDownload(m.download);
    }, []);

    useEffect(() => {
      void refresh();
      const unlisten = listen<LlmDownload>("dictation-cleanup-pull", (e) =>
        setDownload(e.payload),
      );
      return () => {
        unlisten.then((fn) => fn());
      };
    }, [refresh]);

    // The last event comes before the files are in place.
    const running = !!download;
    useEffect(() => {
      if (!running) return;
      const timer = setInterval(() => void refresh(), 2000);
      return () => clearInterval(timer);
    }, [running, refresh]);

    const start = useCallback(() => {
      setFailed(false);
      setDownload({ status: "starting", completed: 0, total: 0 });
      void commands.downloadDictationCleanupModel().then((r) => {
        if (r.status === "error") setFailed(true);
        void refresh();
      });
    }, [refresh]);

    if (model?.state === "unavailable") return null;
    const missing = model?.state === "missing";
    const percent =
      download && download.total > 0
        ? Math.floor((download.completed / download.total) * 100)
        : 0;
    const label = t("sokki.settings.cleanup");

    return (
      <SettingContainer
        title={label}
        description={
          failed
            ? t("sokki.settings.cleanupFailed")
            : t("sokki.settings.cleanupDescription")
        }
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="flex items-center gap-3">
          {missing && enabled && !download && (
            <Button variant="secondary" size="sm" onClick={start}>
              {t("sokki.settings.cleanupDownload", {
                size: gigabytes(model.size, i18n.language),
              })}
            </Button>
          )}
          {missing && download && (
            <span className="text-xs text-ink-3 tabular-nums">
              {t("sokki.settings.cleanupDownloading", { percent })}
            </span>
          )}
          <Switch
            checked={enabled}
            onChange={(next) =>
              updateSetting("dictation_cleanup_enabled", next)
            }
            disabled={isUpdating("dictation_cleanup_enabled")}
            ariaLabel={label}
          />
        </div>
      </SettingContainer>
    );
  },
);

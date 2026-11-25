import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../ui/ToggleSwitch";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";

interface AutocorrectProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

/** Fixes typos as you type, in the apps where macOS doesn't
 * (`src-tauri/src/autocorrect.rs`). Off by default; macOS only. */
export const Autocorrect: React.FC<AutocorrectProps> = React.memo(
  ({ descriptionMode = "inline", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();
    const os = useOsType();
    if (os !== "macos") return null;
    return (
      <ToggleSwitch
        checked={getSetting("autocorrect_enabled") ?? false}
        onChange={(next) => updateSetting("autocorrect_enabled", next)}
        isUpdating={isUpdating("autocorrect_enabled")}
        label={t("sokki.settings.autocorrect")}
        description={t("sokki.settings.autocorrectDescription")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      />
    );
  },
);

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
    const on = getSetting("autocorrect_enabled") ?? false;
    return (
      <>
        <ToggleSwitch
          checked={on}
          onChange={(next) => updateSetting("autocorrect_enabled", next)}
          isUpdating={isUpdating("autocorrect_enabled")}
          label={t("sokki.settings.autocorrect")}
          description={t("sokki.settings.autocorrectDescription")}
          descriptionMode={descriptionMode}
          grouped={grouped}
        />
        {/* The dictionary pill (src-tauri/src/dictionary.rs): only with
            autocorrect on, which tells it which words were typed. */}
        {on && (
          <ToggleSwitch
            checked={getSetting("autocorrect_add_words") ?? false}
            onChange={(next) => updateSetting("autocorrect_add_words", next)}
            isUpdating={isUpdating("autocorrect_add_words")}
            label={t("sokki.settings.autocorrectAddWords")}
            description={t("sokki.settings.autocorrectAddWordsDescription")}
            descriptionMode={descriptionMode}
            grouped={grouped}
          />
        )}
      </>
    );
  },
);

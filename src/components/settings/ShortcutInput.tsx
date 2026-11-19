import React, { useRef } from "react";
import { useTranslation } from "react-i18next";
import { formatKeyCombination } from "../../lib/utils/keyboard";
import { ResetButton } from "../ui/ResetButton";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import { useOsType } from "../../hooks/useOsType";
import { useShortcutCapture } from "../../hooks/useShortcutCapture";
import { usePressAway } from "../../hooks/usePressAway";
import { IS_SOKKI } from "@/lib/brand";

interface ShortcutInputProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
  shortcutId: string;
  disabled?: boolean;
}

/**
 * A binding's shortcut; click to record a new one. Clicking elsewhere cancels;
 * Escape is recorded like any key (Cancel's default is Escape).
 */
export const ShortcutInput: React.FC<ShortcutInputProps> = ({
  descriptionMode = "tooltip",
  grouped = false,
  shortcutId,
  disabled = false,
}) => {
  const { t } = useTranslation();
  const { getSetting, resetBinding, isUpdating, isLoading } = useSettings();
  const osType = useOsType();
  const { capturing, partial, start, cancel } = useShortcutCapture(
    shortcutId,
    osType,
  );
  const shortcutRef = useRef<HTMLDivElement | null>(null);

  const bindings = getSetting("bindings") || {};

  usePressAway(capturing, shortcutRef, cancel);

  // If still loading, show loading state
  if (isLoading) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-ink-3">
          {t("settings.general.shortcut.loading")}
        </div>
      </SettingContainer>
    );
  }

  // If no bindings are loaded, show empty state
  if (Object.keys(bindings).length === 0) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-ink-3">
          {t("settings.general.shortcut.none")}
        </div>
      </SettingContainer>
    );
  }

  const binding = bindings[shortcutId];
  if (!binding) {
    return (
      <SettingContainer
        title={t("settings.general.shortcut.title")}
        description={t("settings.general.shortcut.notFound")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <div className="text-sm text-ink-3">
          {t("settings.general.shortcut.none")}
        </div>
      </SettingContainer>
    );
  }

  // Get translated name and description for the binding
  const translatedName = t(
    `settings.general.shortcut.bindings.${shortcutId}.name`,
    binding.name,
  );
  const translatedDescription = t(
    `settings.general.shortcut.bindings.${shortcutId}.description`,
    binding.description,
  );

  return (
    <SettingContainer
      title={translatedName}
      description={translatedDescription}
      descriptionMode={descriptionMode}
      grouped={grouped}
      disabled={disabled}
      layout="horizontal"
    >
      {IS_SOKKI ? (
        // The key itself, at the control height, right-aligned with the
        // other controls; reset shows only once the shortcut was changed.
        <div className="flex items-center gap-1.5">
          {binding.current_binding !== binding.default_binding && (
            <ResetButton
              ariaLabel={t("common.resetSetting", { setting: translatedName })}
              onClick={() => resetBinding(shortcutId)}
              disabled={isUpdating(`binding_${shortcutId}`)}
              className="text-ink-3"
            />
          )}
          {capturing ? (
            <div
              ref={shortcutRef}
              className="inline-flex h-[var(--vb-control-h)] items-center rounded-[8px] border border-logo-primary bg-logo-primary/15 px-3 text-[13px] font-medium"
            >
              {partial
                ? formatKeyCombination(partial, osType)
                : t("settings.general.shortcut.pressKeys")}
            </div>
          ) : (
            <button
              type="button"
              className="inline-flex h-[var(--vb-control-h)] cursor-pointer items-center rounded-[8px] border border-line bg-wash px-3 text-[13px] font-medium transition-colors duration-150 hover:border-logo-primary focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
              onClick={start}
            >
              {formatKeyCombination(binding.current_binding, osType)}
            </button>
          )}
        </div>
      ) : (
        <div className="flex items-center space-x-1">
          {capturing ? (
            <div
              ref={shortcutRef}
              className="px-2 py-1 text-sm font-semibold border border-logo-primary bg-logo-primary/20 rounded-md"
            >
              {partial
                ? formatKeyCombination(partial, osType)
                : t("settings.general.shortcut.pressKeys")}
            </div>
          ) : (
            <div
              className="px-2 py-1 text-sm font-semibold bg-wash border border-line-strong hover:bg-logo-primary/10 rounded-md cursor-pointer hover:border-logo-primary"
              onClick={start}
            >
              {formatKeyCombination(binding.current_binding, osType)}
            </div>
          )}
          <ResetButton
            ariaLabel={t("common.resetSetting", { setting: translatedName })}
            onClick={() => resetBinding(shortcutId)}
            disabled={isUpdating(`binding_${shortcutId}`)}
          />
        </div>
      )}
    </SettingContainer>
  );
};

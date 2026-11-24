import { commands, type Theme } from "@/bindings";

/**
 * Appearance theme handling.
 *
 * Sokki is dark whatever macOS is set to. The Light setting sets
 * `data-theme="light"`, whose CSS selectors switch the palette (see
 * `styles/theme.css`). A `system` value from older settings is shown dark.
 *
 * The choice is persisted in `AppSettings` (source of truth) and mirrored to
 * localStorage so it can be applied synchronously on boot, before React mounts,
 * avoiding a flash of the wrong palette.
 */

export const THEME_STORAGE_KEY = "sokki.theme";

export const THEME_OPTIONS: Theme[] = ["dark", "light"];

const isTheme = (value: unknown): value is Theme =>
  value === "system" || value === "light" || value === "dark";

/** Apply a theme to the document root and remember it for the next launch. */
export const applyTheme = (theme: Theme): void => {
  const root = document.documentElement;
  root.dataset.theme = theme === "light" ? "light" : "dark";
  try {
    localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    // localStorage may be unavailable (e.g. private mode); the setting still
    // persists in AppSettings, so this only costs a one-frame flash on boot.
  }
};

/** Read the last-applied theme for synchronous boot-time application. */
export const getStoredTheme = (): Theme => {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (isTheme(stored)) return stored;
  } catch {
    // ignore
  }
  return "dark";
};

/** Apply the persisted theme from AppSettings (the source of truth). */
export const syncThemeFromSettings = async (): Promise<void> => {
  try {
    const result = await commands.getAppSettings();
    if (result.status === "ok") {
      applyTheme(result.data.theme ?? "dark");
    }
  } catch (e) {
    console.warn("Failed to sync theme from settings:", e);
  }
};

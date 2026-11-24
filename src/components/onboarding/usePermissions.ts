import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { platform } from "@tauri-apps/plugin-os";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { notify } from "@/stores/noticeStore";
import { commands } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import { useOnboardingStore, type Uses } from "./onboardingStore";

type PermissionStatus = "checking" | "needed" | "waiting" | "granted";
type PermissionPlatform = "macos" | "windows" | "other";

export interface Permissions {
  platform: PermissionPlatform;
  microphone: PermissionStatus;
  accessibility: PermissionStatus;
  /** The first check is done. */
  checked: boolean;
  /** Screen Recording, for a meeting with its screen (macOS). */
  screen: PermissionStatus;
  /** System Audio Recording (macOS). macOS can't say whether it was
   * allowed, so "granted" means it was asked. */
  systemSound: PermissionStatus;
  /** Something still needs allowing for these uses (decided by the first
   * check, so the screen stays in the flow once everything is allowed). */
  neededFor: (uses: Uses) => boolean;
  /** Everything these uses need is allowed (or asked, for system sound). */
  allGrantedFor: (uses: Uses) => boolean;
  /** Asked once already: asking again opens the OS settings instead. */
  microphoneAsked: boolean;
  requestMicrophone: () => Promise<void>;
  requestAccessibility: () => Promise<void>;
  requestScreen: () => Promise<void>;
  requestSystemSound: () => Promise<void>;
}

const MAX_POLLING_ERRORS = 3;
// No answer seen this long after asking: offer the button again.
const WAITING_TIMEOUT_MS = 15_000;

/** macOS's answers: `[accessibility, microphone]`. */
const readMac = () =>
  Promise.all([checkAccessibilityPermission(), checkMicrophonePermission()]);

const currentPlatform = (): PermissionPlatform => {
  const p = platform();
  return p === "macos" ? "macos" : p === "windows" ? "windows" : "other";
};

/**
 * Microphone (macOS, Windows) and Accessibility (macOS) permissions: check,
 * request, then poll until granted. Granting Accessibility initializes input
 * so no relaunch is needed.
 */
export const usePermissions = (preview: boolean): Permissions => {
  const { t } = useTranslation();
  const refreshAudioDevices = useSettingsStore((s) => s.refreshAudioDevices);
  const refreshOutputDevices = useSettingsStore((s) => s.refreshOutputDevices);
  const [os] = useState(currentPlatform);
  const [microphone, setMicrophone] = useState<PermissionStatus>("checking");
  const [accessibility, setAccessibility] =
    useState<PermissionStatus>("checking");
  const [screen, setScreen] = useState<PermissionStatus>("checking");
  const systemSoundAsked = useOnboardingStore((s) => s.systemSoundAsked);
  const setOnboarding = useOnboardingStore((s) => s.set);
  const systemSound: PermissionStatus = systemSoundAsked ? "granted" : "needed";
  /** The first check's answers: what the flow shows doesn't change later. */
  const [first, setFirst] = useState<{
    mic: boolean;
    acc: boolean;
    screen: boolean;
    sound: boolean;
  } | null>(null);
  const [microphoneAsked, setMicrophoneAsked] = useState(false);
  const pollingRef = useRef<ReturnType<typeof setInterval> | null>(null);
  /** What was asked for: polling ends once those are allowed. */
  const askedRef = useRef({ mic: false, acc: false, scr: false });
  const errorCountRef = useRef(0);

  const isMacOS = os === "macos";
  const isWindows = os === "windows";
  const allGrantedFor = (uses: Uses) =>
    isMacOS
      ? microphone === "granted" &&
        (!uses.dictation || accessibility === "granted") &&
        (!uses.meetings || (screen === "granted" && systemSound === "granted"))
      : isWindows
        ? microphone === "granted"
        : true;
  const neededFor = (uses: Uses) =>
    !!first &&
    (!first.mic ||
      (isMacOS && uses.dictation && !first.acc) ||
      (isMacOS && uses.meetings && (!first.screen || !first.sound)));

  const hasWindowsMicrophoneAccess = useCallback(async () => {
    const status = await commands.getWindowsMicrophonePermissionStatus();
    return !status.supported || status.overall_access !== "denied";
  }, []);

  const initializeAfterAccessibility = () =>
    Promise.all([
      commands.initializeEnigo(),
      commands.initializeShortcuts(),
    ]).catch((e) => {
      console.warn("Failed to initialize after permission grant:", e);
    });

  const stopPolling = () => {
    if (pollingRef.current) {
      clearInterval(pollingRef.current);
      pollingRef.current = null;
    }
  };

  const onAllGranted = useCallback(async () => {
    await Promise.all([refreshAudioDevices(), refreshOutputDevices()]);
  }, [refreshAudioDevices, refreshOutputDevices]);

  // The first check: decides whether the Permissions screen is part of the flow.
  useEffect(() => {
    if (preview) {
      setAccessibility(isMacOS ? "needed" : "granted");
      setMicrophone(os === "other" ? "granted" : "needed");
      setScreen(isMacOS ? "needed" : "granted");
      setFirst({
        mic: os === "other",
        acc: !isMacOS,
        screen: !isMacOS,
        sound: !isMacOS,
      });
      return;
    }
    if (os === "other") {
      setAccessibility("granted");
      setMicrophone("granted");
      setScreen("granted");
      setFirst({ mic: true, acc: true, screen: true, sound: true });
      return;
    }
    const check = async () => {
      if (isMacOS) {
        try {
          const [acc, mic] = await readMac();
          const scr = await commands.screenPermission();
          if (acc) await initializeAfterAccessibility();
          setAccessibility(acc ? "granted" : "needed");
          setMicrophone(mic ? "granted" : "needed");
          setScreen(scr ? "granted" : "needed");
          setFirst({ mic, acc, screen: scr, sound: systemSoundAsked });
          if (acc && mic) await onAllGranted();
        } catch (error) {
          console.error("Failed to check macOS permissions:", error);
          notify({ title: t("onboarding.permissions.errors.checkFailed") });
          setAccessibility("needed");
          setMicrophone("needed");
          setScreen("needed");
          setFirst({
            mic: false,
            acc: false,
            screen: false,
            sound: systemSoundAsked,
          });
        }
        return;
      }
      setAccessibility("granted");
      setScreen("granted");
      try {
        const mic = await hasWindowsMicrophoneAccess();
        setMicrophone(mic ? "granted" : "needed");
        setFirst({ mic, acc: true, screen: true, sound: true });
        if (mic) await onAllGranted();
      } catch (error) {
        console.warn("Failed to check Windows microphone permissions:", error);
        setMicrophone("granted");
        setFirst({ mic: true, acc: true, screen: true, sound: true });
        await onAllGranted();
      }
    };
    check();
    // The first check runs once.
  }, []);

  const startPolling = useCallback(() => {
    if (pollingRef.current) return;
    pollingRef.current = setInterval(async () => {
      try {
        if (isWindows) {
          if (await hasWindowsMicrophoneAccess()) {
            setMicrophone("granted");
            stopPolling();
            await onAllGranted();
          }
          errorCountRef.current = 0;
          return;
        }
        const [acc, mic] = await readMac();
        const scr = await commands.screenPermission();
        setAccessibility((prev) => {
          if (acc && prev !== "granted") initializeAfterAccessibility();
          return acc ? "granted" : prev;
        });
        setMicrophone((prev) => (mic ? "granted" : prev));
        setScreen((prev) => (scr ? "granted" : prev));
        const asked = askedRef.current;
        if ((!asked.acc || acc) && (!asked.mic || mic) && (!asked.scr || scr)) {
          stopPolling();
          await onAllGranted();
        }
        errorCountRef.current = 0;
      } catch (error) {
        console.error("Error checking permissions:", error);
        errorCountRef.current += 1;
        if (errorCountRef.current >= MAX_POLLING_ERRORS) {
          stopPolling();
          notify({ title: t("onboarding.permissions.errors.checkFailed") });
        }
      }
    }, 1000);
  }, [hasWindowsMicrophoneAccess, isWindows, onAllGranted, t]);

  useEffect(() => stopPolling, []);

  // A waiting row whose answer was no (or never came): ask again.
  const recheckWaiting = useCallback(async () => {
    try {
      if (isWindows) {
        const mic = await hasWindowsMicrophoneAccess();
        setMicrophone((prev) =>
          prev === "waiting" ? (mic ? "granted" : "needed") : prev,
        );
        return;
      }
      if (!isMacOS) return;
      const [acc, mic] = await readMac();
      setAccessibility((prev) =>
        prev === "waiting" ? (acc ? "granted" : "needed") : prev,
      );
      setMicrophone((prev) =>
        prev === "waiting" ? (mic ? "granted" : "needed") : prev,
      );
    } catch (error) {
      console.warn("Failed to recheck permissions:", error);
    }
  }, [hasWindowsMicrophoneAccess, isMacOS, isWindows]);

  const waiting = microphone === "waiting" || accessibility === "waiting";
  useEffect(() => {
    if (preview || !waiting) return;
    const unlisten = getCurrentWindow().onFocusChanged(({ payload }) => {
      if (payload) recheckWaiting();
    });
    // Only while Sokki is in front: away in Settings, the row keeps waiting.
    const timer = setTimeout(() => {
      if (document.hasFocus()) recheckWaiting();
    }, WAITING_TIMEOUT_MS);
    return () => {
      clearTimeout(timer);
      unlisten.then((off) => off());
    };
  }, [preview, waiting, recheckWaiting]);

  const requestAccessibility = async () => {
    if (preview) return;
    try {
      await requestAccessibilityPermission();
      askedRef.current.acc = true;
      setAccessibility("waiting");
      startPolling();
    } catch (error) {
      console.error("Failed to request accessibility permission:", error);
      notify({ title: t("onboarding.permissions.errors.requestFailed") });
    }
  };

  const requestMicrophone = async () => {
    if (preview) return;
    try {
      // macOS prompts once; after that, only System Settings can allow it.
      if (isWindows || microphoneAsked)
        await commands.openMicrophonePrivacySettings();
      else {
        await requestMicrophonePermission();
        // A prompt takes focus from Sokki; still in front a moment later
        // means there was none (denied before), so go back to asking.
        setTimeout(() => {
          if (document.hasFocus()) recheckWaiting();
        }, 1000);
      }
      setMicrophoneAsked(true);
      askedRef.current.mic = true;
      setMicrophone("waiting");
      startPolling();
    } catch (error) {
      console.error("Failed to request microphone permission:", error);
      notify({ title: t("onboarding.permissions.errors.requestFailed") });
    }
  };

  /** macOS shows its prompt (or System Settings); once allowed, macOS asks
   * to quit and reopen Sokki, and the next start resumes here, welcomed back. */
  const requestScreen = async () => {
    if (preview) return;
    try {
      const r = await commands.requestScreenPermission();
      if (r.status === "ok" && r.data) {
        setScreen("granted");
        return;
      }
      setOnboarding({ relaunchPending: true });
      askedRef.current.scr = true;
      setScreen("waiting");
      startPolling();
    } catch (error) {
      console.error("Failed to request screen recording permission:", error);
      notify({ title: t("onboarding.permissions.errors.requestFailed") });
    }
  };

  const requestSystemSound = async () => {
    if (preview) return;
    const r = await commands.requestSystemAudioPermission();
    if (r.status === "error") {
      console.error("Failed to ask for system sound:", r.error);
      notify({ title: t("onboarding.permissions.errors.requestFailed") });
      return;
    }
    setOnboarding({ systemSoundAsked: true });
  };

  return {
    platform: os,
    microphone,
    accessibility,
    screen,
    systemSound,
    checked:
      microphone !== "checking" &&
      accessibility !== "checking" &&
      screen !== "checking" &&
      first !== null,
    neededFor,
    allGrantedFor,
    microphoneAsked,
    requestMicrophone,
    requestAccessibility,
    requestScreen,
    requestSystemSound,
  };
};

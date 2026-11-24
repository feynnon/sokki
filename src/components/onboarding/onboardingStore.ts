import { create } from "zustand";
import {
  CHINESE_SCRIPTS,
  suggestedLanguages,
  suggestedScript,
  type ChineseScriptChoice,
} from "@/lib/utils/languageSuggestions";

export type OnboardingStep =
  | "welcome"
  | "purpose"
  | "permissions"
  | "model"
  | "voice"
  | "demo"
  | "try"
  | "done";

export const STEP_ORDER: OnboardingStep[] = [
  "welcome",
  "purpose",
  "permissions",
  "model",
  "voice",
  "demo",
  "try",
  "done",
];

/** What Sokki is for. */
export interface Uses {
  dictation: boolean;
  meetings: boolean;
}

/** What the flow remembers between screens, and across a relaunch. */
interface Persisted {
  step: OnboardingStep;
  /** Spoken languages (codes), in the order they were picked. */
  languages: string[];
  script: ChineseScriptChoice;
  /** The selected model (catalog id); null means the recommendation. */
  modelId: string | null;
  /** A model picked from the full list: it stays on Model as a fourth row. */
  extraModelId: string | null;
  /** The model being downloaded, or already on disk (catalog id). */
  downloadId: string | null;
  /** What Sokki is for (the "What for?" screen); Permissions asks only
   * for what these need. */
  uses: Uses;
  /** System sound was asked for (macOS can't tell whether it was allowed). */
  systemSoundAsked: boolean;
  /** Back after the Accessibility relaunch: Permissions says so. */
  welcomeBack: boolean;
  /** Screen was asked for: macOS reopens Sokki once it's allowed, and the
   * next start says "Welcome back" (not this one). */
  relaunchPending: boolean;
  /** Which ways of using the shortcut they've done for real (Try it). */
  lessons: { hold: boolean; tap: boolean };
  /** Set when All set's panel jumps back to a screen; continuing returns here. */
  returnTo: OnboardingStep | null;
  /** Start Sokki at login: set on All set, saved when they finish. */
  atLogin: boolean;
}

interface OnboardingState extends Persisted {
  /** Debug preview: no permission prompts, downloads or setting writes. */
  preview: boolean;
  /** The download of `downloadId` stopped (failed, or cut off by a quit). */
  downloadFailed: boolean;

  set: (patch: Partial<Persisted & { downloadFailed: boolean }>) => void;
  setLessons: (patch: Partial<{ hold: boolean; tap: boolean }>) => void;
  /** Start over (a new person, or a preview). */
  reset: (preview: boolean) => void;
}

const STORAGE_KEY = "sokki.onboarding";

const fresh = (): Persisted => ({
  step: "welcome",
  languages: suggestedLanguages(),
  script: suggestedScript(),
  modelId: null,
  extraModelId: null,
  downloadId: null,
  uses: { dictation: true, meetings: true },
  systemSoundAsked: false,
  welcomeBack: false,
  relaunchPending: false,
  lessons: { hold: false, tap: false },
  returnTo: null,
  atLogin: true,
});

const isStringList = (v: unknown): v is string[] =>
  Array.isArray(v) && v.every((x) => typeof x === "string");

/** Saved progress, field by field: anything malformed starts fresh. */
const load = (): Persisted => {
  const base = fresh();
  try {
    const saved: unknown = JSON.parse(
      localStorage.getItem(STORAGE_KEY) ?? "null",
    );
    if (!saved || typeof saved !== "object") return base;
    const fields = saved as Record<string, unknown>;
    const pick = <K extends keyof Persisted>(
      key: K,
      ok: (v: unknown) => boolean,
    ): Persisted[K] =>
      ok(fields[key]) ? (fields[key] as Persisted[K]) : base[key];
    const isId = (v: unknown) => v === null || typeof v === "string";
    const isBool = (v: unknown) => typeof v === "boolean";
    return {
      step: pick("step", (v) => STEP_ORDER.includes(v as OnboardingStep)),
      languages: pick("languages", isStringList),
      script: pick("script", (v) => CHINESE_SCRIPTS.some((c) => c.id === v)),
      modelId: pick("modelId", isId),
      extraModelId: pick("extraModelId", isId),
      downloadId: pick("downloadId", isId),
      uses: pick(
        "uses",
        (v) =>
          !!v &&
          typeof v === "object" &&
          isBool((v as Uses).dictation) &&
          isBool((v as Uses).meetings),
      ),
      systemSoundAsked: pick("systemSoundAsked", isBool),
      // A start after the Screen prompt is the relaunch it was waiting for.
      welcomeBack:
        pick("welcomeBack", isBool) || fields.relaunchPending === true,
      relaunchPending: false,
      returnTo: pick(
        "returnTo",
        (v) => v === null || STEP_ORDER.includes(v as OnboardingStep),
      ),
      atLogin: pick("atLogin", isBool),
      lessons: pick(
        "lessons",
        (v) =>
          !!v &&
          typeof v === "object" &&
          isBool((v as Persisted["lessons"]).hold) &&
          isBool((v as Persisted["lessons"]).tap),
      ),
    };
  } catch {
    return base;
  }
};

const save = (state: OnboardingState) => {
  if (state.preview) return;
  const persisted: Persisted = {
    step: state.step,
    languages: state.languages,
    script: state.script,
    modelId: state.modelId,
    extraModelId: state.extraModelId,
    downloadId: state.downloadId,
    uses: state.uses,
    systemSoundAsked: state.systemSoundAsked,
    welcomeBack: state.welcomeBack,
    relaunchPending: state.relaunchPending,
    lessons: state.lessons,
    returnTo: state.returnTo,
    atLogin: state.atLogin,
  };
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(persisted));
  } catch {
    // localStorage unavailable: a relaunch starts over, nothing worse
  }
};

/** Forget the flow (it's done, or starting over), saved and in memory. */
export const clearOnboardingProgress = () => {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // ignore
  }
  useOnboardingStore.getState().reset(false);
};

export const useOnboardingStore = create<OnboardingState>()((set, get) => ({
  ...load(),
  preview: false,
  downloadFailed: false,

  set: (patch) => {
    set(patch);
    save(get());
  },
  setLessons: (patch) => {
    set({ lessons: { ...get().lessons, ...patch } });
    save(get());
  },
  reset: (preview) => {
    set({ ...(preview ? fresh() : load()), preview, downloadFailed: false });
  },
}));

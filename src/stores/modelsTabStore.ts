import { create } from "zustand";

/** Which tab the Models page shows; other pages can open the language models. */
export type ModelsTab = "speech" | "language";

interface ModelsTabState {
  tab: ModelsTab;
  setTab: (tab: ModelsTab) => void;
}

export const useModelsTab = create<ModelsTabState>((set) => ({
  tab: "speech",
  setTab: (tab) => set({ tab }),
}));

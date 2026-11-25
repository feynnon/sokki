import { create } from "zustand";

/**
 * Which one-time cards and messages are on screen now (the message window,
 * What's New, the milestone card), so the star card never shows together
 * with one of them.
 */
interface CardStore {
  shown: string[];
  setShown: (name: string, shown: boolean) => void;
}

export const useCardStore = create<CardStore>((set) => ({
  shown: [],
  setShown: (name, shown) =>
    set((s) => {
      const has = s.shown.includes(name);
      if (shown === has) return s;
      return {
        shown: shown ? [...s.shown, name] : s.shown.filter((n) => n !== name),
      };
    }),
}));

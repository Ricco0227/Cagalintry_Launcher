import { useMemo } from "react";
import { create } from "zustand";

import type { GameLogLine, InstallProgress, PackView } from "./api";

/**
 * Live state pushed from the backend.
 *
 * Kept in one store outside the component tree because it has to survive
 * navigation: an install started from the Library must still be visible after
 * opening a modpack page, and the game's log has to keep accumulating while
 * you are looking at a different tab.
 */

/** Bounds memory — a modded launch is chatty and can run for hours. */
const MAX_LOG_LINES = 2000;

/**
 * Which modpack the Library's Play button acts on.
 *
 * Persisted so the launcher opens on the pack you were last playing rather than
 * resetting to whichever happens to be newest — the selection is the closest
 * thing this app has to "where you left off".
 */
const SELECTED_PACK_KEY = "cagalintry.selectedPack";

function storedSelection(): string | null {
  try {
    return localStorage.getItem(SELECTED_PACK_KEY);
  } catch {
    // Private mode, a locked-down webview — a forgotten selection is harmless.
    return null;
  }
}

interface LauncherState {
  progress: Record<string, InstallProgress>;
  logs: Record<string, GameLogLine[]>;

  /** May name a pack that no longer exists; callers resolve against the list. */
  selectedPackId: string | null;
  /** Whether the New modpack dialog is open, opened from the rail's +. */
  creating: boolean;

  setProgress: (progress: InstallProgress) => void;
  clearProgress: (packId: string) => void;
  appendLog: (line: GameLogLine) => void;
  clearLogs: (packId: string) => void;
  selectPack: (packId: string) => void;
  setCreating: (creating: boolean) => void;
}

export const useLauncherStore = create<LauncherState>((set) => ({
  progress: {},
  logs: {},
  selectedPackId: storedSelection(),
  creating: false,

  selectPack: (packId) => {
    try {
      localStorage.setItem(SELECTED_PACK_KEY, packId);
    } catch {
      // Not worth failing the click over.
    }
    set({ selectedPackId: packId });
  },

  setCreating: (creating) => set({ creating }),

  setProgress: (progress) =>
    set((state) => ({
      progress: { ...state.progress, [progress.packId]: progress },
    })),

  clearProgress: (packId) =>
    set((state) => {
      const { [packId]: _removed, ...rest } = state.progress;
      return { progress: rest };
    }),

  appendLog: (line) =>
    set((state) => {
      const existing = state.logs[line.packId] ?? [];
      const next = [...existing, line];
      return {
        logs: {
          ...state.logs,
          [line.packId]: next.length > MAX_LOG_LINES ? next.slice(-MAX_LOG_LINES) : next,
        },
      };
    }),

  clearLogs: (packId) =>
    set((state) => ({ logs: { ...state.logs, [packId]: [] } })),
}));

/**
 * The modpack the Library acts on.
 *
 * The stored selection wins while it still exists — deleting the selected pack
 * shouldn't leave the Library pointing at nothing. After that, the one you
 * played most recently, and finally the newest, since `listPacks` returns them
 * newest first.
 */
export function activePack(
  packs: PackView[],
  selectedPackId: string | null,
): PackView | undefined {
  const selected = packs.find((pack) => pack.id === selectedPackId);
  if (selected) return selected;

  const lastPlayed = packs
    .filter((pack) => pack.lastPlayed)
    .sort((a, b) => (a.lastPlayed! < b.lastPlayed! ? 1 : -1))[0];

  return lastPlayed ?? packs[0];
}

/**
 * Installs currently in flight, for the task drawer.
 *
 * The filtering deliberately happens in a memo rather than inside the selector.
 * A selector that builds a new array returns a different reference every time
 * it runs, and `useSyncExternalStore` treats that as "the store changed" — so
 * it re-renders, re-runs the selector, gets another new array, and loops until
 * React aborts with "Maximum update depth exceeded". Selecting the stable
 * `progress` object and deriving from it keeps the snapshot identity stable.
 */
export function useActiveTasks(): InstallProgress[] {
  const progress = useLauncherStore((state) => state.progress);

  return useMemo(
    () =>
      Object.values(progress).filter(
        // A finished install reports every byte accounted for; keep it out of
        // the drawer rather than leaving a permanent 100% row.
        (task) => task.totalBytes === 0 || task.downloadedBytes < task.totalBytes,
      ),
    [progress],
  );
}

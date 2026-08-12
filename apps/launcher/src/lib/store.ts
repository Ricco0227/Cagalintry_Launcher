import { useMemo } from "react";
import { create } from "zustand";

import type { GameLogLine, InstallProgress } from "./api";

/**
 * Live state pushed from the backend.
 *
 * Kept in one store outside the component tree because it has to survive
 * navigation: an install started from the Library must still be visible after
 * opening an instance page, and the game's log has to keep accumulating while
 * you are looking at a different tab.
 */

/** Bounds memory — a modded launch is chatty and can run for hours. */
const MAX_LOG_LINES = 2000;

interface LauncherState {
  progress: Record<string, InstallProgress>;
  logs: Record<string, GameLogLine[]>;

  setProgress: (progress: InstallProgress) => void;
  clearProgress: (instanceId: string) => void;
  appendLog: (line: GameLogLine) => void;
  clearLogs: (instanceId: string) => void;
}

export const useLauncherStore = create<LauncherState>((set) => ({
  progress: {},
  logs: {},

  setProgress: (progress) =>
    set((state) => ({
      progress: { ...state.progress, [progress.instanceId]: progress },
    })),

  clearProgress: (instanceId) =>
    set((state) => {
      const { [instanceId]: _removed, ...rest } = state.progress;
      return { progress: rest };
    }),

  appendLog: (line) =>
    set((state) => {
      const existing = state.logs[line.instanceId] ?? [];
      const next = [...existing, line];
      return {
        logs: {
          ...state.logs,
          [line.instanceId]: next.length > MAX_LOG_LINES ? next.slice(-MAX_LOG_LINES) : next,
        },
      };
    }),

  clearLogs: (instanceId) =>
    set((state) => ({ logs: { ...state.logs, [instanceId]: [] } })),
}));

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

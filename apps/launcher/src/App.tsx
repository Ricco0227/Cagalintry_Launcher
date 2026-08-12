import { useEffect } from "react";
import { Route, Routes } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Rail } from "@/components/Rail";
import { TitleBar } from "@/components/TitleBar";
import { TaskDrawer } from "@/components/TaskDrawer";
import { CreatePackDialog } from "@/components/CreatePackDialog";
import { Library } from "@/routes/Library";
import { PackPage } from "@/routes/Pack";
import { Discover } from "@/routes/Discover";
import { Settings } from "@/routes/Settings";
import { Accounts } from "@/routes/Accounts";
import {
  getSettings,
  listPacks,
  onGameExit,
  onGameLog,
  onInstallProgress,
} from "@/lib/api";
import { useLauncherStore } from "@/lib/store";
import { applyTheme, watchSystemTheme } from "@/lib/theme";

export default function App() {
  useBackendEvents();
  useAppliedTheme();

  // The drawer names the pack a task belongs to, and this query is already
  // warm from the Library.
  const packs = useQuery({ queryKey: ["packs"], queryFn: listPacks });

  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <Rail />
        <main className="flex min-w-0 flex-1 flex-col">
          <Routes>
            <Route path="/" element={<Library />} />
            <Route path="/pack/:id" element={<PackPage />} />
            <Route path="/discover" element={<Discover />} />
            <Route path="/accounts" element={<Accounts />} />
            <Route path="/settings" element={<Settings />} />
          </Routes>
        </main>
      </div>

      <TaskDrawer packs={packs.data ?? []} />

      {/* App-level: the button that opens it is in the rail, which is on every
          page. Its errors have nowhere page-specific to go. */}
      <CreatePackDialog onError={(message) => console.error(message)} />
    </div>
  );
}

/**
 * Subscribes to backend events once, for the lifetime of the app.
 *
 * These are deliberately not owned by a route: an install started from the
 * Library has to keep reporting after you navigate away, and the game's output
 * has to keep accumulating whichever page is open.
 */
function useBackendEvents() {
  const queryClient = useQueryClient();

  useEffect(() => {
    const { setProgress, appendLog, clearProgress } = useLauncherStore.getState();

    const subscriptions = [
      onInstallProgress(setProgress),
      onGameLog(appendLog),
      onGameExit((exit) => {
        clearProgress(exit.packId);
        // The primary button depends on whether the game is running, so both
        // the list and the open pack page need refreshing.
        void queryClient.invalidateQueries({ queryKey: ["packs"] });
        void queryClient.invalidateQueries({ queryKey: ["pack", exit.packId] });
      }),
    ];

    return () => {
      for (const pending of subscriptions) void pending.then((off) => off());
    };
  }, [queryClient]);
}

/** Keeps `data-theme` in step with the saved preference and the OS. */
function useAppliedTheme() {
  const settings = useQuery({ queryKey: ["settings"], queryFn: getSettings });
  const theme = settings.data?.theme;

  useEffect(() => {
    if (!theme) return;
    applyTheme(theme);
    return watchSystemTheme(theme);
  }, [theme]);
}


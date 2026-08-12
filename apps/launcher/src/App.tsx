import { useEffect } from "react";
import { Route, Routes } from "react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Compass, Layers } from "lucide-react";

import { Rail } from "@/components/Rail";
import { TitleBar } from "@/components/TitleBar";
import { TaskDrawer } from "@/components/TaskDrawer";
import { EmptyState, Page } from "@/components/Page";
import { Library } from "@/routes/Library";
import { InstancePage } from "@/routes/Instance";
import { Settings } from "@/routes/Settings";
import { Accounts } from "@/routes/Accounts";
import {
  getSettings,
  listInstances,
  onGameExit,
  onGameLog,
  onInstallProgress,
} from "@/lib/api";
import { useLauncherStore } from "@/lib/store";
import { applyTheme, watchSystemTheme } from "@/lib/theme";

export default function App() {
  useBackendEvents();
  useAppliedTheme();

  // The drawer names the instance a task belongs to, and this query is already
  // warm from the Library.
  const instances = useQuery({ queryKey: ["instances"], queryFn: listInstances });

  return (
    <div className="flex h-full flex-col overflow-hidden bg-bg text-text">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <Rail />
        <main className="flex min-w-0 flex-1 flex-col">
          <Routes>
            <Route path="/" element={<Library />} />
            <Route path="/instance/:id" element={<InstancePage />} />
            <Route path="/packs" element={<Packs />} />
            <Route path="/discover" element={<Discover />} />
            <Route path="/accounts" element={<Accounts />} />
            <Route path="/settings" element={<Settings />} />
          </Routes>
        </main>
      </div>

      <TaskDrawer instances={instances.data ?? []} />
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
        clearProgress(exit.instanceId);
        // The primary button depends on whether the game is running, so both
        // the list and the open instance page need refreshing.
        void queryClient.invalidateQueries({ queryKey: ["instances"] });
        void queryClient.invalidateQueries({ queryKey: ["instance", exit.instanceId] });
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

function Packs() {
  return (
    <Page title="Packs" subtitle="Modpacks shared with your group">
      <EmptyState
        icon={<Layers size={24} />}
        title="Not connected to a sync server"
        description="Packs you and your friends publish will appear here, and an instance bound to one will offer Update whenever somebody changes it."
      />
    </Page>
  );
}

function Discover() {
  return (
    <Page title="Discover" subtitle="Browse Modrinth">
      <EmptyState
        icon={<Compass size={24} />}
        title="Modrinth browsing coming soon"
        description="Search mods, resource packs and shaders, then add them to an instance or straight into a pack."
      />
    </Page>
  );
}

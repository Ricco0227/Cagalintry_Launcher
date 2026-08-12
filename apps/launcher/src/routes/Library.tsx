import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { Boxes, Info, Plus, SlidersHorizontal, TriangleAlert } from "lucide-react";

import {
  errorMessage,
  formatBytes,
  isCommandError,
  launchPack,
  listPacks,
  onGameExit,
  type PackView,
} from "@/lib/api";
import { activePack, useLauncherStore } from "@/lib/store";
import { cn } from "@/lib/cn";
import { Button } from "@/components/Button";
import { EmptyState, Page } from "@/components/Page";
import { PrimaryButton } from "@/components/PrimaryButton";

/** A banner message. Not every interruption is a failure. */
interface Notice {
  tone: "error" | "info";
  message: string;
}

/**
 * The launcher's home screen: one modpack, one button.
 *
 * The modpacks themselves live in the rail, so this page carries no list at
 * all — just the pack you have selected and the single action that applies to
 * it right now, sat at the bottom where a launcher's play button belongs.
 */
export function Library() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [notice, setNotice] = useState<Notice | null>(null);

  const setError = (message: string) => setNotice({ tone: "error", message });

  const packs = useQuery({ queryKey: ["packs"], queryFn: listPacks });
  const selectedPackId = useLauncherStore((state) => state.selectedPackId);
  const setCreating = useLauncherStore((state) => state.setCreating);
  const progress = useLauncherStore((state) => state.progress);

  // A crash is worth surfacing here; the app-level listener handles refreshing.
  useEffect(() => {
    const subscription = onGameExit((exit) => {
      if (exit.crashed) {
        setError(
          `The game exited unexpectedly${exit.code === null ? "" : ` with code ${exit.code}`}. Its output is on the modpack's Logs tab.`,
        );
      }
    });
    return () => void subscription.then((off) => off());
  }, []);

  const launch = useMutation({
    mutationFn: launchPack,
    onMutate: () => setNotice(null),
    onSettled: () => void queryClient.invalidateQueries({ queryKey: ["packs"] }),
    onError: (err) => {
      if (!isCommandError(err)) {
        setError(errorMessage(err));
        return;
      }
      // An impatient second click is not something to apologise for — the
      // guard did its job and the first launch is still running.
      if (err.code === "busy" || err.code === "running") return;
      // Not having signed in yet is a next step, not a fault.
      setNotice({ tone: err.code === "noAccount" ? "info" : "error", message: err.message });
    },
  });

  /**
   * Signing in is a different action from launching, so it is dispatched here
   * rather than sent to the backend only to come back as an error.
   */
  const handlePrimary = (pack: PackView) => {
    if (pack.action.kind === "linkMinecraft") {
      setNotice({
        tone: "info",
        message:
          "Microsoft sign-in lands in the next phase. Once your Azure application is approved for the Minecraft API, you'll link an account here and this becomes Play.",
      });
      return;
    }
    launch.mutate(pack.id);
  };

  const list = packs.data ?? [];
  const pack = activePack(list, selectedPackId);

  if (packs.isPending) return <Page title="Library" subtitle="" children={null} />;

  if (!pack) {
    return (
      <Page title="Library" subtitle="No modpacks yet">
        <EmptyState
          icon={<Boxes size={24} />}
          title="No modpacks yet"
          description="Create one to download Minecraft and play, then add mods to it. Libraries, assets and Java are shared between modpacks, so a second one on the same version is nearly instant."
          action={
            <Button variant="primary" icon={<Plus size={15} />} onClick={() => setCreating(true)}>
              New modpack
            </Button>
          }
        />
      </Page>
    );
  }

  const task = progress[pack.id];

  return (
    <div className="flex min-h-0 flex-1 flex-col px-8 py-6">
      {notice && (
        <div
          className={cn(
            "flex items-start gap-2.5 rounded-[12px] border px-3.5 py-3 text-[13px]",
            notice.tone === "error"
              ? "border-danger/35 bg-danger/10"
              : "border-accent/35 bg-accent-soft",
          )}
        >
          {notice.tone === "error" ? (
            <TriangleAlert size={16} className="mt-px shrink-0 text-danger" />
          ) : (
            <Info size={16} className="mt-px shrink-0 text-accent" />
          )}
          <p data-selectable className="min-w-0 flex-1 leading-relaxed">
            {notice.message}
          </p>
          <button
            type="button"
            onClick={() => setNotice(null)}
            className="shrink-0 text-text-subtle hover:text-text"
          >
            Dismiss
          </button>
        </div>
      )}

      {/* The action sits at the bottom, centred, with the pack it belongs to
          named directly above it — nothing else competes for the space. */}
      <div className="flex min-h-0 flex-1 flex-col items-center justify-end gap-5 pb-4">
        <div className="flex flex-col items-center gap-1.5">
          <h1 className="text-center text-[28px] leading-tight font-semibold">{pack.name}</h1>
          <div className="flex items-center gap-1.5 text-[12px]">
            <Chip>{pack.mcVersion}</Chip>
            {pack.loader.kind !== "vanilla" && (
              <Chip className="capitalize">{pack.loader.kind}</Chip>
            )}
          </div>
        </div>

        {/* Edit hangs off to the left rather than sharing a centred row, so the
            play button stays exactly centred on the screen no matter how wide
            the secondary control is. */}
        <div className="relative flex items-center">
          <Button
            size="lg"
            icon={<SlidersHorizontal size={16} />}
            title={`Edit ${pack.name}`}
            aria-label={`Edit ${pack.name}`}
            onClick={() => void navigate(`/pack/${pack.id}`)}
            className="absolute right-full mr-2.5"
          >
            Edit
          </Button>

          <PrimaryButton action={pack.action} size="lg" onClick={() => handlePrimary(pack)} />
        </div>

        {/* Reserved whether or not work is running, so the button doesn't jump
            a line up the moment an install finishes. */}
        <p className="h-4 text-[12px] text-text-subtle tabular-nums">
          {task &&
            `${task.stage}${
              task.totalBytes > 0
                ? ` · ${formatBytes(task.downloadedBytes)} of ${formatBytes(task.totalBytes)}`
                : ""
            }`}
        </p>
      </div>
    </div>
  );
}

function Chip({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <span className={cn("rounded-md bg-surface-2 px-1.5 py-0.5 font-medium", className)}>
      {children}
    </span>
  );
}

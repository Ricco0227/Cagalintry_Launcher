import { Trash2 } from "lucide-react";

import { formatBytes, type InstallProgress, type InstanceView } from "@/lib/api";
import { cn } from "@/lib/cn";
import { PrimaryButton } from "./PrimaryButton";

/**
 * Deterministic cover art from the instance id.
 *
 * Real artwork arrives with pack icons; until then a stable hue per instance
 * still gives the grid the artwork-led feel, and the same instance always looks
 * the same rather than shuffling on every render.
 */
function coverStyle(id: string): React.CSSProperties {
  let hash = 0;
  for (let i = 0; i < id.length; i += 1) {
    hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  }
  const hue = hash % 360;
  return {
    backgroundImage: `linear-gradient(140deg,
      oklch(0.62 0.17 ${hue}) 0%,
      oklch(0.48 0.15 ${(hue + 40) % 360}) 55%,
      oklch(0.33 0.10 ${(hue + 75) % 360}) 100%)`,
  };
}

export function InstanceCard({
  instance,
  progress,
  onPrimary,
  onDelete,
}: {
  instance: InstanceView;
  progress?: InstallProgress | undefined;
  onPrimary: () => void;
  onDelete: () => void;
}) {
  const busy = instance.action.kind === "busy";
  const percent =
    progress && progress.totalBytes > 0
      ? Math.min(100, (progress.downloadedBytes / progress.totalBytes) * 100)
      : null;

  return (
    <article className="group relative flex flex-col overflow-hidden rounded-[14px] border border-border bg-surface shadow-[var(--shadow-card)] transition-colors hover:border-border-strong">
      <div className="relative h-32 shrink-0" style={coverStyle(instance.id)}>
        <div className="absolute inset-0 bg-gradient-to-t from-surface via-surface/35 to-transparent" />

        <button
          type="button"
          onClick={onDelete}
          aria-label={`Delete ${instance.name}`}
          title="Delete instance"
          // Hidden until hover so the grid stays calm, but still reachable by
          // keyboard.
          className="absolute top-2 right-2 grid size-8 place-items-center rounded-lg bg-black/35 text-white/80 opacity-0 backdrop-blur-sm transition-all hover:bg-danger hover:text-white group-hover:opacity-100 focus-visible:opacity-100"
        >
          <Trash2 size={15} />
        </button>
      </div>

      <div className="flex min-w-0 flex-1 flex-col gap-3 p-4 pt-2">
        <div className="min-w-0">
          <h3 className="truncate text-[15px] font-semibold" title={instance.name}>
            {instance.name}
          </h3>
          <div className="mt-1 flex items-center gap-1.5 text-[12px] text-text-muted">
            <span className="rounded-md bg-surface-2 px-1.5 py-0.5 font-medium">
              {instance.mcVersion}
            </span>
            {instance.loader.kind !== "vanilla" && (
              <span className="rounded-md bg-surface-2 px-1.5 py-0.5 font-medium capitalize">
                {instance.loader.kind}
              </span>
            )}
          </div>
        </div>

        {busy && progress ? (
          <div className="flex flex-col gap-1.5">
            <div className="h-1.5 overflow-hidden rounded-full bg-surface-3">
              <div
                className={cn(
                  "h-full rounded-full bg-accent transition-[width] duration-200",
                  // No known total yet: show motion rather than a stuck 0%.
                  percent === null && "w-1/3 animate-pulse",
                )}
                style={percent === null ? undefined : { width: `${percent}%` }}
              />
            </div>
            <p className="truncate text-[11.5px] text-text-subtle tabular-nums">
              {progress.stage}
              {progress.totalBytes > 0 &&
                ` — ${formatBytes(progress.downloadedBytes)} of ${formatBytes(progress.totalBytes)}`}
            </p>
          </div>
        ) : (
          <PrimaryButton size="sm" action={instance.action} onClick={onPrimary} className="w-full" />
        )}
      </div>
    </article>
  );
}

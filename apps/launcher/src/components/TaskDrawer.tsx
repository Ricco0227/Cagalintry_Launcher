import { useState } from "react";
import { ChevronDown, Loader2 } from "lucide-react";

import { formatBytes, type PackView } from "@/lib/api";
import { useActiveTasks } from "@/lib/store";
import { cn } from "@/lib/cn";

/**
 * Background work, bottom-right.
 *
 * Downloads happen whether or not you are looking at the page that started
 * them, so progress belongs in fixed window chrome rather than inside a route.
 * The drawer hides itself entirely when nothing is running.
 */
export function TaskDrawer({ packs }: { packs: PackView[] }) {
  const tasks = useActiveTasks();
  const [collapsed, setCollapsed] = useState(false);

  if (tasks.length === 0) return null;

  const nameFor = (packId: string) =>
    packs.find((pack) => pack.id === packId)?.name ?? "Pack";

  return (
    <div className="pointer-events-none fixed right-5 bottom-5 z-40 w-[320px]">
      <div className="pointer-events-auto overflow-hidden rounded-[14px] border border-border bg-bg-elevated shadow-[var(--shadow-pop)]">
        <button
          type="button"
          onClick={() => setCollapsed((value) => !value)}
          className="flex w-full items-center gap-2.5 px-4 py-3 text-left transition-colors hover:bg-surface-2"
        >
          <Loader2 size={15} className="shrink-0 animate-spin text-accent" />
          <span className="flex-1 text-[13px] font-medium">
            {tasks.length === 1 ? "1 task running" : `${tasks.length} tasks running`}
          </span>
          <ChevronDown
            size={15}
            className={cn(
              "shrink-0 text-text-subtle transition-transform duration-200",
              collapsed && "-rotate-90",
            )}
          />
        </button>

        {!collapsed && (
          <div className="flex max-h-[280px] flex-col gap-3 overflow-y-auto border-t border-border px-4 py-3">
            {tasks.map((task) => {
              const percent =
                task.totalBytes > 0
                  ? Math.min(100, (task.downloadedBytes / task.totalBytes) * 100)
                  : null;

              return (
                <div key={task.packId} className="flex flex-col gap-1.5">
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="truncate text-[12.5px] font-medium">
                      {nameFor(task.packId)}
                    </span>
                    <span className="shrink-0 text-[11.5px] text-text-subtle tabular-nums">
                      {percent === null ? task.stage : `${Math.round(percent)}%`}
                    </span>
                  </div>

                  <div className="h-1.5 overflow-hidden rounded-full bg-surface-3">
                    <div
                      className={cn(
                        "h-full rounded-full bg-accent transition-[width] duration-200",
                        percent === null && "w-1/3 animate-pulse",
                      )}
                      style={percent === null ? undefined : { width: `${percent}%` }}
                    />
                  </div>

                  <p className="truncate text-[11.5px] text-text-subtle tabular-nums">
                    {task.stage}
                    {task.totalBytes > 0 &&
                      ` — ${formatBytes(task.downloadedBytes)} of ${formatBytes(task.totalBytes)}`}
                  </p>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

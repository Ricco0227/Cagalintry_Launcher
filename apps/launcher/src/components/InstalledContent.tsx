import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Package, Trash2 } from "lucide-react";

import {
  errorMessage,
  listContent,
  removeContent,
  setContentEnabled,
  type EntryKind,
  type PackEntry,
} from "@/lib/api";
import { cn } from "@/lib/cn";
import { EmptyState } from "./Page";

const GROUPS: { kind: EntryKind; title: string }[] = [
  { kind: "mod", title: "Mods" },
  { kind: "resourcepack", title: "Resource packs" },
  { kind: "shaderpack", title: "Shaders" },
];

/** What is installed in an instance, with per-item enable and remove. */
export function InstalledContent({
  instanceId,
  onError,
  onBrowse,
}: {
  instanceId: string;
  onError: (message: string) => void;
  onBrowse: () => void;
}) {
  const queryClient = useQueryClient();
  const content = useQuery({
    queryKey: ["content", instanceId],
    queryFn: () => listContent(instanceId),
  });

  const update = (entries: PackEntry[]) =>
    queryClient.setQueryData(["content", instanceId], entries);

  const toggle = useMutation({
    mutationFn: ({ path, enabled }: { path: string; enabled: boolean }) =>
      setContentEnabled(instanceId, path, enabled),
    onSuccess: update,
    onError: (err) => onError(errorMessage(err)),
  });

  const remove = useMutation({
    mutationFn: (path: string) => removeContent(instanceId, path),
    onSuccess: update,
    onError: (err) => onError(errorMessage(err)),
  });

  const entries = content.data ?? [];

  if (content.isPending) return null;

  if (entries.length === 0) {
    return (
      <EmptyState
        icon={<Package size={24} />}
        title="No content installed"
        description="Add mods, resource packs and shaders from Modrinth. Anything you install here can later be published as a pack for your friends."
        action={
          <button
            type="button"
            onClick={onBrowse}
            className="inline-flex h-9 items-center rounded-[10px] bg-accent px-4 text-[13px] font-medium text-accent-fg transition-colors hover:bg-accent-hover"
          >
            Browse Modrinth
          </button>
        }
      />
    );
  }

  return (
    <div className="flex max-w-[720px] flex-col gap-6">
      {GROUPS.map(({ kind, title }) => {
        const group = entries.filter((entry) => entry.kind === kind);
        if (group.length === 0) return null;

        return (
          <section key={kind} className="flex flex-col gap-2">
            <h2 className="text-[13px] font-semibold text-text-muted">
              {title}
              <span className="ml-1.5 text-text-subtle tabular-nums">{group.length}</span>
            </h2>

            <div className="flex flex-col gap-1.5">
              {group.map((entry) => (
                <div
                  key={entry.path}
                  className={cn(
                    "flex items-center gap-3 rounded-[11px] border border-border bg-surface px-3 py-2.5",
                    !entry.enabled && "opacity-55",
                  )}
                >
                  <input
                    type="checkbox"
                    checked={entry.enabled}
                    onChange={(event) =>
                      toggle.mutate({ path: entry.path, enabled: event.target.checked })
                    }
                    aria-label={`Enable ${entry.name ?? entry.path}`}
                    className="size-4 shrink-0 accent-[var(--accent)]"
                  />

                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[13px] font-medium">
                      {entry.name ?? entry.path.split("/").pop()}
                    </p>
                    <p className="truncate text-[11.5px] text-text-subtle">
                      {entry.versionNumber ?? entry.path}
                      {entry.side === "client" && " · client only"}
                    </p>
                  </div>

                  <button
                    type="button"
                    onClick={() => remove.mutate(entry.path)}
                    aria-label={`Remove ${entry.name ?? entry.path}`}
                    title="Remove"
                    className="grid size-8 shrink-0 place-items-center rounded-lg text-text-subtle transition-colors hover:bg-danger hover:text-white"
                  >
                    <Trash2 size={15} />
                  </button>
                </div>
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );
}

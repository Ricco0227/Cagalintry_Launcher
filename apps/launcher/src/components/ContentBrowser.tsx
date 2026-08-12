import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Download, Loader2, Package, Search } from "lucide-react";

import {
  errorMessage,
  installContent,
  listContent,
  searchContent,
  type EntryKind,
  type InstanceView,
  type SearchHit,
} from "@/lib/api";
import { cn } from "@/lib/cn";
import { EmptyState } from "./Page";

const KINDS: { id: EntryKind; label: string }[] = [
  { id: "mod", label: "Mods" },
  { id: "resourcepack", label: "Resource packs" },
  { id: "shaderpack", label: "Shaders" },
];

/**
 * Browse Modrinth and install into an instance.
 *
 * Scoped to the instance on purpose: results are filtered to that Minecraft
 * version and loader, so what you see is what will actually work rather than
 * everything Modrinth has.
 */
export function ContentBrowser({
  instance,
  onError,
}: {
  instance: InstanceView;
  onError: (message: string) => void;
}) {
  const queryClient = useQueryClient();
  const [kind, setKind] = useState<EntryKind>("mod");
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");

  // Debounced so typing doesn't fire a request per keystroke and burn through
  // the rate limit.
  useEffect(() => {
    const timer = setTimeout(() => setQuery(text), 350);
    return () => clearTimeout(timer);
  }, [text]);

  const results = useQuery({
    queryKey: ["modrinth", kind, query, instance.mcVersion, instance.loader.kind],
    queryFn: () =>
      searchContent({
        kind,
        query,
        mcVersion: instance.mcVersion,
        loader: instance.loader.kind,
        limit: 30,
      }),
    staleTime: 5 * 60 * 1000,
  });

  const installed = useQuery({
    queryKey: ["content", instance.id],
    queryFn: () => listContent(instance.id),
  });

  const install = useMutation({
    mutationFn: (hit: SearchHit) => installContent(instance.id, hit.projectId, kind),
    onSuccess: (entries) => queryClient.setQueryData(["content", instance.id], entries),
    onError: (err) => onError(errorMessage(err)),
  });

  const installedProjects = new Set(
    (installed.data ?? []).map((entry) => entry.source.projectId),
  );

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex gap-1">
          {KINDS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              onClick={() => setKind(id)}
              className={cn(
                "rounded-[9px] px-3 py-1.5 text-[12.5px] font-medium transition-colors",
                kind === id
                  ? "bg-accent-soft text-accent"
                  : "text-text-muted hover:bg-surface-2 hover:text-text",
              )}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="relative min-w-[220px] flex-1">
          <Search
            size={15}
            className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-text-subtle"
          />
          <input
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder={`Search ${KINDS.find((k) => k.id === kind)?.label.toLowerCase()}…`}
            className="h-9 w-full rounded-[10px] border border-border bg-bg pr-3 pl-9 text-[13px] outline-none placeholder:text-text-subtle focus:border-accent"
          />
        </div>
      </div>

      <p className="text-[12px] text-text-subtle">
        Showing content compatible with {instance.mcVersion}
        {instance.loader.kind !== "vanilla" && kind === "mod" && ` on ${instance.loader.kind}`}.
      </p>

      {results.isPending ? (
        <div className="flex items-center gap-2 py-10 text-[13px] text-text-subtle">
          <Loader2 size={15} className="animate-spin" />
          Searching Modrinth…
        </div>
      ) : results.isError ? (
        <p className="py-6 text-[13px] text-danger">{errorMessage(results.error)}</p>
      ) : results.data.hits.length === 0 ? (
        <EmptyState
          icon={<Package size={24} />}
          title="Nothing found"
          description={`No ${KINDS.find((k) => k.id === kind)?.label.toLowerCase()} match that search for this Minecraft version and loader.`}
        />
      ) : (
        <div className="flex flex-col gap-2">
          {results.data.hits.map((hit) => (
            <ResultRow
              key={hit.projectId}
              hit={hit}
              installed={installedProjects.has(hit.projectId)}
              installing={install.isPending && install.variables?.projectId === hit.projectId}
              onInstall={() => install.mutate(hit)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ResultRow({
  hit,
  installed,
  installing,
  onInstall,
}: {
  hit: SearchHit;
  installed: boolean;
  installing: boolean;
  onInstall: () => void;
}) {
  return (
    <div className="flex items-center gap-3 rounded-[12px] border border-border bg-surface p-3 transition-colors hover:border-border-strong">
      {hit.iconUrl ? (
        <img
          src={hit.iconUrl}
          alt=""
          loading="lazy"
          className="size-11 shrink-0 rounded-[9px] bg-surface-2 object-cover"
        />
      ) : (
        <div className="grid size-11 shrink-0 place-items-center rounded-[9px] bg-surface-2 text-text-subtle">
          <Package size={18} />
        </div>
      )}

      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <h3 className="truncate text-[13.5px] font-semibold">{hit.title}</h3>
          {hit.author && (
            <span className="shrink-0 text-[11.5px] text-text-subtle">by {hit.author}</span>
          )}
        </div>
        <p className="mt-0.5 line-clamp-2 text-[12.5px] leading-relaxed text-text-muted">
          {hit.description}
        </p>
        <p className="mt-1 text-[11.5px] text-text-subtle tabular-nums">
          {compactNumber(hit.downloads)} downloads
        </p>
      </div>

      <button
        type="button"
        onClick={onInstall}
        disabled={installed || installing}
        className={cn(
          "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-[9px] px-3 text-[12.5px] font-medium transition-colors",
          installed
            ? "cursor-default bg-surface-2 text-success"
            : "bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-60",
        )}
      >
        {installed ? (
          <>
            <Check size={14} /> Installed
          </>
        ) : installing ? (
          <>
            <Loader2 size={14} className="animate-spin" /> Installing
          </>
        ) : (
          <>
            <Download size={14} /> Install
          </>
        )}
      </button>
    </div>
  );
}

function compactNumber(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(0)}K`;
  return String(value);
}

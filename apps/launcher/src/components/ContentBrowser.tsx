import { useEffect, useState } from "react";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  ArrowDownWideNarrow,
  Check,
  ChevronLeft,
  ChevronRight,
  Download,
  Loader2,
  Package,
  Search,
} from "lucide-react";

import {
  errorMessage,
  installContent,
  listContent,
  searchContent,
  type EntryKind,
  type PackView,
  type SearchHit,
  type SearchSort,
} from "@/lib/api";
import { cn } from "@/lib/cn";
import { EmptyState } from "./Page";
import { ProjectView } from "./ProjectView";
import { Select, type SelectOption } from "./Select";

const KINDS: { id: EntryKind; label: string }[] = [
  { id: "mod", label: "Mods" },
  { id: "resourcepack", label: "Resource packs" },
  { id: "shaderpack", label: "Shaders" },
];

/** Results per page. Modrinth's search takes an offset, so paging is server-side. */
const PAGE_SIZE = 30;

const SORTS: SelectOption<SearchSort>[] = [
  { value: "relevance", label: "Relevance" },
  { value: "downloads", label: "Downloads" },
  { value: "follows", label: "Followers" },
  { value: "newest", label: "Newest" },
  { value: "updated", label: "Recently updated" },
];

/**
 * Browse Modrinth, and — given a modpack — install into it.
 *
 * With a `pack`, results are filtered to that Minecraft version and loader, so
 * everything offered is something that will actually run in it. Without one
 * this is the Discover view: all of Modrinth, read-only, with no install
 * control anywhere. Installing is always an action taken inside a modpack.
 */
export function ContentBrowser({
  pack,
  onError,
}: {
  pack: PackView | null;
  onError: (message: string) => void;
}) {
  const queryClient = useQueryClient();
  const [kind, setKind] = useState<EntryKind>("mod");
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  const [openProject, setOpenProject] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [sort, setSort] = useState<SearchSort>("relevance");
  const packId = pack?.id ?? null;

  // Debounced so typing doesn't fire a request per keystroke and burn through
  // the rate limit.
  useEffect(() => {
    const timer = setTimeout(() => setQuery(text), 350);
    return () => clearTimeout(timer);
  }, [text]);

  // A new search, sort or content kind is a different result set, so page 7 of
  // the old one is meaningless — and would show an empty list if the new set is
  // shorter.
  useEffect(() => {
    setPage(0);
  }, [query, kind, sort]);

  const results = useQuery({
    queryKey: [
      "modrinth",
      kind,
      query,
      sort,
      page,
      pack?.mcVersion ?? null,
      pack?.loader.kind ?? null,
    ],
    queryFn: () =>
      searchContent({
        kind,
        query,
        sort,
        limit: PAGE_SIZE,
        offset: page * PAGE_SIZE,
        // Omitted entirely when browsing: an unscoped search is the point of
        // Discover, and the filters are absent rather than undefined.
        ...(pack ? { mcVersion: pack.mcVersion, loader: pack.loader.kind } : {}),
      }),
    staleTime: 5 * 60 * 1000,
    // Keeps the current page on screen while the next one loads, so paging
    // doesn't flash the whole list back to a spinner.
    placeholderData: keepPreviousData,
  });

  const installed = useQuery({
    queryKey: ["content", packId],
    queryFn: () => listContent(packId as string),
    enabled: packId !== null,
  });

  const install = useMutation({
    mutationFn: (hit: SearchHit) => installContent(packId as string, hit.projectId, kind),
    onSuccess: (entries) => queryClient.setQueryData(["content", packId], entries),
    onError: (err) => onError(errorMessage(err)),
  });

  const installedProjects = new Set(
    (installed.data ?? []).map((entry) => entry.source.projectId),
  );

  // Opening a project replaces the results rather than layering over them, so
  // the search state is preserved and Back returns to exactly where you were.
  if (openProject) {
    return (
      <ProjectView
        projectId={openProject}
        kind={kind}
        pack={pack}
        onBack={() => setOpenProject(null)}
        onError={onError}
      />
    );
  }

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

        <div className="flex shrink-0 items-center gap-1.5 text-text-subtle">
          <ArrowDownWideNarrow size={15} />
          <Select
            value={sort}
            options={SORTS}
            onChange={setSort}
            ariaLabel="Sort results"
            className="w-[170px]"
          />
        </div>
      </div>

      <p className="text-[12px] text-text-subtle">
        {pack ? (
          <>
            Showing content compatible with {pack.mcVersion}
            {pack.loader.kind !== "vanilla" && kind === "mod" && ` on ${pack.loader.kind}`}.
          </>
        ) : (
          "Browsing all of Modrinth. Open a modpack to add any of this to it."
        )}
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
          description={
            pack
              ? `No ${KINDS.find((k) => k.id === kind)?.label.toLowerCase()} match that search for this Minecraft version and loader.`
              : `No ${KINDS.find((k) => k.id === kind)?.label.toLowerCase()} match that search.`
          }
        />
      ) : (
        <>
          <div className={cn("flex flex-col gap-2", results.isFetching && "opacity-60")}>
            {results.data.hits.map((hit) => (
              <ResultRow
                key={hit.projectId}
                hit={hit}
                installed={installedProjects.has(hit.projectId)}
                installing={install.isPending && install.variables?.projectId === hit.projectId}
                // No pack, no install button — Discover is for reading.
                onInstall={pack ? () => install.mutate(hit) : null}
                onOpen={() => setOpenProject(hit.projectId)}
              />
            ))}
          </div>

          <Pager
            page={page}
            pageCount={Math.ceil(results.data.totalHits / PAGE_SIZE)}
            total={results.data.totalHits}
            shown={results.data.hits.length}
            busy={results.isFetching}
            onChange={setPage}
          />
        </>
      )}
    </div>
  );
}

/**
 * Previous/next paging over the search results.
 *
 * Modrinth reports the full match count, so the range and the last page are
 * both known up front — no "load more" that leaves you unable to go back.
 */
function Pager({
  page,
  pageCount,
  total,
  shown,
  busy,
  onChange,
}: {
  page: number;
  pageCount: number;
  total: number;
  shown: number;
  busy: boolean;
  onChange: (page: number) => void;
}) {
  // A single page of results needs no controls at all.
  if (pageCount <= 1) return null;

  const first = page * PAGE_SIZE + 1;
  const last = page * PAGE_SIZE + shown;

  return (
    <div className="flex items-center justify-between gap-3 pt-1">
      <p className="text-[12px] text-text-subtle tabular-nums">
        {first}–{last} of {total.toLocaleString()}
      </p>

      <div className="flex items-center gap-1.5">
        <PagerButton
          disabled={page === 0 || busy}
          onClick={() => onChange(page - 1)}
          label="Previous page"
        >
          <ChevronLeft size={15} />
          Previous
        </PagerButton>

        <span className="px-1 text-[12px] text-text-muted tabular-nums">
          {page + 1} / {pageCount}
        </span>

        <PagerButton
          disabled={page >= pageCount - 1 || busy}
          onClick={() => onChange(page + 1)}
          label="Next page"
        >
          Next
          <ChevronRight size={15} />
        </PagerButton>
      </div>
    </div>
  );
}

function PagerButton({
  disabled,
  onClick,
  label,
  children,
}: {
  disabled: boolean;
  onClick: () => void;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      className="inline-flex h-8 items-center gap-1 rounded-[9px] border border-border bg-surface px-2.5 text-[12.5px] font-medium text-text-muted transition-colors hover:border-border-strong hover:text-text disabled:pointer-events-none disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function ResultRow({
  hit,
  installed,
  installing,
  onInstall,
  onOpen,
}: {
  hit: SearchHit;
  installed: boolean;
  installing: boolean;
  /** `null` in browse-only mode, where the row carries no install control. */
  onInstall: (() => void) | null;
  onOpen: () => void;
}) {
  return (
    <div className="flex items-center gap-3 rounded-[12px] border border-border bg-surface p-3 transition-colors hover:border-border-strong">
      {/* The icon and text open the project; only the button installs, so a
          click to read about something never installs it by accident. */}
      <button
        type="button"
        onClick={onOpen}
        aria-label={`Open ${hit.title}`}
        className="shrink-0 cursor-pointer"
      >
        {hit.iconUrl ? (
          <img
            src={hit.iconUrl}
            alt=""
            loading="lazy"
            className="size-11 rounded-[9px] bg-surface-2 object-cover"
          />
        ) : (
          <div className="grid size-11 place-items-center rounded-[9px] bg-surface-2 text-text-subtle">
            <Package size={18} />
          </div>
        )}
      </button>

      <button
        type="button"
        onClick={onOpen}
        className="min-w-0 flex-1 cursor-pointer text-left"
      >
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
      </button>

      {onInstall && (
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
      )}
    </div>
  );
}

function compactNumber(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(0)}K`;
  return String(value);
}

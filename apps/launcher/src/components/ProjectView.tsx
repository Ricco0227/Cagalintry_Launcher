import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  Bug,
  Check,
  Download,
  ExternalLink,
  Heart,
  Loader2,
  Code2,
  Package,
} from "lucide-react";

import {
  errorMessage,
  getProject,
  installContent,
  listContent,
  listProjectVersions,
  type EntryKind,
  type PackView,
} from "@/lib/api";
import { cn } from "@/lib/cn";
import { Button } from "./Button";

/**
 * A project's full page: description, gallery, links and versions.
 *
 * With a `pack` it can install, and its versions are narrowed to that modpack.
 * Without one — reached from Discover — it is strictly reading material: every
 * version is listed and nothing can be installed, because there is no modpack
 * for it to go into.
 *
 * The description arrives as HTML already sanitised in Rust, which is why it can
 * be inserted directly — the untrusted Markdown never reaches the webview.
 */
export function ProjectView({
  projectId,
  kind,
  pack,
  onBack,
  onError,
}: {
  projectId: string;
  kind: EntryKind;
  pack: PackView | null;
  onBack: () => void;
  onError: (message: string) => void;
}) {
  const queryClient = useQueryClient();
  const [showAllVersions, setShowAllVersions] = useState(false);
  const packId = pack?.id ?? null;

  const project = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => getProject(projectId),
    staleTime: 10 * 60 * 1000,
  });

  const versions = useQuery({
    queryKey: ["project-versions", projectId, packId, kind],
    queryFn: () => listProjectVersions(packId, projectId, kind),
    staleTime: 5 * 60 * 1000,
  });

  const installed = useQuery({
    queryKey: ["content", packId],
    queryFn: () => listContent(packId as string),
    enabled: packId !== null,
  });

  const install = useMutation({
    mutationFn: (versionId?: string) =>
      installContent(packId as string, projectId, kind, versionId),
    onSuccess: (entries) => queryClient.setQueryData(["content", packId], entries),
    onError: (err) => onError(errorMessage(err)),
  });

  const installedEntry = (installed.data ?? []).find(
    (entry) => entry.source.projectId === projectId,
  );

  if (project.isPending) {
    return (
      <div className="flex items-center gap-2 py-12 text-[13px] text-text-subtle">
        <Loader2 size={15} className="animate-spin" />
        Loading…
      </div>
    );
  }

  if (project.isError) {
    return (
      <div className="flex flex-col items-start gap-3 py-8">
        <p className="text-[13px] text-danger">{errorMessage(project.error)}</p>
        <Button size="sm" icon={<ArrowLeft size={14} />} onClick={onBack}>
          Back
        </Button>
      </div>
    );
  }

  const data = project.data;
  const visibleVersions = showAllVersions ? versions.data : versions.data?.slice(0, 5);

  return (
    <div className="flex flex-col gap-5">
      <Button size="sm" icon={<ArrowLeft size={14} />} onClick={onBack} className="w-fit">
        Back to results
      </Button>

      <header className="flex items-start gap-4">
        {data.iconUrl ? (
          <img
            src={data.iconUrl}
            alt=""
            className="size-16 shrink-0 rounded-[12px] bg-surface-2 object-cover"
          />
        ) : (
          <div className="grid size-16 shrink-0 place-items-center rounded-[12px] bg-surface-2 text-text-subtle">
            <Package size={26} />
          </div>
        )}

        <div className="min-w-0 flex-1">
          <h1 className="text-[20px] leading-tight font-semibold">{data.title}</h1>
          <p className="mt-1 text-[13px] leading-relaxed text-text-muted">{data.description}</p>

          <div className="mt-2 flex flex-wrap items-center gap-3 text-[12px] text-text-subtle">
            <span className="inline-flex items-center gap-1 tabular-nums">
              <Download size={13} />
              {compactNumber(data.downloads)}
            </span>
            <span className="inline-flex items-center gap-1 tabular-nums">
              <Heart size={13} />
              {compactNumber(data.followers)}
            </span>
            {data.clientSide === "required" && data.serverSide === "unsupported" && (
              <span className="rounded-md bg-surface-2 px-1.5 py-0.5">Client only</span>
            )}
          </div>
        </div>

        {pack && (
          <div className="flex shrink-0 flex-col items-end gap-2">
            <button
              type="button"
              onClick={() => install.mutate(undefined)}
              disabled={install.isPending || installedEntry !== undefined}
              className={cn(
                "inline-flex h-9 items-center gap-2 rounded-[10px] px-4 text-[13px] font-semibold transition-colors",
                installedEntry
                  ? "cursor-default bg-surface-2 text-success"
                  : "bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-60",
              )}
            >
              {installedEntry ? (
                <>
                  <Check size={15} /> Installed
                </>
              ) : install.isPending ? (
                <>
                  <Loader2 size={15} className="animate-spin" /> Installing
                </>
              ) : (
                <>
                  <Download size={15} /> Install
                </>
              )}
            </button>
            {installedEntry?.versionNumber && (
              <span className="text-[11.5px] text-text-subtle">{installedEntry.versionNumber}</span>
            )}
          </div>
        )}
      </header>

      <div className="flex flex-wrap gap-2">
        <LinkButton url={`https://modrinth.com/project/${data.slug}`} icon={<ExternalLink size={13} />}>
          Modrinth
        </LinkButton>
        {data.sourceUrl && (
          <LinkButton url={data.sourceUrl} icon={<Code2 size={13} />}>
            Source
          </LinkButton>
        )}
        {data.issuesUrl && (
          <LinkButton url={data.issuesUrl} icon={<Bug size={13} />}>
            Issues
          </LinkButton>
        )}
      </div>

      {data.categories.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {data.categories.map((category) => (
            <span
              key={category}
              className="rounded-md bg-surface-2 px-2 py-0.5 text-[11.5px] text-text-muted capitalize"
            >
              {category}
            </span>
          ))}
        </div>
      )}

      <section className="flex flex-col gap-2">
        <h2 className="text-[13px] font-semibold text-text-muted">Versions</h2>

        {versions.isPending ? (
          <p className="text-[12.5px] text-text-subtle">Loading versions…</p>
        ) : versions.data && versions.data.length > 0 ? (
          <>
            <div className="flex flex-col gap-1.5">
              {visibleVersions?.map((version) => (
                <div
                  key={version.id}
                  className="flex items-center gap-3 rounded-[10px] border border-border bg-surface px-3 py-2"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[12.5px] font-medium">{version.versionNumber}</p>
                    <p className="truncate text-[11.5px] text-text-subtle">
                      {version.gameVersions.join(", ")}
                      {version.loaders.length > 0 && ` · ${version.loaders.join(", ")}`}
                    </p>
                  </div>

                  {version.versionType !== "release" && (
                    <span className="shrink-0 rounded-md bg-warning/15 px-1.5 py-0.5 text-[11px] text-warning capitalize">
                      {version.versionType}
                    </span>
                  )}

                  {pack && (
                    <button
                      type="button"
                      onClick={() => install.mutate(version.id)}
                      disabled={install.isPending}
                      className="shrink-0 rounded-lg px-2.5 py-1 text-[12px] font-medium text-accent transition-colors hover:bg-accent-soft disabled:opacity-50"
                    >
                      {installedEntry?.source.versionId === version.id ? "Reinstall" : "Install"}
                    </button>
                  )}
                </div>
              ))}
            </div>

            {versions.data.length > 5 && (
              <button
                type="button"
                onClick={() => setShowAllVersions((value) => !value)}
                className="w-fit text-[12.5px] text-accent hover:underline"
              >
                {showAllVersions
                  ? "Show fewer"
                  : `Show all ${versions.data.length} versions`}
              </button>
            )}
          </>
        ) : (
          <p className="text-[12.5px] text-text-subtle">
            {pack ? (
              <>
                No versions for {pack.mcVersion}
                {kind === "mod" && pack.loader.kind !== "vanilla" && ` on ${pack.loader.kind}`}.
              </>
            ) : (
              "No versions published."
            )}
          </p>
        )}
      </section>

      {data.gallery.length > 0 && (
        <section className="flex flex-col gap-2">
          <h2 className="text-[13px] font-semibold text-text-muted">Gallery</h2>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-2">
            {data.gallery.map((image) => (
              <img
                key={image.url}
                src={image.url}
                alt={image.title ?? ""}
                loading="lazy"
                className="w-full rounded-[10px] border border-border object-cover"
              />
            ))}
          </div>
        </section>
      )}

      {data.bodyHtml && (
        <section className="flex flex-col gap-2">
          <h2 className="text-[13px] font-semibold text-text-muted">Description</h2>
          <ProjectBody html={data.bodyHtml} />
        </section>
      )}
    </div>
  );
}

/**
 * The rendered description.
 *
 * Links are intercepted rather than followed: a plain anchor click inside a
 * webview would navigate the launcher itself away from the app, so they are
 * handed to the OS browser instead.
 */
function ProjectBody({ html }: { html: string }) {
  return (
    <div
      data-selectable
      onClick={(event) => {
        const anchor = (event.target as HTMLElement).closest("a");
        const href = anchor?.getAttribute("href");
        if (!href) return;
        event.preventDefault();
        void openUrl(href);
      }}
      className="project-body text-[13px] leading-relaxed text-text-muted"
      // Rendered from Markdown and sanitised in Rust before crossing the IPC
      // boundary: no scripts, event handlers, inline styles or non-http URLs.
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function LinkButton({
  url,
  icon,
  children,
}: {
  url: string;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={() => void openUrl(url)}
      className="inline-flex h-7 items-center gap-1.5 rounded-[8px] border border-border bg-surface px-2.5 text-[12px] text-text-muted transition-colors hover:border-border-strong hover:text-text"
    >
      {icon}
      {children}
    </button>
  );
}

function compactNumber(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(0)}K`;
  return String(value);
}

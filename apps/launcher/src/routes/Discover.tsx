import { useEffect, useState } from "react";
import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { Boxes, Compass, TriangleAlert } from "lucide-react";

import { listInstances } from "@/lib/api";
import { accentStyle } from "@/lib/accent";
import { ContentBrowser } from "@/components/ContentBrowser";
import { EmptyState, Page } from "@/components/Page";

/**
 * Browsing Modrinth, launcher-wide.
 *
 * Content is always installed *into* something, so this picks a target
 * instance first and then reuses the same browser the instance page uses.
 * Scoping to an instance is also what makes results trustworthy: they are
 * filtered to that Minecraft version and loader.
 */
export function Discover() {
  const instances = useQuery({ queryKey: ["instances"], queryFn: listInstances });
  const [targetId, setTargetId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const list = instances.data ?? [];
  const target = list.find((instance) => instance.id === targetId) ?? list[0];

  // Keep the selection valid if the chosen instance is deleted elsewhere.
  useEffect(() => {
    if (targetId && !list.some((instance) => instance.id === targetId)) setTargetId(null);
  }, [list, targetId]);

  if (instances.isPending) return <Page title="Discover" subtitle="Browse Modrinth" children={null} />;

  if (!target) {
    return (
      <Page title="Discover" subtitle="Browse Modrinth">
        <EmptyState
          icon={<Compass size={24} />}
          title="Create an instance first"
          description="Mods are installed into an instance, and results are filtered to its Minecraft version and loader — so there needs to be one to install into."
          action={
            <Link
              to="/"
              className="inline-flex h-9 items-center gap-2 rounded-[10px] bg-accent px-4 text-[13px] font-medium text-accent-fg transition-colors hover:bg-accent-hover"
            >
              <Boxes size={15} />
              Go to Library
            </Link>
          }
        />
      </Page>
    );
  }

  return (
    <Page
      title="Discover"
      subtitle="Browse Modrinth"
      actions={
        <label className="flex items-center gap-2 text-[12.5px] text-text-muted">
          Install into
          <select
            value={target.id}
            onChange={(event) => setTargetId(event.target.value)}
            className="h-9 max-w-[220px] rounded-[10px] border border-border bg-surface px-2.5 text-[13px] text-text outline-none focus:border-accent"
          >
            {list.map((instance) => (
              <option key={instance.id} value={instance.id}>
                {instance.name}
              </option>
            ))}
          </select>
        </label>
      }
    >
      <div style={accentStyle(target.id)} className="flex flex-col gap-4">
        {error && (
          <div className="flex items-start gap-2.5 rounded-[12px] border border-danger/35 bg-danger/10 px-3.5 py-3 text-[13px]">
            <TriangleAlert size={16} className="mt-px shrink-0 text-danger" />
            <p data-selectable className="min-w-0 flex-1 leading-relaxed">
              {error}
            </p>
            <button
              type="button"
              onClick={() => setError(null)}
              className="shrink-0 text-text-subtle hover:text-text"
            >
              Dismiss
            </button>
          </div>
        )}

        <ContentBrowser instance={target} onError={setError} />
      </div>
    </Page>
  );
}

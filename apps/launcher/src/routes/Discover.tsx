import { useState } from "react";
import { TriangleAlert } from "lucide-react";

import { ContentBrowser } from "@/components/ContentBrowser";
import { Page } from "@/components/Page";

/**
 * Browsing Modrinth, launcher-wide.
 *
 * Deliberately read-only: everything Modrinth has is listed, unfiltered, and
 * nothing here can be installed. Installing belongs to a modpack, where the
 * Minecraft version and loader are known and results can be narrowed to what
 * will actually run — so it lives behind Add mods on the pack itself rather
 * than being offered here against a target picked from a dropdown.
 */
export function Discover() {
  const [error, setError] = useState<string | null>(null);

  return (
    <Page title="Discover" subtitle="Browse Modrinth">
      <div className="flex flex-col gap-4">
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

        <ContentBrowser pack={null} onError={setError} />
      </div>
    </Page>
  );
}

import { useState } from "react";
import { Link, NavLink, useLocation, useNavigate } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Boxes, ChevronDown, Compass, Plus, Settings, Trash2, User } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { deletePack, errorMessage, listPacks, type PackView } from "@/lib/api";
import { useLauncherStore } from "@/lib/store";
import { cn } from "@/lib/cn";
import { Button } from "./Button";
import { ContextMenu } from "./ContextMenu";
import { Modal } from "./Modal";

interface RailItem {
  to: string;
  icon: LucideIcon;
  label: string;
}

const SECONDARY: RailItem[] = [
  { to: "/accounts", icon: User, label: "Accounts" },
  { to: "/settings", icon: Settings, label: "Settings" },
];

/**
 * Fixed navigation rail. Icon-only, with the label as a hover tooltip.
 *
 * Library expands in place rather than opening a page of its own: the modpacks
 * appear directly under it and push everything below them down, so switching
 * between packs is one click from anywhere in the app.
 */
export function Rail() {
  const [expanded, setExpanded] = useState(true);
  /** Which pack was right-clicked, and where, so the menu can be placed. */
  const [menu, setMenu] = useState<{ pack: PackView; x: number; y: number } | null>(null);
  /** Set while a delete is awaiting confirmation. */
  const [confirming, setConfirming] = useState<PackView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();

  const packs = useQuery({ queryKey: ["packs"], queryFn: listPacks });
  const list = packs.data ?? [];

  const selectedPackId = useLauncherStore((state) => state.selectedPackId);
  const selectPack = useLauncherStore((state) => state.selectPack);
  const setCreating = useLauncherStore((state) => state.setCreating);

  const remove = useMutation({
    mutationFn: (pack: PackView) => deletePack(pack.id),
    onSuccess: (_result, pack) => {
      setConfirming(null);
      void queryClient.invalidateQueries({ queryKey: ["packs"] });
      // Leaving the page of a pack that no longer exists would sit on a failed
      // query forever. The Library resolves a new active pack on its own.
      if (location.pathname === `/pack/${pack.id}`) void navigate("/");
    },
    onError: (err) => setError(errorMessage(err)),
  });

  return (
    <nav className="flex w-16 shrink-0 flex-col items-center border-r border-border bg-bg-elevated py-3">
      <div className="flex min-h-0 flex-1 flex-col items-center gap-1 overflow-y-auto">
        <RailButton
          to="/"
          icon={Boxes}
          label="Library"
          // The chevron marks it as an expander; the click still navigates
          // home, so Library never becomes a control that only opens a menu.
          badge={
            <ChevronDown
              size={11}
              className={cn(
                "absolute right-0.5 bottom-0.5 text-text-subtle transition-transform duration-150",
                expanded && "rotate-180",
              )}
            />
          }
          onClick={() => setExpanded((open) => !open)}
        />

        {expanded && (
          // Roomier than the rail's own spacing: these are separate modpacks,
          // not a tight group of controls, and they need to read as individual
          // targets rather than one stacked block.
          <div className="flex w-full flex-col items-center gap-2.5 py-1.5">
            {list.map((pack) => (
              <PackButton
                key={pack.id}
                pack={pack}
                active={pack.id === selectedPackId}
                onSelect={() => selectPack(pack.id)}
                onContextMenu={(event) => {
                  // Replaces the webview's own menu, which offers reload and
                  // view-source and nothing a player wants.
                  event.preventDefault();
                  setMenu({ pack, x: event.clientX, y: event.clientY });
                }}
              />
            ))}

            <button
              type="button"
              title="New modpack"
              aria-label="New modpack"
              onClick={() => setCreating(true)}
              className="grid size-9 place-items-center rounded-[11px] border border-dashed border-border-strong text-text-subtle transition-colors hover:border-accent hover:text-accent"
            >
              <Plus size={16} />
            </button>
          </div>
        )}

        <RailButton to="/discover" icon={Compass} label="Discover" />
      </div>

      <div className="flex shrink-0 flex-col items-center gap-1 pt-1">
        {SECONDARY.map((item) => (
          <RailButton key={item.to} {...item} />
        ))}
      </div>

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            {
              label: "Delete",
              icon: <Trash2 size={14} />,
              danger: true,
              onSelect: () => setConfirming(menu.pack),
            },
          ]}
        />
      )}

      {/* Deleting takes the worlds with it and cannot be undone, so it is
          confirmed rather than done straight off a menu click. */}
      <Modal
        open={confirming !== null}
        title={`Delete ${confirming?.name ?? ""}?`}
        onClose={() => setConfirming(null)}
        footer={
          <>
            <Button onClick={() => setConfirming(null)}>Cancel</Button>
            <Button
              disabled={remove.isPending}
              onClick={() => confirming && remove.mutate(confirming)}
              className="border-danger/40 text-danger hover:bg-danger hover:text-white"
            >
              {remove.isPending ? "Deleting…" : "Delete"}
            </Button>
          </>
        }
      >
        <p className="text-[13px] leading-relaxed text-text-muted">
          This removes the modpack and everything inside it, including its worlds. Shared
          libraries, assets and Java runtimes are left alone. This cannot be undone.
        </p>
        {error && <p className="mt-3 text-[12.5px] text-danger">{error}</p>}
      </Modal>
    </nav>
  );
}

/**
 * One modpack in the rail.
 *
 * Switching packs lands on the pack's main page — the home screen with its Play
 * button — not on its management page. Editing is a separate, deliberate step
 * behind Edit; picking a pack in the rail just says "this is the one I want to
 * play now".
 *
 * A plain link rather than a programmatic `navigate()`, which did not fire when
 * called alongside the store update in the same handler. `Link` and not
 * `NavLink` because every pack points at the same route: NavLink would mark all
 * of them `aria-current` at once, so the selected one is marked by hand.
 *
 * No pack artwork exists yet, so this is the pack's initials on the app accent.
 * Packs deliberately have no colour of their own — the ring marks the selected
 * one — and this becomes an icon later without the surrounding layout changing.
 */
function PackButton({
  pack,
  active,
  onSelect,
  onContextMenu,
}: {
  pack: PackView;
  active: boolean;
  onSelect: () => void;
  onContextMenu: (event: React.MouseEvent) => void;
}) {
  return (
    <Link
      to="/"
      title={pack.name}
      aria-label={pack.name}
      aria-current={active ? "true" : undefined}
      onClick={onSelect}
      onContextMenu={onContextMenu}
      className={cn(
        "relative grid size-9 shrink-0 place-items-center rounded-[11px] bg-accent text-[12px] font-semibold text-accent-fg transition-all duration-150",
        active
          ? "ring-2 ring-accent-ring ring-offset-2 ring-offset-bg-elevated"
          : "opacity-80 hover:opacity-100",
      )}
    >
      {initials(pack.name)}
    </Link>
  );
}

/**
 * Up to two letters identifying a pack.
 *
 * Only words that actually start with a letter count, because pack names are
 * routinely "NeoForge 26.2" — one initial per word would render that as "N2",
 * which reads as a chemical formula rather than a name. A single qualifying
 * word falls back to its first two letters, so it becomes "NE".
 */
function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter((word) => /^\p{L}/u.test(word));
  if (words.length === 0) return name.trim().slice(0, 2).toUpperCase() || "?";
  if (words.length === 1) return words[0]!.slice(0, 2).toUpperCase();
  return (words[0]![0]! + words[1]![0]!).toUpperCase();
}

function RailButton({
  to,
  icon: Icon,
  label,
  badge,
  onClick,
}: RailItem & { badge?: React.ReactNode; onClick?: () => void }) {
  return (
    <NavLink
      to={to}
      // `end` only on the index route, or Library would stay lit everywhere.
      end={to === "/"}
      title={label}
      aria-label={label}
      onClick={onClick}
      className={({ isActive }) =>
        cn(
          "group relative grid size-11 shrink-0 place-items-center rounded-[12px] transition-colors duration-150",
          isActive
            ? "bg-accent-soft text-accent"
            : "text-text-subtle hover:bg-surface-2 hover:text-text",
        )
      }
    >
      {({ isActive }) => (
        <>
          <Icon size={20} strokeWidth={isActive ? 2.2 : 1.9} />
          {badge}
          {/* Active marker on the rail edge, so the current section is legible
              at a glance without reading the icons. The button is 44px centred
              in a 64px rail, so -8px lands it 2px inside the window edge —
              anything further left is clipped off-screen entirely. */}
          <span
            className={cn(
              "absolute -left-2 h-5 w-[3px] rounded-r-full bg-accent transition-all duration-200",
              isActive ? "opacity-100" : "scale-y-0 opacity-0",
            )}
          />
        </>
      )}
    </NavLink>
  );
}

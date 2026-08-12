import { NavLink } from "react-router";
import { Boxes, Compass, Layers, Settings, User } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { cn } from "@/lib/cn";

interface RailItem {
  to: string;
  icon: LucideIcon;
  label: string;
}

const PRIMARY: RailItem[] = [
  { to: "/", icon: Boxes, label: "Library" },
  { to: "/packs", icon: Layers, label: "Packs" },
  { to: "/discover", icon: Compass, label: "Discover" },
];

const SECONDARY: RailItem[] = [
  { to: "/accounts", icon: User, label: "Accounts" },
  { to: "/settings", icon: Settings, label: "Settings" },
];

/** Fixed navigation rail. Icon-only, with the label as a hover tooltip. */
export function Rail() {
  return (
    <nav className="flex w-16 shrink-0 flex-col items-center gap-1 border-r border-border bg-bg-elevated py-3">
      {PRIMARY.map((item) => (
        <RailButton key={item.to} {...item} />
      ))}
      <div className="flex-1" />
      {SECONDARY.map((item) => (
        <RailButton key={item.to} {...item} />
      ))}
    </nav>
  );
}

function RailButton({ to, icon: Icon, label }: RailItem) {
  return (
    <NavLink
      to={to}
      // `end` only on the index route, or Library would stay lit everywhere.
      end={to === "/"}
      title={label}
      aria-label={label}
      className={({ isActive }) =>
        cn(
          "group relative grid size-11 place-items-center rounded-[12px] transition-colors duration-150",
          isActive
            ? "bg-accent-soft text-accent"
            : "text-text-subtle hover:bg-surface-2 hover:text-text",
        )
      }
    >
      {({ isActive }) => (
        <>
          <Icon size={20} strokeWidth={isActive ? 2.2 : 1.9} />
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

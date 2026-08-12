import { Download, KeyRound, Loader2, Play, RefreshCw, Square } from "lucide-react";

import type { PrimaryAction } from "@/lib/api";
import { cn } from "@/lib/cn";

/**
 * Colour carries the meaning here, so it is fixed rather than derived from the
 * app or pack accent: green is "this gets you into the game" — whether that
 * means linking an account, installing, or launching — and orange is the one
 * state that asks you to take something on first.
 */
const PRESENTATION: Record<
  PrimaryAction["kind"],
  { label: string; icon: typeof Play; tone: "play" | "update" | "muted" }
> = {
  linkMinecraft: { label: "Link Minecraft", icon: KeyRound, tone: "play" },
  busy: { label: "Working", icon: Loader2, tone: "muted" },
  running: { label: "Running", icon: Square, tone: "muted" },
  install: { label: "Install", icon: Download, tone: "play" },
  update: { label: "Update", icon: RefreshCw, tone: "update" },
  play: { label: "Play", icon: Play, tone: "play" },
};

/**
 * The one control on a modpack. Which state it is in is decided in Rust and
 * arrives on the pack — this component only renders it, so the label can
 * never disagree with what the click actually does.
 */
export function PrimaryButton({
  action,
  onClick,
  className,
  size = "md",
}: {
  action: PrimaryAction;
  onClick: () => void;
  className?: string;
  /** `lg` is the home screen's single action; the rest are inline controls. */
  size?: "sm" | "md" | "lg";
}) {
  const { label, icon: Icon, tone } = PRESENTATION[action.kind];
  const disabled = action.kind === "busy" || action.kind === "running";
  const spinning = action.kind === "busy";

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      className={cn(
        "inline-flex shrink-0 items-center justify-center gap-2 font-semibold transition-all duration-150",
        size === "sm" && "h-9 px-4 text-[13px] rounded-[10px]",
        size === "md" && "h-11 px-6 text-[14px] rounded-[12px]",
        size === "lg" && "h-14 min-w-[280px] px-10 text-[16px] rounded-[14px]",
        tone === "play" &&
          "bg-play text-play-fg hover:bg-play-hover active:bg-play-active shadow-[0_2px_14px_-4px_var(--play-ring)]",
        // Updates get their own colour so an out-of-date pack is obvious
        // without reading any labels.
        tone === "update" &&
          "bg-update text-update-fg hover:bg-update-hover active:bg-update-active shadow-[0_2px_14px_-4px_var(--update-ring)]",
        tone === "muted" && "bg-surface-3 text-text-muted",
        disabled && "cursor-default",
        className,
      )}
    >
      <Icon
        size={size === "sm" ? 15 : size === "lg" ? 20 : 17}
        className={cn(spinning && "animate-spin")}
      />
      {label}
      {action.kind === "update" && action.changes > 0 && (
        <span className="ml-0.5 rounded-full bg-black/20 px-2 py-0.5 text-[11px] tabular-nums">
          {action.changes}
        </span>
      )}
    </button>
  );
}

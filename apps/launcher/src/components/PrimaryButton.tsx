import { Download, KeyRound, Loader2, Play, RefreshCw, Square } from "lucide-react";

import type { PrimaryAction } from "@/lib/api";
import { cn } from "@/lib/cn";

const PRESENTATION: Record<
  PrimaryAction["kind"],
  { label: string; icon: typeof Play; tone: "accent" | "update" | "muted" }
> = {
  linkMinecraft: { label: "Link Minecraft", icon: KeyRound, tone: "accent" },
  busy: { label: "Working", icon: Loader2, tone: "muted" },
  running: { label: "Running", icon: Square, tone: "muted" },
  install: { label: "Install", icon: Download, tone: "accent" },
  update: { label: "Update", icon: RefreshCw, tone: "update" },
  play: { label: "Play", icon: Play, tone: "accent" },
};

/**
 * The one control on an instance. Which state it is in is decided in Rust and
 * arrives on the instance — this component only renders it, so the label can
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
  size?: "sm" | "md";
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
        size === "sm" ? "h-9 px-4 text-[13px] rounded-[10px]" : "h-11 px-6 text-[14px] rounded-[12px]",
        tone === "accent" &&
          "bg-accent text-accent-fg hover:bg-accent-hover active:bg-accent-active shadow-[0_2px_14px_-4px_var(--accent-ring)]",
        // Updates get their own colour so an out-of-date pack is obvious in a
        // grid without reading any labels.
        tone === "update" && "bg-warning text-black hover:brightness-110",
        tone === "muted" && "bg-surface-3 text-text-muted",
        disabled && "cursor-default",
        className,
      )}
    >
      <Icon size={size === "sm" ? 15 : 17} className={cn(spinning && "animate-spin")} />
      {label}
      {action.kind === "update" && action.changes > 0 && (
        <span className="ml-0.5 rounded-full bg-black/20 px-2 py-0.5 text-[11px] tabular-nums">
          {action.changes}
        </span>
      )}
    </button>
  );
}

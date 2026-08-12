import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { cn } from "@/lib/cn";

export interface ContextMenuItem {
  label: string;
  icon?: React.ReactNode;
  onSelect: () => void;
  /** Renders in the danger colour. For destructive, irreversible actions. */
  danger?: boolean;
}

/**
 * A right-click menu anchored to the pointer.
 *
 * Positioned against the viewport through a portal so it is never clipped by a
 * scrolling ancestor, and nudged back inside the window when opened near an
 * edge — a menu half off-screen at the bottom of the rail is the normal case,
 * not the exception.
 */
export function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Fixed from the very first frame, not just after measuring. A block-level
  // div in `body` is as wide as the window, and measuring *that* makes the
  // clamp below compute a negative left — the menu lands off the left edge of
  // the window. Fixed positioning shrinks it to its content, so the width it
  // reports is the width it has. Hidden until placed, since a frame in the
  // wrong corner reads as a flicker.
  const [style, setStyle] = useState<React.CSSProperties>({
    position: "fixed",
    left: x,
    top: y,
    visibility: "hidden",
  });

  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;

    const { width, height } = menu.getBoundingClientRect();
    const margin = 8;

    // Clamped at both ends: near the right edge it slides back inside, and the
    // rail is hard against the left edge, where an unclamped value would push
    // it out of the window entirely.
    setStyle({
      position: "fixed",
      left: Math.max(margin, Math.min(x, window.innerWidth - width - margin)),
      top: Math.max(margin, Math.min(y, window.innerHeight - height - margin)),
    });
  }, [x, y]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (ref.current?.contains(event.target as Node)) return;
      onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    // Anchored to a point, not an element: once the page moves under it the
    // menu no longer refers to anything, so it closes rather than follows.
    const onScroll = () => onClose();

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={ref}
      role="menu"
      style={style}
      className="z-50 min-w-[160px] rounded-[10px] border border-border bg-bg-elevated p-1 shadow-[0_12px_32px_rgba(0,0,0,0.45)]"
    >
      {items.map((item) => (
        <button
          key={item.label}
          type="button"
          role="menuitem"
          onClick={() => {
            item.onSelect();
            onClose();
          }}
          className={cn(
            "flex w-full items-center gap-2 rounded-[7px] px-2 py-1.5 text-left text-[12.5px] transition-colors",
            item.danger
              ? "text-danger hover:bg-danger/12"
              : "text-text-muted hover:bg-surface-2 hover:text-text",
          )}
        >
          {item.icon}
          {item.label}
        </button>
      ))}
    </div>,
    document.body,
  );
}

import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

import { cn } from "@/lib/cn";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  /** Secondary text shown dimmed after the label — "(prerelease)" and friends. */
  hint?: string;
}

/**
 * The launcher's dropdown.
 *
 * A native `<select>` renders as an OS widget the app has no control over: on
 * Windows it ignores the theme entirely and pops a white list into a dark UI.
 * This is a button plus a listbox, so it looks like the rest of the launcher in
 * both themes.
 *
 * The list is rendered through a portal and positioned against the trigger's
 * viewport rect. Anchoring it in place would let a scrollable ancestor clip it
 * — the version list inside the New modpack dialog is exactly that case — and
 * it flips above the trigger when there isn't room below.
 */
export function Select<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
  disabled = false,
  className,
}: {
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
  ariaLabel: string;
  disabled?: boolean;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const listId = useId();

  const selectedIndex = options.findIndex((option) => option.value === value);
  const selected = selectedIndex === -1 ? undefined : options[selectedIndex];

  // Opening starts from the current choice, not the top of the list.
  const openList = () => {
    if (disabled) return;
    setActive(selectedIndex === -1 ? 0 : selectedIndex);
    setOpen(true);
  };

  const commit = (index: number) => {
    const option = options[index];
    if (option) onChange(option.value);
    setOpen(false);
    triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!open) return;

    // Only a click genuinely outside both the trigger and the list closes it.
    // Scroll and resize reposition the list instead of dismissing it — a
    // capture-phase scroll listener sees events from every element on the page,
    // including the list scrolling its own highlighted row into view, which
    // would make the list close the instant it opened.
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (triggerRef.current?.contains(target) || listRef.current?.contains(target)) return;
      setOpen(false);
    };

    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [open]);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (!open) {
      if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        openList();
      }
      return;
    }

    switch (event.key) {
      case "Escape":
        event.preventDefault();
        setOpen(false);
        break;
      case "ArrowDown":
        event.preventDefault();
        setActive((index) => Math.min(index + 1, options.length - 1));
        break;
      case "ArrowUp":
        event.preventDefault();
        setActive((index) => Math.max(index - 1, 0));
        break;
      case "Home":
        event.preventDefault();
        setActive(0);
        break;
      case "End":
        event.preventDefault();
        setActive(options.length - 1);
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        commit(active);
        break;
      case "Tab":
        setOpen(false);
        break;
    }
  };

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={(event) => {
          // Stops here rather than bubbling: a `<label>` ancestor — which is
          // how every Field wraps its control — would re-dispatch the click to
          // the button it labels, and `<button>` is labelable. That second
          // click toggled the list shut the instant it opened.
          event.stopPropagation();
          if (open) setOpen(false);
          else openList();
        }}
        onKeyDown={onKeyDown}
        className={cn(
          "inline-flex h-9 items-center justify-between gap-2 rounded-[10px] border bg-surface px-2.5 text-[13px] text-text transition-colors",
          open ? "border-accent" : "border-border hover:border-border-strong",
          disabled && "opacity-50",
          className,
        )}
      >
        <span className="truncate">{selected?.label ?? ""}</span>
        <ChevronDown
          size={15}
          className={cn(
            "shrink-0 text-text-subtle transition-transform duration-150",
            open && "rotate-180",
          )}
        />
      </button>

      {open && (
        <Listbox
          id={listId}
          ref={listRef}
          anchor={triggerRef.current}
          options={options}
          activeIndex={active}
          selectedIndex={selectedIndex}
          onHover={setActive}
          onPick={commit}
          onKeyDown={onKeyDown}
        />
      )}
    </>
  );
}

function Listbox<T extends string>({
  id,
  ref,
  anchor,
  options,
  activeIndex,
  selectedIndex,
  onHover,
  onPick,
  onKeyDown,
}: {
  id: string;
  ref: React.RefObject<HTMLDivElement | null>;
  anchor: HTMLElement | null;
  options: SelectOption<T>[];
  activeIndex: number;
  selectedIndex: number;
  onHover: (index: number) => void;
  onPick: (index: number) => void;
  onKeyDown: (event: React.KeyboardEvent) => void;
}) {
  const [style, setStyle] = useState<React.CSSProperties>({ opacity: 0 });

  // Measured before paint: a frame at the wrong position reads as a flicker.
  useLayoutEffect(() => {
    if (!anchor) return;

    const place = () => {
      const rect = anchor.getBoundingClientRect();
      const gap = 4;
      const maxHeight = 280;
      const below = window.innerHeight - rect.bottom - gap;
      const above = rect.top - gap;
      // Open upwards only when below is genuinely too cramped and above is
      // roomier — a list that flips on every small scroll is worse than a
      // short one.
      const flip = below < Math.min(maxHeight, 160) && above > below;

      setStyle({
        position: "fixed",
        left: rect.left,
        minWidth: rect.width,
        maxHeight: Math.min(maxHeight, Math.max(flip ? above : below, 120)),
        ...(flip ? { bottom: window.innerHeight - rect.top + gap } : { top: rect.bottom + gap }),
      });
    };

    place();

    // Follow the trigger rather than dismissing when the page moves under it.
    // Scrolls started inside the list are its own business and must not
    // re-place it mid-interaction.
    const onScroll = (event: Event) => {
      if (ref.current?.contains(event.target as Node)) return;
      place();
    };

    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", place);
    };
  }, [anchor, ref]);

  // Keep the highlighted row in view when arrowing through a long list.
  useEffect(() => {
    ref.current
      ?.querySelector(`[data-index="${activeIndex}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, ref]);

  return createPortal(
    <div
      ref={ref}
      id={id}
      role="listbox"
      tabIndex={-1}
      onKeyDown={onKeyDown}
      style={style}
      className="z-50 overflow-y-auto rounded-[10px] border border-border bg-bg-elevated p-1 shadow-[0_12px_32px_rgba(0,0,0,0.45)]"
    >
      {options.map((option, index) => (
        <div
          key={option.value}
          data-index={index}
          role="option"
          aria-selected={index === selectedIndex}
          onMouseEnter={() => onHover(index)}
          onClick={() => onPick(index)}
          className={cn(
            "flex cursor-pointer items-center gap-2 rounded-[7px] px-2 py-1.5 text-[12.5px]",
            index === activeIndex ? "bg-accent-soft text-accent" : "text-text-muted",
          )}
        >
          <Check
            size={13}
            className={cn("shrink-0", index === selectedIndex ? "opacity-100" : "opacity-0")}
          />
          <span className="min-w-0 flex-1 truncate">{option.label}</span>
          {option.hint && <span className="shrink-0 text-[11.5px] text-text-subtle">{option.hint}</span>}
        </div>
      ))}
    </div>,
    portalTarget(anchor),
  );
}

/**
 * Where the list should be rendered.
 *
 * A modal `<dialog>` is promoted to the browser's top layer, which sits above
 * the whole document — so a list portalled to `<body>` renders *behind* the
 * dialog however high its z-index goes. Portalling into the dialog itself puts
 * the list in that same top layer. Outside a dialog, `<body>` is still right:
 * it escapes any scrolling or clipping ancestor.
 */
function portalTarget(anchor: HTMLElement | null): HTMLElement {
  return anchor?.closest("dialog[open]") ?? document.body;
}

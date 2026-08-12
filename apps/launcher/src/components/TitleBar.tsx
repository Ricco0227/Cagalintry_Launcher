import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, Copy, X } from "lucide-react";

import { cn } from "@/lib/cn";
import logo from "@/assets/logo.png";

/**
 * Custom window chrome. The native title bar is off so the launcher can put its
 * own controls and search in that strip instead of losing 32px to an empty bar.
 *
 * `data-tauri-drag-region` is what makes the strip behave like a title bar for
 * dragging and double-click-to-maximise; anything interactive inside it has to
 * opt out, or clicks land on the drag handler instead of the button.
 */
export function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let disposed = false;

    void win.isMaximized().then((value) => {
      if (!disposed) setMaximized(value);
    });

    // Covers snapping and the OS maximising us, not just our own button.
    const unlisten = win.onResized(() => {
      void win.isMaximized().then((value) => {
        if (!disposed) setMaximized(value);
      });
    });

    return () => {
      disposed = true;
      void unlisten.then((off) => off());
    };
  }, []);

  const win = getCurrentWindow();

  return (
    <header
      data-tauri-drag-region
      className="flex h-10 shrink-0 items-center justify-between border-b border-border bg-bg-elevated pl-3 select-none"
    >
      <div data-tauri-drag-region className="flex items-center gap-2.5">
        <BrandMark />
        <span data-tauri-drag-region className="text-[13px] font-semibold tracking-tight">
          Cagalintry Launcher
        </span>
      </div>

      <div className="flex h-full items-center">
        <WindowButton label="Minimize" onClick={() => void win.minimize()}>
          <Minus size={15} strokeWidth={2} />
        </WindowButton>
        <WindowButton
          label={maximized ? "Restore" : "Maximize"}
          onClick={() => void win.toggleMaximize()}
        >
          {maximized ? <Copy size={13} strokeWidth={2} /> : <Square size={12} strokeWidth={2} />}
        </WindowButton>
        <WindowButton label="Close" danger onClick={() => void win.close()}>
          <X size={16} strokeWidth={2} />
        </WindowButton>
      </div>
    </header>
  );
}

function WindowButton({
  children,
  label,
  onClick,
  danger = false,
}: {
  children: React.ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "grid h-10 w-12 place-items-center text-text-muted transition-colors",
        danger
          ? "hover:bg-danger hover:text-white"
          : "hover:bg-surface-2 hover:text-text",
      )}
    >
      {children}
    </button>
  );
}

function BrandMark() {
  return (
    <img
      src={logo}
      alt=""
      data-tauri-drag-region
      // Rounding is applied here rather than baked into the asset so the same
      // source image can be reused at other sizes and shapes.
      className="size-[22px] rounded-[7px] object-cover ring-1 ring-border"
      draggable={false}
    />
  );
}

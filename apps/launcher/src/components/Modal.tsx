import { useEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";

/**
 * A minimal dialog built on the native `<dialog>` element, which brings focus
 * trapping, the top layer and inert background for free — all things a
 * hand-rolled overlay has to reimplement badly.
 */
export function Modal({
  open,
  title,
  onClose,
  children,
  footer,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      // Esc fires `cancel`; routing it through onClose keeps React's state the
      // single source of truth rather than the DOM's own open flag.
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        // Clicks on the backdrop land on the dialog element itself.
        if (event.target === ref.current) onClose();
      }}
      className="m-auto w-[440px] max-w-[calc(100vw-3rem)] rounded-[16px] border border-border bg-bg-elevated p-0 text-text shadow-[var(--shadow-pop)] backdrop:bg-black/55 backdrop:backdrop-blur-[2px]"
    >
      <div className="flex items-center justify-between border-b border-border px-5 py-3.5">
        <h2 className="text-[15px] font-semibold">{title}</h2>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close"
          className="grid size-7 place-items-center rounded-lg text-text-subtle transition-colors hover:bg-surface-2 hover:text-text"
        >
          <X size={16} />
        </button>
      </div>

      <div className="px-5 py-4">{children}</div>

      {footer && (
        <div className="flex justify-end gap-2 border-t border-border px-5 py-3.5">{footer}</div>
      )}
    </dialog>
  );
}

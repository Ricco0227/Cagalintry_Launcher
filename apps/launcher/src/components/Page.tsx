import type { ReactNode } from "react";

/**
 * Standard page frame: a sticky header strip and a single scroll container.
 *
 * Scrolling lives here rather than on the body so the rail and title bar stay
 * fixed, and so wide content (log output, tables) can scroll inside its own
 * box without the whole window shifting sideways.
 */
export function Page({
  title,
  subtitle,
  actions,
  children,
}: {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-end justify-between gap-4 px-8 pt-7 pb-5">
        <div className="min-w-0">
          <h1 className="truncate text-[22px] leading-tight font-semibold">{title}</h1>
          {subtitle && (
            <p className="mt-1 truncate text-[13px] text-text-muted">{subtitle}</p>
          )}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-8">{children}</div>
    </div>
  );
}

/** Centred placeholder for a section with nothing in it yet. */
export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="grid min-h-[380px] place-items-center">
      <div className="max-w-[420px] text-center">
        <div className="mx-auto grid size-14 place-items-center rounded-2xl bg-surface-2 text-text-subtle">
          {icon}
        </div>
        <h2 className="mt-5 text-[16px] font-semibold">{title}</h2>
        <p className="mt-2 text-[13px] leading-relaxed text-text-muted">{description}</p>
        {action && <div className="mt-6 flex justify-center">{action}</div>}
      </div>
    </div>
  );
}

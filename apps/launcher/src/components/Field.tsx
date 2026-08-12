import type { ReactNode } from "react";

import { cn } from "@/lib/cn";

/** A labelled setting with optional explanatory text underneath. */
export function Field({
  label,
  hint,
  children,
  className,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <label className="text-[12.5px] font-medium text-text">{label}</label>
      {children}
      {hint && <p className="text-[12px] leading-relaxed text-text-subtle">{hint}</p>}
    </div>
  );
}

/** Grouped settings under a heading, as a bordered card. */
export function Section({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-[14px] border border-border bg-surface">
      <div className="border-b border-border px-5 py-3.5">
        <h2 className="text-[14px] font-semibold">{title}</h2>
        {description && <p className="mt-0.5 text-[12.5px] text-text-muted">{description}</p>}
      </div>
      <div className="flex flex-col gap-5 px-5 py-4">{children}</div>
    </section>
  );
}

export const inputClass =
  "h-9 w-full rounded-[10px] border border-border bg-bg px-3 text-[13px] outline-none placeholder:text-text-subtle focus:border-accent";

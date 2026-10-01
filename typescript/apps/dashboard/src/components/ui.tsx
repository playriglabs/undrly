import clsx from "clsx";
import type { ReactNode } from "react";
import { direction, formatAge, formatPercent } from "../lib/format";

/** A titled block on the detail page. Square, hairline-bordered. */
export function Panel({
  title,
  children,
  className,
}: {
  title: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={clsx("border border-line bg-panel", className)}>
      <h2 className="border-b border-line px-5 py-3.5 font-mono text-[11px] tracking-[0.1em] text-faint uppercase">
        {title}
      </h2>
      {children}
    </section>
  );
}

/** Label/value rows inside a Panel. */
export function Fields({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="divide-y divide-line">
      {rows.map(([label, value]) => (
        <div
          key={label}
          className="flex items-baseline justify-between gap-6 px-5 py-3 text-[14px]"
        >
          <dt className="shrink-0 text-muted">{label}</dt>
          <dd className="min-w-0 truncate text-right tabular">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

export function Change({
  percent,
  className,
}: {
  percent: string | null | undefined;
  className?: string;
}) {
  if (!percent) return <span className={clsx("text-faint", className)}>—</span>;
  const dir = direction(percent);
  return (
    <span
      className={clsx(
        "tabular",
        dir === "up" ? "text-up" : dir === "down" ? "text-down" : "text-muted",
        className,
      )}
    >
      {formatPercent(percent)}
    </span>
  );
}

export function Tag({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: "neutral" | "up" | "down";
}) {
  return (
    <span
      className={clsx(
        "inline-flex items-center border px-1.5 py-0.5 font-mono text-[10.5px] tracking-[0.04em] uppercase",
        tone === "up" && "border-[#cfeea033] text-up",
        tone === "down" && "border-[#e3876b33] text-down",
        tone === "neutral" && "border-line-strong text-muted",
      )}
    >
      {children}
    </span>
  );
}

export function ErrorState({ title, message }: { title: string; message: string }) {
  return (
    <div className="border border-dashed border-line-strong px-6 py-14 text-center">
      <p className="font-display text-2xl">{title}</p>
      <p className="mt-2 text-[14px] text-muted">{message}</p>
    </div>
  );
}

/** Time since `iso`. Server and browser clocks differ, so the text is allowed to change on hydration. */
export function Age({ iso, suffix = "" }: { iso: string; suffix?: string }) {
  return (
    <span suppressHydrationWarning>
      {formatAge(Date.now() - Date.parse(iso))}
      {suffix}
    </span>
  );
}

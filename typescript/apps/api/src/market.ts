/**
 * Pure market arithmetic for quotes served by the API: quote age and the
 * freshness policy's elapsed time. Exact integer arithmetic only.
 */
import { v1 } from "@undrly/contracts";

/**
 * Whole milliseconds from `asOf` to `now`, never negative: `max(0,
 * floor(now - asOf))`. Elapsed wall-clock time only; freshness is a
 * separate, cadence-aware verdict.
 */
export function ageMs(asOf: string, now: Date): number {
  const elapsed = BigInt(now.getTime()) * 1000n - v1.timestampMicros(asOf);
  return elapsed <= 0n ? 0 : Number(elapsed / 1000n);
}

/** Mirrors `undrly_core::FreshnessClock`. */
export type FreshnessClock = "continuous" | "weekdays";

/**
 * Milliseconds from `from` to `to` that a freshness policy counts: all of
 * them (`continuous`), or only those on Monday to Friday, UTC (`weekdays`:
 * for rates published on business days, so the weekend does not age a
 * Friday rate). No holiday calendar. Never negative. Policy only: `ageMs`
 * stays literal elapsed time.
 */
export function policyElapsedMs(from: Date, to: Date, clock: FreshnessClock): number {
  const [start, end] = [from.getTime(), to.getTime()];
  if (end <= start) return 0;
  if (clock === "continuous") return end - start;
  let counted = 0;
  for (let t = start; t < end; ) {
    const d = new Date(t);
    const nextDay = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate() + 1);
    const until = Math.min(nextDay, end);
    const weekday = d.getUTCDay();
    if (weekday !== 0 && weekday !== 6) counted += until - t;
    t = until;
  }
  return counted;
}

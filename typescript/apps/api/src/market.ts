/**
 * Pure market arithmetic for canonical quotes served by the API: quote age
 * and the `mean-venue-mid-v1` price of a set of venue quotes (used for the
 * 24-hour baseline). Exact decimals only; no floating point.
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

/**
 * `mean-venue-mid-v1` over venue quotes, exactly as `undrly_core::quote`
 * computes it: `mid_i = (bid_i + ask_i) / 2` at scale
 * `max(scale(bid_i), scale(ask_i)) + 1`; `price = Σ mid_i / n` at scale
 * `max(scale(mid_i)) + 1`, half to even (exact for n ≤ 2). `null` for no
 * quotes or a malformed decimal.
 */
export function meanVenueMid(quotes: { bid: string; ask: string }[]): string | null {
  const two: v1.Decimal = { mantissa: 2n, scale: 0 };
  const mids: v1.Decimal[] = [];
  for (const q of quotes) {
    const [bid, ask] = [v1.parseDecimal(q.bid), v1.parseDecimal(q.ask)];
    if (bid === null || ask === null) return null;
    const mid = v1.divide(v1.add(bid, ask), two, Math.max(bid.scale, ask.scale) + 1);
    if (mid === null) return null;
    mids.push(mid);
  }
  const first = mids[0];
  if (first === undefined) return null;
  const sum = mids.slice(1).reduce(v1.add, first);
  const scale = Math.max(...mids.map((m) => m.scale)) + 1;
  const mean = v1.divide(sum, { mantissa: BigInt(mids.length), scale: 0 }, scale);
  return mean === null ? null : v1.formatDecimal(mean);
}

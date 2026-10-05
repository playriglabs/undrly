/**
 * Exact decimal arithmetic on canonical decimal strings (as
 * `rust_decimal::Decimal` prints them), with `bigint` mantissas. No floating
 * point anywhere. Used to state and check the derived quote fields
 * (`spread`, `spreadBps`, `change24h`).
 */

const DECIMAL = /^(-?)(0|[1-9][0-9]*)(?:\.([0-9]+))?$/;

/** `mantissa × 10^-scale`. */
export type Decimal = { mantissa: bigint; scale: number };

export function parseDecimal(text: string): Decimal | null {
  const m = DECIMAL.exec(text);
  if (m === null) return null;
  const fraction = m[3] ?? "";
  const magnitude = BigInt(`${m[2]}${fraction}`);
  return { mantissa: m[1] === "-" ? -magnitude : magnitude, scale: fraction.length };
}

/** Canonical text, keeping the scale (`1.10` stays `1.10`); zero is never negative. */
export function formatDecimal(d: Decimal): string {
  const negative = d.mantissa < 0n;
  const digits = (negative ? -d.mantissa : d.mantissa).toString().padStart(d.scale + 1, "0");
  const whole = digits.slice(0, digits.length - d.scale);
  const fraction = digits.slice(digits.length - d.scale);
  const body = d.scale === 0 ? whole : `${whole}.${fraction}`;
  return negative ? `-${body}` : body;
}

function upscale(d: Decimal, scale: number): bigint {
  return d.mantissa * 10n ** BigInt(scale - d.scale);
}

/** `a - b`, exact, at scale `max(scale(a), scale(b))`. */
export function subtract(a: Decimal, b: Decimal): Decimal {
  const scale = Math.max(a.scale, b.scale);
  return { mantissa: upscale(a, scale) - upscale(b, scale), scale };
}

/** `a + b`, exact, at scale `max(scale(a), scale(b))`. */
export function add(a: Decimal, b: Decimal): Decimal {
  const scale = Math.max(a.scale, b.scale);
  return { mantissa: upscale(a, scale) + upscale(b, scale), scale };
}

/** Numeric order: negative, zero or positive as `a` is below, equal to or above `b`. */
export function compare(a: Decimal, b: Decimal): number {
  const d = subtract(a, b).mantissa;
  return d < 0n ? -1 : d > 0n ? 1 : 0;
}

/**
 * `a × factor / b`, rounded half to even at `scale`, from the exact
 * rational value (one rounding). `null` when `b` is zero.
 */
export function divide(a: Decimal, b: Decimal, scale: number, factor = 1n): Decimal | null {
  if (b.mantissa === 0n) return null;
  // a/b = (ma / 10^sa) / (mb / 10^sb); result mantissa = a/b × factor × 10^scale.
  let num = a.mantissa * factor * 10n ** BigInt(b.scale + scale);
  let den = b.mantissa * 10n ** BigInt(a.scale);
  if (den < 0n) {
    num = -num;
    den = -den;
  }
  const negative = num < 0n;
  const n = negative ? -num : num;
  let q = n / den;
  const twice = (n % den) * 2n;
  if (twice > den || (twice === den && q % 2n === 1n)) q += 1n;
  return { mantissa: negative ? -q : q, scale };
}

/**
 * Exact numeric order of two canonical decimal strings (`1.10` equals `1.1`).
 * `NaN` when either is not a decimal.
 */
export function decimalCompare(a: string, b: string): number {
  const x = parseDecimal(a);
  const y = parseDecimal(b);
  return x === null || y === null ? Number.NaN : compare(x, y);
}

/** Scale of `spreadBps` and of `change24h.percent`: 4 decimal places, half to even. */
export const RATIO_SCALE = 4;

/**
 * `spread = ask - bid` (exact, at `max(scale(bid), scale(ask))`) and
 * `spreadBps = (ask - bid) / price × 10 000` (half to even at 4 places;
 * `null` unless `price > 0`). Both `null` without a bid and ask.
 */
export function spreadOf(
  price: string,
  bid: string | null,
  ask: string | null,
): { spread: string | null; spreadBps: string | null } {
  const [p, b, a] = [
    parseDecimal(price),
    bid === null ? null : parseDecimal(bid),
    ask === null ? null : parseDecimal(ask),
  ];
  if (p === null || b === null || a === null) return { spread: null, spreadBps: null };
  const spread = subtract(a, b);
  const bps = p.mantissa > 0n ? divide(spread, p, RATIO_SCALE, 10_000n) : null;
  return { spread: formatDecimal(spread), spreadBps: bps === null ? null : formatDecimal(bps) };
}

/**
 * `absolute = current - baseline` (exact, at the larger scale) and
 * `percent = (current - baseline) / baseline × 100` (half to even at 4
 * places). `null` when the baseline is zero or either is not a decimal.
 */
export function changeOf(
  current: string,
  baseline: string,
): { absolute: string; percent: string } | null {
  const [c, b] = [parseDecimal(current), parseDecimal(baseline)];
  if (c === null || b === null || b.mantissa === 0n) return null;
  const absolute = subtract(c, b);
  const percent = divide(absolute, b, RATIO_SCALE, 100n);
  if (percent === null) return null;
  return { absolute: formatDecimal(absolute), percent: formatDecimal(percent) };
}

/** Significant digits as stated, trailing zeros included (`2100.00` → 6). */
function significantDigits(d: Decimal): number {
  const m = d.mantissa < 0n ? -d.mantissa : d.mantissa;
  return m === 0n ? 1 : m.toString().length;
}

/**
 * `numerator / denominator`, rounded half to even to the fewer significant
 * digits of the two, from the exact rational (one rounding): a ratio cannot
 * be more precise than its inputs. Mirrors `undrly_core::quote::cross_rate`
 * (V1.9 derived FX). `null` unless both are positive decimals.
 */
export function crossRate(numerator: string, denominator: string): string | null {
  const [n, d] = [parseDecimal(numerator), parseDecimal(denominator)];
  if (n === null || d === null || n.mantissa <= 0n || d.mantissa <= 0n) return null;
  const digits = Math.min(significantDigits(n), significantDigits(d));
  // Integer digits of the quotient: the largest e with 10^(e-1) <= n/d.
  const num = n.mantissa * 10n ** BigInt(d.scale);
  const den = d.mantissa * 10n ** BigInt(n.scale);
  let e = 0;
  while (num >= den * 10n ** BigInt(e)) e++;
  if (e === 0) {
    // Below 1: count the leading zeros after the point.
    let z = 0;
    while (num * 10n ** BigInt(z + 1) < den) z++;
    e = -z;
  }
  const q = divide(n, d, Math.max(0, digits - e));
  return q === null ? null : formatDecimal(q);
}

/**
 * `first × second`, rounded half to even to the fewer significant digits of
 * the two (one rounding of the exact product). Mirrors
 * `undrly_core::quote::convert_rate` (V1.10, `convert-via-stablecoin-v1`).
 * `null` unless both are positive decimals.
 */
export function convertRate(first: string, second: string): string | null {
  const [a, b] = [parseDecimal(first), parseDecimal(second)];
  if (a === null || b === null || a.mantissa <= 0n || b.mantissa <= 0n) return null;
  const digits = Math.min(significantDigits(a), significantDigits(b));
  const mantissa = a.mantissa * b.mantissa;
  const scale = a.scale + b.scale;
  const drop = mantissa.toString().length - digits;
  if (drop <= 0) return formatDecimal({ mantissa, scale });
  const unit = 10n ** BigInt(drop);
  let q = mantissa / unit;
  const twice = (mantissa % unit) * 2n;
  if (twice > unit || (twice === unit && q % 2n === 1n)) q += 1n;
  const kept = scale - drop;
  return formatDecimal(
    kept >= 0 ? { mantissa: q, scale: kept } : { mantissa: q * 10n ** BigInt(-kept), scale: 0 },
  );
}

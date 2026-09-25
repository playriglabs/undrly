/**
 * Query syntax shared by resolve, quote and quotes. Pure: no database.
 *
 * - `undrly:<category>:<id>`                  a canonical id
 * - `isin:US67066G1040`, `cik:1045810`, …     `scheme:value` identifiers
 * - `NASDAQ:NVDA`, `KRAKEN:XXBTZUSD`          `VENUE:SYMBOL` (or `SOURCE:SYMBOL`)
 * - `EUR/USD`, `BTC/USD`, `XAU/USD`           `BASE/QUOTE` pairs
 * - anything else                              an exact alias (`BTC`, `Gold`)
 */
import { v1 } from "@undrly/contracts";

export const IDENTIFIER_SCHEMES = ["isin", "figi", "lei", "cik", "mic", "iso4217"] as const;
export type IdentifierScheme = (typeof IDENTIFIER_SCHEMES)[number];

export type ParsedQuery =
  | { kind: "canonical_id"; category: v1.Category; uuid: string }
  | { kind: "identifier"; scheme: IdentifierScheme; value: string }
  | { kind: "venue_symbol"; venue: string; symbol: string }
  | { kind: "pair"; base: string; quote: string }
  | { kind: "alias"; text: string };

/** Applies the namespace's normalization, as `undrly_core` does. */
export function normalizeIdentifier(scheme: IdentifierScheme, raw: string): string {
  const value = raw.trim().toUpperCase();
  if (scheme === "cik" && /^[0-9]{1,10}$/.test(value)) return value.padStart(10, "0");
  return value;
}

export function parseQuery(raw: string): ParsedQuery | null {
  const q = raw.trim();
  if (q === "" || q.length > 256 || /[\p{Cc}]/u.test(q)) return null;

  const category = v1.canonicalIdCategory(q);
  const uuid = v1.canonicalIdUuid(q);
  if (category !== null && uuid !== null) return { kind: "canonical_id", category, uuid };

  const colon = /^([A-Za-z0-9-]+):(\S+)$/.exec(q);
  if (colon !== null) {
    const [, prefix = "", rest = ""] = colon;
    const scheme = prefix.toLowerCase();
    if ((IDENTIFIER_SCHEMES as readonly string[]).includes(scheme)) {
      const s = scheme as IdentifierScheme;
      return { kind: "identifier", scheme: s, value: normalizeIdentifier(s, rest) };
    }
    return { kind: "venue_symbol", venue: prefix, symbol: rest };
  }

  const pair = /^([^/\s]+)\/([^/\s]+)$/.exec(q);
  if (pair !== null) {
    const [, base = "", quote = ""] = pair;
    return { kind: "pair", base, quote };
  }
  return { kind: "alias", text: q };
}

/**
 * Class-share punctuation, one explicit rule for queries only: a symbol of
 * 1–5 letters, `-` and one letter is also looked up with `.` (`BRK-B` →
 * `BRK.B`), the spelling equity listings keep. `null` for anything else.
 */
export function classShareSymbol(symbol: string): string | null {
  const match = /^([A-Za-z]{1,5})-([A-Za-z])$/.exec(symbol);
  if (match === null) return null;
  const [, root = "", share = ""] = match;
  return `${root}.${share}`;
}

/** Converts PostgreSQL `to_char(… 'YYYY-MM-DD"T"HH24:MI:SS.US')` text to
 * canonical `undrly_core::Timestamp` text (fraction omitted, 3 or 6 digits). */
export function canonicalTimestamp(text: string): string {
  const match = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})\.(\d{6})$/.exec(text);
  if (match === null) throw new Error(`unexpected timestamp text ${text}`);
  const [, seconds = "", micros = ""] = match;
  if (micros === "000000") return `${seconds}Z`;
  if (micros.endsWith("000")) return `${seconds}.${micros.slice(0, 3)}Z`;
  return `${seconds}.${micros}Z`;
}

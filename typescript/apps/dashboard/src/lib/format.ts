import type { v1 } from "@undrly/contracts";

/** Groups the integer digits of an exact decimal string; never rounds or converts to a float. */
export function formatDecimal(value: string): string {
  const negative = value.startsWith("-");
  const [int = "0", frac] = (negative ? value.slice(1) : value).split(".");
  const grouped = int.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  return `${negative ? "-" : ""}${grouped}${frac === undefined ? "" : `.${frac}`}`;
}

/** A percent string from the API (`"0.5990"`) for display, with sign. Display only. */
export function formatPercent(value: string): string {
  const n = Number(value);
  return `${n > 0 ? "+" : ""}${n.toFixed(2)}%`;
}

export function direction(value: string | null | undefined): "up" | "down" | "flat" {
  if (!value) return "flat";
  const n = Number(value);
  return n > 0 ? "up" : n < 0 ? "down" : "flat";
}

/**
 * A market's name for display, without the venue suffix stored on perpetuals
 * ("BTC Perpetual (Hyperliquid)" → "BTC Perpetual"). The API name is unchanged.
 */
export function displayName(name: string): string {
  return name.replace(/\s*\(Hyperliquid\)$/, "");
}

export function unitCode(unit: v1.PriceUnitV1): string {
  return unit.code ?? "asset";
}

export const CLASS_LABEL: Record<v1.InstrumentClass, string> = {
  crypto_asset: "Crypto",
  equity: "Equities",
  perpetual_future: "Perpetuals",
  fx: "Forex",
  commodity: "Commodities",
  tokenized_security: "Tokenized",
};

/** Tab order of the asset classes. */
export const CLASS_ORDER: v1.InstrumentClass[] = [
  "crypto_asset",
  "equity",
  "perpetual_future",
  "fx",
  "commodity",
  "tokenized_security",
];

export function subjectClass(subject: v1.PriceSubjectV1): string {
  return subject.kind === "instrument" ? CLASS_LABEL[subject.class] : "Currency";
}

export const STATUS_LABEL: Record<NonNullable<v1.MarketV1["marketStatus"]>, string> = {
  continuous: "24/7",
  open: "Open",
  pre_market: "Pre-market",
  after_hours: "After hours",
  closed: "Closed",
  unknown: "Unknown",
};

export function formatAge(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.round(m / 60);
  return h < 48 ? `${h}h` : `${Math.round(h / 24)}d`;
}

export function formatTimestamp(iso: string): string {
  return new Date(iso).toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "UTC",
    timeZoneName: "short",
  });
}

/**
 * What a market's change is measured against, when it is not the last 24
 * hours: `1d` / `1m` for a daily or monthly publication (by the gap between
 * the two values), `session` for an exchange session. `null` for 24 hours.
 */
export function changeBasis(stats: v1.MarketStatisticsV1 | null | undefined): string | null {
  if (!stats) return null;
  if (stats.window === "session") return "session";
  if (stats.window !== "previous_publication") return null;
  const days = (Date.parse(stats.to) - Date.parse(stats.from)) / 86_400_000;
  return days >= 25 ? "1m" : days >= 5 ? "1w" : "1d";
}

/** The stat card label for a market's change. */
export function changeLabel(stats: v1.MarketStatisticsV1 | null | undefined): string {
  if (stats?.window === "session") return "Session change";
  if (stats?.window === "previous_publication") return "Since previous publication";
  return "24h change";
}

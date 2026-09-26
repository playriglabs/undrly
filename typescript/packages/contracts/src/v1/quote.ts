import { z } from "zod";
import { decimalCompare, spreadOf } from "./decimal.ts";
import { OBSERVATION_BASES, PriceUnitV1 } from "./market-observation.ts";
import {
  CurrencyCode,
  CurrencyId,
  DecimalString,
  InstrumentId,
  SourceId,
  TimestampString,
  VenueId,
} from "./primitives.ts";

/** Mirrors `undrly_core::InstrumentClass`. */
export const INSTRUMENT_CLASSES = [
  "equity",
  "crypto_asset",
  "commodity",
  "perpetual_future",
  "fx",
] as const;
export type InstrumentClass = (typeof INSTRUMENT_CLASSES)[number];

/** Mirrors `undrly_core::PriceType`. */
export const PRICE_TYPES = ["last", "mid", "mark", "reference", "average"] as const;
export type PriceType = (typeof PRICE_TYPES)[number];

/** Mirrors `undrly_core::AggregationMethod`. */
export const AGGREGATION_METHODS = ["latest-observation-v1", "mean-venue-mid-v1"] as const;

/** Mirrors `undrly_core::UnitOfMeasure`. */
export const UNITS_OF_MEASURE = [
  "troy_ounce",
  "barrel",
  "mmbtu",
  "metric_ton",
  "kilogram",
] as const;
export type UnitOfMeasure = (typeof UNITS_OF_MEASURE)[number];

/** A currency named by its canonical id and ISO 4217 code. */
export const CurrencyRefV1 = z.strictObject({ id: CurrencyId, code: CurrencyCode });
export type CurrencyRefV1 = z.infer<typeof CurrencyRefV1>;

/**
 * What is priced: one unit of an instrument (a share, one BTC, one troy
 * ounce, one perpetual contract, one unit of an FX market's base currency)
 * or of a fiat currency. An FX market (`class: "fx"`, e.g. `EUR/USD`) states
 * its base and quote currency; its price is in the quote currency.
 */
export const PriceSubjectV1 = z.discriminatedUnion("kind", [
  z.strictObject({
    id: InstrumentId,
    kind: z.literal("instrument"),
    class: z.enum(INSTRUMENT_CLASSES),
    name: z.string().min(1),
    /** Units of the underlying per contract (`1000` for `kPEPE`); present only when not 1. */
    contractMultiplier: DecimalString.optional(),
    /** The physical unit a commodity is priced per; present only when stated. */
    unitOfMeasure: z.enum(UNITS_OF_MEASURE).optional(),
    /** FX markets only: the currency one unit of which is priced (`EUR` in `EUR/USD`). */
    baseCurrency: CurrencyRefV1.optional(),
    /** FX markets only: the currency the price is in (`USD` in `EUR/USD`); equals `unit`. */
    quoteCurrency: CurrencyRefV1.optional(),
  }),
  z.strictObject({
    id: CurrencyId,
    kind: z.literal("currency"),
    code: CurrencyCode,
    name: z.string().min(1),
  }),
]);
export type PriceSubjectV1 = z.infer<typeof PriceSubjectV1>;

export const VenueRefV1 = z.strictObject({ id: VenueId, name: z.string().min(1) });

/** A storage id rendered as decimal text (opaque; never a number in JSON). */
const StoredId = z.string().regex(/^[1-9][0-9]*$/, "invalid stored id");

const priceFields = {
  schemaVersion: z.literal(1),
  subject: PriceSubjectV1,
  unit: PriceUnitV1,
  priceType: z.enum(PRICE_TYPES),
  price: DecimalString,
  bid: DecimalString.nullable(),
  ask: DecimalString.nullable(),
  /**
   * `venue`: one venue's market, named in `venue` (e.g. an IEX venue quote).
   * `aggregated`: computed across sources; `venue` is `null`.
   */
  basis: z.enum(OBSERVATION_BASES),
  venue: VenueRefV1.nullable(),
  /** The source's time for the price; `null` when the source states none. */
  observedAt: TimestampString.nullable(),
  /** When Undrly received the source response (the latest one, for an aggregate). */
  receivedAt: TimestampString,
};

type PriceShape = {
  subject: PriceSubjectV1;
  unit: { id: string };
  basis: string;
  venue: unknown;
  bid: unknown;
  ask: unknown;
};

function consistent<T extends PriceShape>(schema: z.ZodType<T>) {
  return schema
    .refine((q) => (q.basis === "venue") === (q.venue !== null), {
      message: "a venue basis names its venue; aggregated/derived never do",
    })
    .refine((q) => (q.bid === null) === (q.ask === null), {
      message: "bid and ask come together",
    })
    .refine((q) => q.unit.id !== q.subject.id, {
      message: "a subject cannot be priced in units of itself",
    })
    .superRefine((q, ctx) => {
      for (const issue of fxIssues(q)) ctx.addIssue({ code: "custom", ...issue });
    });
}

/**
 * One source's observation (one feed), as stored. Served by `/v1/quotes`.
 * `freshness` is computed at read time with the pair's aggregation window;
 * `sourceRecord` is the raw upstream response it was normalized from.
 */
export const ObservationV1 = consistent(
  z.strictObject({
    ...priceFields,
    observationId: StoredId,
    source: z.strictObject({ id: SourceId }),
    sourceRecord: z.strictObject({ id: StoredId, key: z.string().min(1) }),
    freshness: z.enum(["fresh", "stale"]),
  }),
);
export type ObservationV1 = z.infer<typeof ObservationV1>;

const QuoteAggregationV1 = z.strictObject({
  method: z.enum(AGGREGATION_METHODS),
  eligibleObservations: z.number().int().min(1),
  computedAt: TimestampString,
});

/** Fields every canonical quote has, whatever its basis. */
const canonicalFields = {
  schemaVersion: z.literal(1),
  subject: PriceSubjectV1,
  unit: PriceUnitV1,
  priceType: z.enum(PRICE_TYPES),
  price: DecimalString,
  bid: DecimalString.nullable(),
  ask: DecimalString.nullable(),
  /** `ask - bid`, exact; `null` without a bid and ask. */
  spread: DecimalString.nullable(),
  /** `(ask - bid) / price × 10 000`, half to even at 4 places; `null` without a bid and ask. */
  spreadBps: DecimalString.nullable(),
};

/** Fields after `basis` (and, for a venue quote, its venue/source/observedAt). */
const timingFields = {
  /** When Undrly received the source response (the latest one, for an aggregate). */
  receivedAt: TimestampString,
  /** The time the price is current as of (oldest input, for an aggregate). */
  asOf: TimestampString,
  /**
   * Wall-clock milliseconds from `asOf` to when this response was made,
   * never negative. Computed per response, never stored. Not a freshness
   * verdict: see `freshness`.
   */
  ageMs: z.number().int().min(0),
  /** Policy verdict at read time, against the feed's (or method's) cadence. */
  freshness: z.enum(["fresh", "stale"]),
  aggregation: QuoteAggregationV1,
};

/**
 * One venue's market (e.g. IEX for NVDA, Kraken for EUR/USD). Names the
 * venue, never the data provider that delivered it.
 */
export const VenueQuoteV1 = z.strictObject({
  ...canonicalFields,
  basis: z.literal("venue"),
  venue: VenueRefV1,
  /** The source's time for the price; `null` when the source states none. */
  observedAt: TimestampString.nullable(),
  ...timingFields,
});

/**
 * Computed across sources or published without a venue (a mean of venue
 * mids, a reference or average series). Attributed to no venue or source.
 */
export const AggregatedQuoteV1 = z.strictObject({
  ...canonicalFields,
  basis: z.literal("aggregated"),
  ...timingFields,
});

/** Derived from other prices (declared by curated feeds); no venue or source. */
export const DerivedQuoteV1 = z.strictObject({
  ...canonicalFields,
  basis: z.literal("derived"),
  ...timingFields,
});

type Issue = { message: string; path?: (string | number)[] };

/** An FX market states both currencies, differing, and is priced in its quote. */
function fxIssues(q: { subject: PriceSubjectV1; unit: { id: string } }): Issue[] {
  const s = q.subject;
  if (s.kind !== "instrument") return [];
  const stated = s.baseCurrency !== undefined || s.quoteCurrency !== undefined;
  if (s.class !== "fx") {
    return stated ? [{ message: "only FX markets state base and quote currencies" }] : [];
  }
  if (s.baseCurrency === undefined || s.quoteCurrency === undefined) {
    return [{ message: "an FX market states its base and quote currency" }];
  }
  const issues: Issue[] = [];
  if (s.baseCurrency.id === s.quoteCurrency.id) {
    issues.push({ message: "an FX market's base and quote currencies differ" });
  }
  if (s.quoteCurrency.id !== q.unit.id) {
    issues.push({ message: "an FX market is priced in its quote currency" });
  }
  return issues;
}

function quoteIssues(q: {
  subject: PriceSubjectV1;
  unit: { id: string };
  basis: string;
  priceType: string;
  price: string;
  bid: string | null;
  ask: string | null;
  spread: string | null;
  spreadBps: string | null;
  asOf: string;
  aggregation: { method: string; eligibleObservations: number };
}): Issue[] {
  const issues: Issue[] = fxIssues(q);
  if (q.unit.id === q.subject.id) {
    issues.push({ message: "a subject cannot be priced in units of itself" });
  }
  if ((q.bid === null) !== (q.ask === null)) issues.push({ message: "bid and ask come together" });
  const expected = spreadOf(q.price, q.bid, q.ask);
  if (q.spread !== expected.spread || q.spreadBps !== expected.spreadBps) {
    issues.push({ message: "spread is ask - bid and spreadBps is spread / price × 10 000" });
  }
  if (
    q.priceType === "mid" &&
    q.bid !== null &&
    q.ask !== null &&
    !(decimalCompare(q.bid, q.price) <= 0 && decimalCompare(q.price, q.ask) <= 0)
  ) {
    issues.push({ message: "a mid lies within its bid and ask" });
  }
  if (
    q.aggregation.method === "mean-venue-mid-v1" &&
    !(q.basis === "aggregated" && q.priceType === "mid" && q.bid !== null)
  ) {
    issues.push({
      message: "a mean of venue mids is an aggregated mid with its mean bid and ask",
    });
  }
  if (
    q.aggregation.method === "latest-observation-v1" &&
    q.aggregation.eligibleObservations !== 1
  ) {
    issues.push({ message: "latest-observation-v1 is its single input" });
  }
  return issues;
}

/**
 * The canonical quote of a subject in a unit. Served by `/v1/quote`: one
 * price, produced from source observations by a named aggregation method,
 * shaped by `basis`:
 *
 * - `venue`: one venue's market; names `venue`, with the source's
 *   `observedAt` (`null` when it states none). Its method is
 *   `latest-observation-v1`: the quote is that one observation.
 * - `aggregated` / `derived`: no `venue` or `observedAt` keys.
 *
 * No quote names its data provider (`source`); `/v1/quotes` does.
 *   `mean-venue-mid-v1`: the mean of fresh venue mids; `priceType` is `mid`,
 *   `asOf` is the oldest input's time, `bid` is the mean of the inputs' bids
 *   and `ask` the mean of their asks (same inputs, same scale as `price`;
 *   `bid <= price <= ask`). They are not a best bid/offer: no venue's best
 *   price is selected. With one input they are that input's own bid and ask.
 *
 * `aggregation` states only the method, how many observations were
 * eligible, and when it was computed. Which observations, venues, sources
 * and raw records were used is kept in storage (`canonical_quote_inputs`)
 * for audit, not served here.
 */
export const QuoteV1 = z
  .discriminatedUnion("basis", [VenueQuoteV1, AggregatedQuoteV1, DerivedQuoteV1])
  .superRefine((q, ctx) => {
    for (const issue of quoteIssues(q)) ctx.addIssue({ code: "custom", ...issue });
  });
export type QuoteV1 = z.infer<typeof QuoteV1>;
export type VenueQuoteV1 = z.infer<typeof VenueQuoteV1>;
export type AggregatedQuoteV1 = z.infer<typeof AggregatedQuoteV1>;
export type DerivedQuoteV1 = z.infer<typeof DerivedQuoteV1>;

export const ObservationsV1 = z.strictObject({
  schemaVersion: z.literal(1),
  query: z.string(),
  observations: z.array(ObservationV1),
});
export type ObservationsV1 = z.infer<typeof ObservationsV1>;

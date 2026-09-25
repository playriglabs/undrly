import { z } from "zod";
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
] as const;
export type InstrumentClass = (typeof INSTRUMENT_CLASSES)[number];

/** Mirrors `undrly_core::PriceType`. */
export const PRICE_TYPES = ["last", "mid", "mark", "reference"] as const;
export type PriceType = (typeof PRICE_TYPES)[number];

/** Mirrors `undrly_core::AggregationMethod`. */
export const AGGREGATION_METHODS = ["latest-observation-v1"] as const;

/**
 * What is priced: one unit of an instrument (a share, one BTC, one troy
 * ounce, one perpetual contract) or of a fiat currency (FX: 1 EUR).
 */
export const PriceSubjectV1 = z.discriminatedUnion("kind", [
  z.strictObject({
    id: InstrumentId,
    kind: z.literal("instrument"),
    class: z.enum(INSTRUMENT_CLASSES),
    name: z.string().min(1),
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

const priceFields = {
  schemaVersion: z.literal(1),
  subject: PriceSubjectV1,
  unit: PriceUnitV1,
  priceType: z.enum(PRICE_TYPES),
  price: DecimalString,
  bid: DecimalString.nullable(),
  ask: DecimalString.nullable(),
  /** `venue`: one venue's market, named in `venue` (e.g. an IEX venue quote). */
  basis: z.enum(OBSERVATION_BASES),
  venue: VenueRefV1.nullable(),
  /** The source's time for the price; `null` when the source states none. */
  observedAt: TimestampString.nullable(),
  /** When Undrly received the source response. */
  receivedAt: TimestampString,
  source: z.strictObject({ id: SourceId }),
};

type PriceShape = {
  subject: { id: string };
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
    });
}

/** One source's observation (one feed), as stored. Served by `/v1/quotes`. */
export const ObservationV1 = consistent(z.strictObject(priceFields));
export type ObservationV1 = z.infer<typeof ObservationV1>;

/**
 * The canonical quote of a subject in a unit. Served by `/v1/quote`: one
 * price, produced from source observations by a named aggregation method.
 * With one eligible observation, the canonical quote is that observation.
 */
export const QuoteV1 = consistent(
  z.strictObject({
    ...priceFields,
    /** `observedAt` when stated, else `receivedAt`. */
    asOf: TimestampString,
    /** Computed at read time from `asOf`. */
    freshness: z.enum(["fresh", "stale"]),
    aggregation: z.strictObject({
      method: z.enum(AGGREGATION_METHODS),
      eligibleObservations: z.number().int().min(1),
      computedAt: TimestampString,
    }),
  }),
);
export type QuoteV1 = z.infer<typeof QuoteV1>;

export const ObservationsV1 = z.strictObject({
  schemaVersion: z.literal(1),
  query: z.string(),
  observations: z.array(ObservationV1),
});
export type ObservationsV1 = z.infer<typeof ObservationsV1>;

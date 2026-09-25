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
export const AGGREGATION_METHODS = ["latest-observation-v1", "mean-venue-mid-v1"] as const;

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

/** One observation a canonical quote was computed from. */
export const AggregationInputV1 = z.strictObject({
  observationId: StoredId,
  sourceId: SourceId,
  venue: VenueRefV1.nullable(),
  /** The price this observation contributed (its mid, for `mean-venue-mid-v1`). */
  price: DecimalString,
  sourceRecordId: StoredId,
});

/**
 * The canonical quote of a subject in a unit. Served by `/v1/quote`: one
 * price, produced from source observations by a named aggregation method.
 *
 * - `latest-observation-v1`: the quote is its single input observation
 *   (that observation's basis, venue and source).
 * - `mean-venue-mid-v1`: the mean of fresh venue mids; `basis` is
 *   `aggregated`, `venue` and `source` are `null`, `priceType` is `mid`, no
 *   bid/ask, `observedAt` is `null`, and `asOf` is the oldest input's time.
 */
export const QuoteV1 = consistent(
  z.strictObject({
    ...priceFields,
    /** The single source, or `null` for a multi-source aggregate. */
    source: z.strictObject({ id: SourceId }).nullable(),
    /** The time the price is current as of (oldest input, for an aggregate). */
    asOf: TimestampString,
    /** Computed at read time from `asOf`. */
    freshness: z.enum(["fresh", "stale"]),
    aggregation: z.strictObject({
      method: z.enum(AGGREGATION_METHODS),
      eligibleObservations: z.number().int().min(1),
      computedAt: TimestampString,
      /** Exactly the observations used (provenance of this quote). */
      inputs: z.array(AggregationInputV1).min(1),
    }),
  }),
)
  .refine((q) => q.aggregation.inputs.length === q.aggregation.eligibleObservations, {
    message: "eligibleObservations counts the inputs",
  })
  .refine(
    (q) =>
      q.aggregation.method !== "mean-venue-mid-v1" ||
      (q.basis === "aggregated" &&
        q.venue === null &&
        q.source === null &&
        q.priceType === "mid" &&
        q.bid === null &&
        q.observedAt === null),
    { message: "a mean of venue mids is aggregated and attributed to no venue or source" },
  )
  .refine(
    (q) =>
      q.aggregation.method !== "latest-observation-v1" ||
      (q.aggregation.inputs.length === 1 && q.source !== null),
    { message: "latest-observation-v1 is its single input" },
  );
export type QuoteV1 = z.infer<typeof QuoteV1>;

export const ObservationsV1 = z.strictObject({
  schemaVersion: z.literal(1),
  query: z.string(),
  observations: z.array(ObservationV1),
});
export type ObservationsV1 = z.infer<typeof ObservationsV1>;

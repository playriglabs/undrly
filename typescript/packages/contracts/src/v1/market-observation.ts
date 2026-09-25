import { z } from "zod";
import {
  CurrencyCode,
  CurrencyId,
  DecimalString,
  InstrumentId,
  SourceId,
  TimestampString,
  VenueId,
} from "./primitives.ts";

/** Mirrors `undrly_core::ObservationBasis`. */
export const OBSERVATION_BASES = ["venue", "aggregated", "derived"] as const;
export type ObservationBasis = (typeof OBSERVATION_BASES)[number];

/** Mirrors `undrly_core::PriceUnit`. */
export const UNIT_KINDS = ["currency", "asset"] as const;
export type UnitKind = (typeof UNIT_KINDS)[number];

/**
 * The unit a price is expressed in. `id` is the canonical identity and is
 * authoritative. `code` is a display convenience (ISO 4217 for currencies, a
 * symbol such as `USDC` for assets) and is never identity. Crypto assets are
 * referenced by their canonical instrument id.
 */
export const PriceUnitV1 = z.discriminatedUnion("kind", [
  z.strictObject({ id: CurrencyId, kind: z.literal("currency"), code: CurrencyCode }),
  z.strictObject({
    id: InstrumentId,
    kind: z.literal("asset"),
    code: z.string().min(1).max(32).regex(/^\S+$/).nullable(),
  }),
]);
export type PriceUnitV1 = z.infer<typeof PriceUnitV1>;

// Key order matches the documented contract so re-encoded documents are identical.
function variant<Basis extends ObservationBasis, Venue extends z.ZodType>(
  basis: Basis,
  venueId: Venue,
) {
  return z.strictObject({
    schemaVersion: z.literal(1),
    instrumentId: InstrumentId,
    basis: z.literal(basis),
    venueId,
    price: DecimalString,
    unit: PriceUnitV1,
    observedAt: TimestampString,
    receivedAt: TimestampString,
    sourceId: SourceId,
  });
}

/**
 * @deprecated Superseded by `ObservationV1` / `QuoteV1` (./quote.ts), which
 * add FX subjects, price type, bid/ask and nullable source time. Kept for its
 * fixtures; no API route serves it.
 *
 * A normalized price observation. `venueId` is present exactly when
 * `basis` is `"venue"`; aggregated and derived values never name a venue.
 */
export const MarketObservationV1 = z
  .discriminatedUnion("basis", [
    variant("venue", VenueId),
    variant("aggregated", z.null()),
    variant("derived", z.null()),
  ])
  .refine((o) => o.unit.id !== o.instrumentId, {
    message: "an instrument cannot be priced in units of itself",
  });
export type MarketObservationV1 = z.infer<typeof MarketObservationV1>;

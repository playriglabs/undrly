import { z } from "zod";
import { NodeRefV1 } from "./discovery.ts";
import { SourceId, TimestampString } from "./primitives.ts";

/** Mirrors `undrly_core::UniverseKey`. */
export const UNIVERSE_KEYS = [
  "crypto-top100",
  "crypto-top250",
  "crypto-top500",
  "sp500",
  "sp400",
  "sp600",
  "nasdaq100",
  "hyperliquid-perps",
  "fx-major",
  "fx-southeast-asia",
  "fx-global",
] as const;
export type UniverseKey = (typeof UNIVERSE_KEYS)[number];

const StoredId = z.string().regex(/^[1-9][0-9]*$/, "invalid stored id");

/**
 * A universe's latest snapshot, summarized. Membership is snapshot
 * metadata: it never affects identity, resolution or quotes.
 */
export const UniverseSummaryV1 = z.strictObject({
  key: z.enum(UNIVERSE_KEYS),
  name: z.string().min(1),
  description: z.string().min(1),
  /** The upstream source that asserted the membership. */
  source: z.strictObject({ id: SourceId }),
  /** The source's own as-of time for the snapshot. */
  asOf: TimestampString,
  memberCount: z.number().int().min(0),
});
export type UniverseSummaryV1 = z.infer<typeof UniverseSummaryV1>;

/** `GET /v1/universes`: every universe that has a snapshot. */
export const UniversesV1 = z.strictObject({
  schemaVersion: z.literal(1),
  universes: z.array(UniverseSummaryV1),
});
export type UniversesV1 = z.infer<typeof UniversesV1>;

/**
 * `GET /v1/universes/{key}`: the latest snapshot's members, in the source's
 * order (rank when it has one, else symbol). No pagination.
 */
export const UniverseV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    ...UniverseSummaryV1.shape,
    /** The stored upstream record the membership comes from. */
    sourceRecord: z.strictObject({ id: StoredId, key: z.string().min(1) }),
    members: z.array(
      z.strictObject({
        node: NodeRefV1,
        /** Source rank (e.g. market-cap rank); metadata, not identity. */
        rank: z.number().int().min(1).nullable(),
        /** As the universe source spells it. */
        sourceSymbol: z.string().min(1).nullable(),
      }),
    ),
  })
  .refine((u) => u.memberCount === u.members.length, {
    message: "memberCount counts the members",
  });
export type UniverseV1 = z.infer<typeof UniverseV1>;

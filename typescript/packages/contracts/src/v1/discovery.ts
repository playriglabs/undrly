import { z } from "zod";
import { PriceUnitV1 } from "./market-observation.ts";
import { CATEGORIES, CanonicalId, SourceId, TimestampString } from "./primitives.ts";
import { INSTRUMENT_CLASSES, PriceSubjectV1 } from "./quote.ts";
import { RELATIONSHIP_TYPES } from "./relationship.ts";

/** A node as the API names it. `class` is set for instruments only. */
export const NodeRefV1 = z.strictObject({
  id: CanonicalId,
  kind: z.enum(CATEGORIES),
  name: z.string().min(1),
  class: z.enum(INSTRUMENT_CLASSES).nullable(),
});
export type NodeRefV1 = z.infer<typeof NodeRefV1>;

/** How well a search result matched. Ordered best first. */
export const SEARCH_RANKS = ["exact_symbol", "exact_name", "prefix", "substring"] as const;

/** Discovery only: results are never used as identity. */
export const SearchResultV1 = z.strictObject({
  schemaVersion: z.literal(1),
  query: z.string(),
  results: z.array(
    z.strictObject({
      node: NodeRefV1,
      matched: z.string().min(1),
      rank: z.enum(SEARCH_RANKS),
    }),
  ),
});
export type SearchResultV1 = z.infer<typeof SearchResultV1>;

/** How `resolve` interpreted the query. */
export const RESOLVE_METHODS = [
  "canonical_id",
  "identifier",
  "venue_symbol",
  "feed_symbol",
  "pair",
  "alias",
] as const;

export const ResolutionV1 = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("node"),
    node: NodeRefV1,
  }),
  z.strictObject({
    kind: z.literal("pair"),
    subject: PriceSubjectV1,
    unit: PriceUnitV1,
  }),
]);
export type ResolutionV1 = z.infer<typeof ResolutionV1>;

/**
 * Deterministic resolution. `resolved` has exactly one `match`; `ambiguous`
 * lists every candidate and picks none; `not_found` has neither.
 */
export const ResolveResultV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    query: z.string(),
    status: z.enum(["resolved", "ambiguous", "not_found"]),
    method: z.enum(RESOLVE_METHODS).nullable(),
    match: ResolutionV1.nullable(),
    candidates: z.array(ResolutionV1),
  })
  .refine((r) => (r.status === "resolved") === (r.match !== null), {
    message: "exactly a resolved result has a match",
  })
  .refine((r) => r.status !== "ambiguous" || r.candidates.length > 1, {
    message: "an ambiguous result lists its candidates",
  });
export type ResolveResultV1 = z.infer<typeof ResolveResultV1>;

/** One hop of the graph around an instrument, in both directions. */
export const GraphV1 = z.strictObject({
  schemaVersion: z.literal(1),
  root: NodeRefV1,
  edges: z.array(
    z.strictObject({
      subject: NodeRefV1,
      relationshipType: z.enum(RELATIONSHIP_TYPES),
      object: NodeRefV1,
      provenance: z.strictObject({ sourceId: SourceId, receivedAt: TimestampString }),
    }),
  ),
  listings: z.array(
    z.strictObject({
      id: CanonicalId,
      venue: NodeRefV1,
      symbols: z.array(z.string().min(1)),
    }),
  ),
});
export type GraphV1 = z.infer<typeof GraphV1>;

/** Structured API error. */
export const ErrorV1 = z.strictObject({
  schemaVersion: z.literal(1),
  error: z.strictObject({
    code: z.enum(["not_found", "ambiguous", "no_quote", "bad_request"]),
    message: z.string(),
    candidates: z.array(ResolutionV1).optional(),
  }),
});
export type ErrorV1 = z.infer<typeof ErrorV1>;

/** `GET /`: what this service is, its endpoints, and example requests. */
export const ServiceIndexV1 = z.strictObject({
  schemaVersion: z.literal(1),
  name: z.literal("Undrly"),
  description: z.string().min(1),
  endpoints: z.array(z.strictObject({ path: z.string().min(1), returns: z.string().min(1) })),
  examples: z.array(z.string().startsWith("/")),
  dataUse: z.string().min(1),
});
export type ServiceIndexV1 = z.infer<typeof ServiceIndexV1>;

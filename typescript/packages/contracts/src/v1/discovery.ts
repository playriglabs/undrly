import { z } from "zod";
import { PriceUnitV1 } from "./market-observation.ts";
import { CATEGORIES, CanonicalId, DeploymentId, SourceId, TimestampString } from "./primitives.ts";
import { INSTRUMENT_CLASSES, PriceSubjectV1 } from "./quote.ts";
import { ProvenanceV1, RELATIONSHIP_TYPES } from "./relationship.ts";

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

/**
 * A deployment of an asset on a chain (V1.4): the deployment node, its chain
 * (the `DEPLOYED_ON` projection) and its CAIP-19 asset type
 * (`<chain>/<asset namespace>:<asset reference>`). The CAIP-19 value is an
 * external identifier, never canonical identity.
 */
export const DeploymentRefV1 = z.strictObject({
  id: DeploymentId,
  chain: NodeRefV1,
  caip19: z.string().min(1),
});
export type DeploymentRefV1 = z.infer<typeof DeploymentRefV1>;

/**
 * One hop of the graph around an instrument, in both directions.
 *
 * V1.4 additions, each omitted (never `null` or empty) when there is nothing
 * to show, so earlier documents are unchanged:
 * - `deployments`: the deployments that `REPRESENTS` the root (the edges
 *   themselves are in `edges`), with chain and CAIP-19 id.
 * - `markets`: the units the root is priced in by a declared quote feed, and
 *   the venues of those feeds. A market is (subject, unit), not a node:
 *   Bitcoin in USD and Bitcoin in USDC are different markets because USD (a
 *   currency) and USDC (an asset) are different units.
 */
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
  deployments: z.array(DeploymentRefV1).min(1).optional(),
  markets: z
    .array(z.strictObject({ unit: PriceUnitV1, venues: z.array(NodeRefV1) }))
    .min(1)
    .optional(),
});
export type GraphV1 = z.infer<typeof GraphV1>;

/**
 * Why one candidate matched (V1.4 `/v1/explain`). Each entry is one rule of
 * the resolver that fired, with the stored value it matched:
 *
 * | rule | namespace | value |
 * | --- | --- | --- |
 * | `canonical_id` | `null` | the id |
 * | `identifier` | `isin`, `figi`, `lei`, `cik`, `mic`, `iso4217`, `caip2`, `caip19` | the stored identifier |
 * | `alias` | `symbol` or `name` | the stored alias (matched case-insensitively) |
 * | `class_share_symbol` | `symbol` | the equity symbol with `.` (`BRK-B` → `BRK.B`) |
 * | `listing_symbol` | `null` | the venue's symbol (`venue` set) |
 * | `feed_symbol` | `null` | a quote feed's symbol (`venue` or `source` set) |
 * | `fx_pair` | `null` | the FX market's base and quote currencies both matched |
 *
 * `side` is `base` or `quote` for the two halves of a `BASE/QUOTE` query.
 * There are no scores or confidences: a rule matched exactly or not at all.
 */
export const MATCH_RULES = [
  "canonical_id",
  "identifier",
  "alias",
  "class_share_symbol",
  "listing_symbol",
  "feed_symbol",
  "fx_pair",
] as const;

export const MatchV1 = z.strictObject({
  rule: z.enum(MATCH_RULES),
  side: z.enum(["base", "quote"]).nullable(),
  namespace: z.string().min(1).nullable(),
  value: z.string().min(1),
  venue: NodeRefV1.nullable(),
  source: z.strictObject({ id: SourceId }).nullable(),
});
export type MatchV1 = z.infer<typeof MatchV1>;

/** Relationship labels in explanations: stored types plus the projections. */
export const EXPLAIN_RELATIONSHIP_TYPES = [
  ...RELATIONSHIP_TYPES,
  "LISTED_ON",
  "DEPLOYED_ON",
] as const;

/**
 * `GET /v1/explain?q=` (V1.4): what `resolve` concluded and why. Uses the
 * same resolver: `status` and `method` always equal `/v1/resolve`'s, and
 * `candidates` are its match (resolved) or candidates (ambiguous), in the
 * same order. An ambiguous query is explained, never decided.
 *
 * Per candidate (for a pair, about its subject):
 * - `matches`: the rules that selected it;
 * - `identifiers`: its current external identifiers (a chain's CAIP-2 id, a
 *   deployment's CAIP-19 id);
 * - `relationships`: its outgoing edges in canonical direction with their
 *   provenance, plus the projections `LISTED_ON` (an instrument's listings'
 *   venues) and `DEPLOYED_ON` (a deployment's chain), marked `projected`;
 * - `quoted`: for a pair, whether a quote feed declares it; `null` for nodes.
 *
 * `quotedPairsOnly` is true when several pair combinations matched and only
 * the quoted ones were kept (the V1.1 pair rule).
 */
export const ExplainV1 = z
  .strictObject({
    schemaVersion: z.literal(1),
    query: z.string(),
    parsedAs: z.enum(["canonical_id", "identifier", "venue_symbol", "pair", "alias"]),
    status: z.enum(["resolved", "ambiguous", "not_found"]),
    method: z.enum(RESOLVE_METHODS).nullable(),
    quotedPairsOnly: z.boolean(),
    candidates: z.array(
      z.strictObject({
        resolution: ResolutionV1,
        matches: z.array(MatchV1).min(1),
        identifiers: z.array(
          z.strictObject({ namespace: z.string().min(1), value: z.string().min(1) }),
        ),
        relationships: z.array(
          z.strictObject({
            relationshipType: z.enum(EXPLAIN_RELATIONSHIP_TYPES),
            object: NodeRefV1,
            projected: z.boolean(),
            provenance: ProvenanceV1,
          }),
        ),
        quoted: z.boolean().nullable(),
      }),
    ),
  })
  .refine((e) => (e.status === "not_found") === (e.candidates.length === 0), {
    message: "exactly a not_found explanation has no candidates",
  })
  .refine((e) => e.status !== "resolved" || e.candidates.length === 1, {
    message: "a resolved explanation has one candidate",
  });
export type ExplainV1 = z.infer<typeof ExplainV1>;

/** Structured API error. */
export const ErrorV1 = z.strictObject({
  schemaVersion: z.literal(1),
  error: z.strictObject({
    /** `no_data` (V1.3): the market exists but this endpoint has no data for it. */
    code: z.enum(["not_found", "ambiguous", "no_quote", "bad_request", "no_data"]),
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

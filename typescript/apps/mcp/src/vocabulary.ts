/**
 * `undrly://vocabulary`: the fixed terms Undrly's answers use, so an agent can
 * read them once instead of inferring them from tool results. Every list is
 * the contract's or the resolver's own constant; nothing here is new
 * semantics (docs/v1.8-mcp.md §9).
 */
import { IDENTIFIER_SCHEMES } from "@undrly/api/query";
import { v1 } from "@undrly/contracts";

const EXPLAIN_PROJECTIONS = v1.EXPLAIN_RELATIONSHIP_TYPES.filter(
  (t) => !(v1.RELATIONSHIP_TYPES as readonly string[]).includes(t),
);

export const VOCABULARY = {
  schemaVersion: 1,
  categories: {
    values: v1.CATEGORIES,
    note: "Every canonical id is undrly:<category>:<id>. A deployment (a token contract on a chain) is not the economic asset it REPRESENTS; a listing (an instrument on a venue) is not the instrument.",
  },
  instrumentClasses: v1.INSTRUMENT_CLASSES,
  relationships: {
    stored: v1.RELATIONSHIP_RULES.map(([type, subject, object]) => ({ type, subject, object })),
    projections: {
      values: EXPLAIN_PROJECTIONS,
      note: "Derived at read time and marked projected: LISTED_ON (an instrument's listings' venues), DEPLOYED_ON (a deployment's chain), TRACKED_BY (the inverse of TRACKS).",
    },
    note: "Edges are in canonical direction (subject → object) and each carries provenance: the source that asserted it and when Undrly received it. A relationship Undrly does not store is absent, never inferred.",
  },
  queries: {
    forms: [
      { form: "undrly:<category>:<id>", meaning: "a canonical id" },
      { form: "<scheme>:<value>", meaning: "an external identifier", schemes: IDENTIFIER_SCHEMES },
      { form: "caip2:<namespace>:<reference>", meaning: "a chain by its CAIP-2 id" },
      {
        form: "caip19:<chain>/<asset namespace>:<asset reference>",
        meaning:
          "a deployment by its CAIP-19 asset type; a bare contract address or mint is never identity",
      },
      { form: "VENUE:SYMBOL", meaning: "a venue listing or a feed symbol (NASDAQ:NVDA)" },
      {
        form: "BASE/QUOTE",
        meaning: "a market: subject priced in unit (EUR/USD, BTC/USD)",
      },
      { form: "anything else", meaning: "an exact symbol, name or identifier value" },
    ],
    resolveStatuses: ["resolved", "ambiguous", "not_found"],
    resolveMethods: v1.RESOLVE_METHODS,
    matchRules: v1.MATCH_RULES,
    searchRanks: v1.SEARCH_RANKS,
  },
  prices: {
    priceTypes: v1.PRICE_TYPES,
    bases: v1.OBSERVATION_BASES,
    unitKinds: {
      values: v1.UNIT_KINDS,
      note: "Every price has a unit. `currency` is a fiat currency (ISO 4217); `asset` is an instrument such as USDC or USDT, which is not the currency it tracks. The unit's id is authoritative; its code is display only.",
    },
    aggregationMethods: v1.AGGREGATION_METHODS,
    freshness: ["fresh", "stale"],
    candleIntervals: v1.CANDLE_INTERVALS,
    marketStatuses: v1.MARKET_STATUSES,
  },
  derivatives: {
    note: "A perpetual's price is in the unit it is DENOMINATED_IN; it is MARGINED_IN and SETTLES_IN possibly different assets. The three are never one 'currency'.",
  },
  invariants: [
    "A deployment is not the economic asset it REPRESENTS.",
    "A tracker or tokenized security is not the security it TRACKS.",
    "A stablecoin or payment asset is not the fiat currency it TRACKS.",
    "Price denomination, margin asset and settlement asset are separate relationships.",
    "A feed (source, venue, price type) is not an instrument.",
    "A listing is not an instrument.",
    "Equal symbols do not establish identity.",
    "A bare blockchain address does not establish a deployment; only CAIP-19 with its chain does.",
    "A missing issuer stays missing.",
    "A missing relationship stays missing.",
  ],
} as const;

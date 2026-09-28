/**
 * The MCP error model (docs/v1.8-mcp.md §7). Every tool failure is a tool
 * result with `isError: true` and `structuredContent.error`, never free
 * text alone, and never a database error or stack trace.
 */
import { v1 } from "@undrly/contracts";
import { z } from "zod";
import type { ApiResponse } from "./api.ts";

export const ERROR_CODES = [
  /** Nothing in Undrly matches (a well-formed query or id). */
  "not_found",
  /** Several candidates match; Undrly lists them and picks none. */
  "ambiguous",
  /** The arguments or the query are malformed or out of bounds. */
  "invalid_query",
  /** An identifier (canonical id, CAIP-2, CAIP-19, `unit`) is malformed. */
  "invalid_identifier",
  /** The object exists but Undrly has no data of this kind for it. */
  "no_data",
  /** The tool does not apply to this kind of object. */
  "unsupported",
  /** Undrly failed; details stay in the server's stderr. */
  "internal",
] as const;
export type ErrorCode = (typeof ERROR_CODES)[number];

export const ToolErrorV1 = z.strictObject({
  error: z.strictObject({
    code: z.enum(ERROR_CODES),
    message: z.string().min(1),
    /** `ambiguous`: the resolver's candidates, in its order. */
    candidates: z.array(v1.ResolutionV1).optional(),
    /** `ambiguous` market: the units the instrument is priced in (pass one as `unit`). */
    units: z.array(v1.PriceUnitV1).optional(),
  }),
});
export type ToolErrorV1 = z.infer<typeof ToolErrorV1>;

/** A failure that becomes a structured tool error. */
export class ToolFailure extends Error {
  constructor(
    readonly code: ErrorCode,
    message: string,
    readonly details: Omit<ToolErrorV1["error"], "code" | "message"> = {},
  ) {
    super(message);
  }
}

/** Query prefixes that name an identifier scheme (docs/v1.4 §resolve). */
const IDENTIFIER_PREFIX = /^(undrly|caip2|caip19|isin|figi|lei|cik|mic|iso4217):/i;

/** True when `query` is written as an identifier rather than a symbol or name. */
export const looksLikeIdentifier = (query: string) => IDENTIFIER_PREFIX.test(query.trim());

/**
 * Maps an API error body to a tool failure. The API's `bad_request` is
 * `invalid_identifier` when the query was written as an identifier,
 * `invalid_query` otherwise; `no_quote` and `no_data` are both `no_data`.
 */
export function apiFailure(res: ApiResponse, query: string | null): ToolFailure {
  const parsed = v1.ErrorV1.safeParse(res.body);
  if (!parsed.success) return new ToolFailure("internal", "unexpected response from Undrly");
  const { code, message, candidates } = parsed.data.error;
  switch (code) {
    case "bad_request":
      return new ToolFailure(
        query !== null && looksLikeIdentifier(query) ? "invalid_identifier" : "invalid_query",
        message,
      );
    case "not_found":
      return new ToolFailure("not_found", message);
    case "ambiguous":
      return new ToolFailure("ambiguous", message, candidates ? { candidates } : {});
    case "no_quote":
    case "no_data":
      return new ToolFailure("no_data", message);
  }
}

/**
 * External API contract (JSON), owned by the TypeScript API.
 *
 * Rust (`undrly-core`) owns canonical semantics; PostgreSQL is the internal
 * contract. This package defines the versioned public JSON shape and must
 * never redefine domain meaning. See `docs/contracts.md`.
 */
export * as v1 from "./v1/index.ts";

# Fixtures

Deterministic test data. Values (prices, symbols, relationships) are
illustrative test data, not market or reference data, and must never be loaded
into a database outside tests. Public identifiers in `identifiers.json` (for
example Apple's ISIN and LEI) are used only as check-digit test vectors.

| Path | Used by | Purpose |
| --- | --- | --- |
| `shared/primitives.json` | Rust core, TypeScript | Canonical ID text ↔ UUID pairs, decimals, timestamps, source ids. Both languages must agree on every case. |
| `shared/vocabulary.json` | Rust core, Rust store (vs. database), TypeScript | Categories, classes, relationship types and rules, observation bases, identifier namespaces. Every side asserts its tables equal this file. |
| `identifiers.json` | Rust core, Rust store (vs. database) | Namespace-specific identifier cases. The database checks shape only, so every Rust-valid value must pass the database checks. |
| `sources/reference-fixture/` | Rust ingest tests | Raw records for the deterministic fixture provider (see below). |
| `sources/sec-edgar/` | Rust provider and ingest tests | Captured SEC EDGAR responses, byte for byte (see below). |
| `api/v1/` | TypeScript | External JSON API contract v1: valid documents (accepted unchanged) and `invalid/` (rejected). |

Released API fixtures are append-only; a breaking change goes in `api/v2/`.

## `sources/reference-fixture/`

Raw payloads in Undrly's own fixture format (not any provider's API),
decoded by `undrly_provider::fixture`.

- `nvda.json`: NVIDIA / NVDA. Every identifier was checked against its
  registry on 2026-09-24:
  - LEI `549300S4KLFTLO7GSQ80`: GLEIF, legal name "NVIDIA CORPORATION",
    status ACTIVE.
  - ISIN `US67066G1040`: OpenFIGI mapping to NVIDIA CORP common stock.
  - Share-class FIGI `BBG001S5TZJ6` and Nasdaq exchange-level FIGI
    `BBG000BBK0R0` (exchange code `UW`): OpenFIGI.
  - MIC `XNAS` (ISO 10383) and `USD` (ISO 4217).

  The US composite FIGI (`BBG000BBJQV0`) is left out on purpose: it
  identifies neither the global security nor a single listing, and the
  schema has no category for it. The record gives no validity bounds, so
  every claim is unbounded ("bounds unknown").
- `synthetic-conflict.json`: **synthetic**. A made-up company whose ISIN
  (`ZZ…`) and LEI (`SYNTHETIC…`) have valid check digits but are not
  registered. It claims NVDA's Nasdaq symbol and exchange FIGI, and exists
  only to test conflict quarantine.

## `sources/sec-edgar/`

Real SEC EDGAR responses, stored exactly as served (no trailing newline, no
reformatting; `.gitattributes` marks them binary so no tool rewrites them).

- `CIK0001045810.json`: `GET https://data.sec.gov/submissions/CIK0001045810.json`
  (NVIDIA CORP), captured 2026-09-25T02:14:18Z, 159,785 bytes, SHA-256
  `e220b9939d680e34f3a59655b5134a8f1a44e6a4d9292426093c8a899d3020a8`. SEC
  EDGAR data is public; the file is a snapshot and goes stale as NVIDIA
  files. Tests derive malformed variants from it in code.

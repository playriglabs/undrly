# SEC EDGAR

The first real source. Its only job for now is filer identity for NVIDIA
Corporation: one authoritative upstream record goes through the full
pipeline, with provenance back to the exact raw record.

```text
data.sec.gov ─GET─▶ SecClient (undrly-provider, feature `http`)
  ─▶ RawRecord: exact body, URL as record key, receipt time
  ─▶ [one transaction]
       source_records  (raw bytes, stored first)
       decode    SecProvider  → CompanySubmissions (SEC's own model)
       normalize SecNormalizer → NormalizedEntityRecord
       resolve   entity by CIK (or LEI, if SEC reports one), else mint
       persist   entities row + identifiers rows, each naming the source record
  ─▶ read back / facts_from_source_record(record) → the facts it produced
```

Decode and normalize also run before the transaction opens, so a bad
response writes nothing.

## Endpoint

| | |
| --- | --- |
| URL | `https://data.sec.gov/submissions/CIK##########.json` (10-digit zero-padded CIK) |
| NVIDIA | `https://data.sec.gov/submissions/CIK0001045810.json` |
| Used fields | `cik`, `entityType`, `name`, `lei` |
| Ignored | `filings` (the most recent 1,000 filings plus pointers to older pages), `tickers`, `exchanges`, `sic`, `ein`, addresses, `formerNames`, ... They stay in the stored raw record. |

This is the smallest official endpoint that names a filer by CIK. Filing
history, XBRL/company facts, and bulk files are not used.

## Code

| Crate | Item | Role |
| --- | --- | --- |
| `undrly-core` | `Cik`, `Namespace::Cik`, `ExternalIdentifier::Cik` | CIK identifier namespace |
| `undrly-provider` | `sec::SecProvider`, `sec::CompanySubmissions`, `sec::SOURCE_ID` (`sec-edgar`) | pure decoding into SEC's model |
| `undrly-provider` (`http`) | `sec::http::{SecClient, SecUserAgent, FetchedRecord, FetchError}` | the only network code |
| `undrly-normalize` | `sec::SecNormalizer` (`EntityNormalizer`), `NormalizedEntityRecord` | SEC model → canonical claims |
| `undrly-ingest` | `ingest_entity`, `sec::{fetch_and_ingest_company, ingest_company, raw_record}` | raw-first ingestion |
| `undrly-store` | migration `0008_cik_identifiers.sql` | `cik` scheme for entities |

## CIK

- Canonical spelling: 10 ASCII digits, zero-padded (`0001045810`), the form
  EDGAR uses in URLs and submissions data. Stored as that text.
- `Cik::parse` accepts only that form. `Cik::normalize` trims, then
  left-pads 1–10 digits (`1045810` → `0001045810`). Signs, separators,
  a `CIK` prefix, and more than 10 digits are rejected.
- There is no check digit, so validation checks shape only. CIK `0` is never
  assigned and is rejected.
- A CIK identifies an SEC filer, modelled as an **entity**. The database
  rejects a CIK on any other category.
- It is a primary identifier for entities, next to LEI. It is never
  canonical identity: the entity id is a generated UUIDv7.

## Normalization

| SEC | Canonical |
| --- | --- |
| `cik` | `ExternalIdentifier::Cik` (namespace rules) |
| `lei` when not `null` | `ExternalIdentifier::Lei` (ISO 17442 check digits) |
| `entityType` = `operating` | `EntityKind::Company` |
| any other `entityType` | rejected (`Unsupported`), nothing written |
| `name` | entity display name, verbatim (must already be trimmed) |

## Facts produced for NVIDIA

From one submissions document, and only these:

1. `entities`: kind `company`, name `NVIDIA CORP`, a newly generated id if no
   entity has CIK `0001045810`, otherwise the existing entity (left
   unchanged).
2. `identifiers`: `cik 0001045810 → that entity`, unbounded validity.

Both rows name the `source_records` row holding the response bytes, and
carry source `sec-edgar` and the receipt time. No instrument, listing,
venue, symbol, currency, ISIN, FIGI, edge, or price comes from SEC, even
though the document lists `tickers: ["NVDA"]` and `exchanges: ["Nasdaq"]`.
SEC is not authoritative for listings.

## Raw record

- The body is stored byte for byte. `reqwest` is built without
  decompression features, so no `Accept-Encoding` is sent, and the stored
  bytes are the document as served. Tests assert this against a local
  server.
- Record key: the request URL. Receipt time: when the full body had been
  read. Source: `sec-edgar`.
- SEC's request id (`x-amzn-requestid`) and `Date` header come back in
  `FetchedRecord` but are **not persisted**: `source_records` has no column
  for them.
- SEC's document changes whenever NVIDIA files. Each distinct body is a new
  raw record. Identity facts stay credited to the record that first
  asserted them (corroboration is deferred).

## Request behaviour

`SecUserAgent` requires printable ASCII with a requester name and a contact
email, per SEC's fair-access policy. It comes from the operator
(`UNDRLY_SEC_USER_AGENT` in the live test) and is never hardcoded. Each call
makes one GET with a 30 s timeout and a 32 MiB body limit. There are no
redirects, no retries, and no concurrency. Any non-200 status is an error
and writes nothing.

## Source registration

Ingestion requires the `sec-edgar` source to be registered, which is an
administrative step. The live test registers it with redistribution
`unknown` (treated as restricted). EDGAR data is public, but deciding
redistribution terms is a human decision, not something code infers.

## Tests

Offline, in the normal suite:

- `undrly-core`: CIK parse and normalize rules, plus shared fixture cases.
- `undrly-provider`: decoding the captured NVIDIA document; malformed
  payloads; `User-Agent` rules.
- `undrly-normalize`: NVIDIA mapping; CIK and LEI rules; unsupported entity
  types.
- `undrly-store`: CIK shape and category constraints; schemes equal core.
- `undrly-ingest/tests/sec_edgar.rs` (database):
  - first ingestion mints the entity and only its CIK;
  - raw bytes are unchanged and every fact traces to the record;
  - replay is idempotent;
  - an existing entity with the same CIK is reused;
  - SEC and fixture records don't link without a shared identifier;
  - an SEC LEI links the CIK to the LEI-identified entity;
  - malformed, unsupported, or unexpected responses write nothing;
  - network and HTTP failures write nothing, checked against a local
    server (403, 404, 500, redirect, hang-up, truncated body, 200 with an
    error page, connection refused);
  - the request sends the declared `User-Agent` and no compression.

### Live NVIDIA check (optional, network)

```sh
UNDRLY_SEC_USER_AGENT="Your Name you@example.com" \
DATABASE_URL=postgres://undrly:<password>@127.0.0.1:5432/undrly \
  cargo test -p undrly-ingest --test sec_live -- --ignored --nocapture
```

This makes one request to data.sec.gov, ingests the response into a fresh
test database, checks raw-byte equality, provenance, and replay, and prints
the result.

## Differences from the fixture model

- **SEC reports no LEI for NVIDIA** (`"lei": null`). The fixture entity is
  identified by LEI `549300S4KLFTLO7GSQ80`, and the SEC entity by CIK. With
  no shared identifier, they resolve to **two entities**; nothing links them
  by name, and merging is deferred. If a later record carries both
  identifiers, it is rejected as ambiguous rather than merging them (tested).
- **Names differ:** SEC `NVIDIA CORP` (EDGAR conformed name), GLEIF
  `NVIDIA CORPORATION`. Names are display data and never identity.
- **SEC lists tickers and exchanges for the filer**, not for a security.
  There is no ISIN or FIGI, and no link between ticker and security, so no
  listing can be justified from SEC.
- **SEC provides an EIN** (`943177549`). No EIN namespace exists; it is not
  stored as a fact.
- **The SEC document is large and changes often** (1,000 recent filings).
  The fixture model assumed small, stable records. Each refetch after a new
  filing adds a raw record that is ~160 KB for NVIDIA. No retention policy
  exists yet.

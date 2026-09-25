# Canonical domain

Design of `rust/crates/undrly-core`. `AGENT.md` is the source of truth; this
file records how the code implements it.

## Canonical identifiers

```text
undrly:<category>:<id>
category = entity | instrument | listing | venue | currency
id       = UUIDv7 as 26 characters of lowercase Crockford base32 (first char 0–7)
```

- Generated (`EntityId::generate()` etc.) and never derived from names,
  tickers, symbols, ISINs, FIGIs, or any external identifier.
- The category is structural and immutable. Asset class (`InstrumentClass`)
  and entity kind are attributes, so correcting them never changes identity.
- Typed ids (`EntityId`, `InstrumentId`, `ListingId`, `VenueId`,
  `CurrencyId`) enforce the category; `CanonicalId` is used where any node may
  appear (relationship endpoints, identifier assignments).
- Parsing is strict: one spelling per id, RFC 9562 variant required.
  Text order equals UUID order within a category.

## Identifier layer

Each namespace has its own type and rules; there is no generic checksummed
identifier. `parse` accepts the canonical spelling; `normalize` trims and
uppercases (these namespaces are case-insensitive by specification).

| Type | Namespace | Rules | May identify |
| --- | --- | --- | --- |
| `Isin` | ISO 6166 | prefix + 9 alnum + Luhn check digit over letter-expanded digits | instrument |
| `Figi` | FIGI | no vowels, 3rd char `G`, reserved prefixes excluded, modified-Luhn check digit | instrument, listing |
| `Lei` | ISO 17442 | 18 alnum + ISO 7064 MOD 97-10 check digits | entity |
| `Mic` | ISO 10383 | 4 uppercase alnum | venue |
| `CurrencyCode` | ISO 4217 | 3 uppercase letters | currency |
| `Cik` | SEC Central Index Key | 10 digits zero-padded, not all zeros, no check digit; `normalize` trims and left-pads 1–10 digits | entity |
| `VenueSymbol` | venue-local | no whitespace/control chars; **no normalization**, case preserved | listing (via `ListingSymbol`, scoped to its venue) |

Registry membership (is this MIC assigned?) is reference data, not syntax.

`IdentifierAssignment` (identifier → node, `Validity`, `Provenance`) checks
that the namespace may identify the node's category. `conflicts_with` detects
the same identifier claimed for a different node over an overlapping period;
conflicts are quarantined by storage, never auto-resolved.

## Values

- **Decimals:** `rust_decimal::Decimal`; `f32`/`f64` are banned by clippy.
  Canonical text is `Decimal::to_string` (scale preserved). Raw onchain
  amounts will be atomic integers kept separate from these (`AGENT.md` §6).
- **Time:** `Timestamp` wraps `chrono::DateTime<Utc>` with microsecond
  precision (matches `timestamptz`) and years 0001–9999. `Validity` is a
  half-open, non-empty `[from, until)` with optional bounds.
- **Sources:** `SourceId` slug; `Redistribution::Unknown` is treated as
  restricted. `Provenance` = source + received time. The raw record a fact
  came from is a storage concern (`source_record_id`, see `persistence.md`);
  core has no database ids.

## Reference objects

`Entity { id, kind, name }`, `Instrument { id, class, name }`,
`Venue { id, name }`, `Currency { id, name }` (fiat; crypto assets are
instruments of class `crypto_asset`), `Listing { id, instrument_id, venue_id,
provenance }`. Names are mutable display data. Facts connecting objects are
relationships or identifier assignments, not fields.

## Relationships

`subject TYPE object` in one canonical direction: dependent → thing it depends
on. Inverses (`UNDERLYING_OF`) are derived at query time; `LISTED_ON` is
projected from listings. `RelationshipType::allowed_endpoints` is the storable
set and equals the database's `relationship_rules`; types with no rules yet
(`DERIVES_FROM`, `HOLDS`, `TRACKS`, `MEMBER_OF`, `TOKENIZES`, `REPRESENTS`,
`PRICED_BY`, `AVAILABLE_ON`, `RELATED_TO`) cannot be constructed until their
node categories exist. Provenance is mandatory. A relationship is a current
assertion by a source; validity periods are deferred.

## Market observations

`MarketObservation::new(instrument_id, basis, price, unit, source_id,
observed_at, received_at)`:

- `basis`: `Venue(VenueId) | Aggregated | Derived`, so a venue quote always
  names its venue and derived values never pose as venue quotes.
- `unit`: `PriceUnit::Currency(CurrencyId) | PriceUnit::Asset(InstrumentId)`.
  Codes/symbols are never unit identity. Self-denominated prices are rejected.
- `observed_at` (source time) and `received_at` (Undrly time) are separate;
  no ordering is enforced (clocks differ). Freshness is computed at read time.

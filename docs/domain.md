# Canonical domain

Design of `rust/crates/undrly-core`. `AGENT.md` is the source of truth; this
file records how the code implements it.

## Canonical identifiers

```text
undrly:<category>:<id>
category = entity | instrument | listing | venue | currency | chain | deployment
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
on. Inverses (`UNDERLYING_OF`, `TRACKED_BY`) are derived at query time; `LISTED_ON` is
projected from listings. `RelationshipType::allowed_endpoints` is the storable
set and equals the database's `relationship_rules`. `DERIVES_FROM` is
storable instrument → instrument (a perpetual → its underlying). V1.4 adds
`TOKENIZES` instrument → instrument and `REPRESENTS` deployment → instrument;
V1.4.1 adds `MARGINED_IN` instrument → currency | instrument (collateral).
V1.6 makes `TRACKS` instrument → instrument storable: a tracker (e.g. a
collateralised tracker certificate issued as a token) → the instrument whose
market value its terms track; it confers no claim on that instrument, unlike
`TOKENIZES`, and is not a derivative contract (`DERIVES_FROM`).
V1.7 adds `TRACKS` instrument → currency: a stablecoin → the fiat currency
one unit is designed to be worth (TIP-20 `currency()`); USD, USD Coin, Tether
and pathUSD stay four distinct objects.
A derivative's price unit (`DENOMINATED_IN`), the asset its cash flows are
paid in (`SETTLES_IN`) and its collateral (`MARGINED_IN`) are separate facts.
`DEPLOYED_ON` is projected from a deployment's chain. Types with no rules yet
(`HOLDS`, `MEMBER_OF`, `PRICED_BY`, `AVAILABLE_ON`, `RELATED_TO`)
cannot be constructed. Provenance is mandatory. A relationship is a current
assertion by a source; validity periods are deferred.

## Chains and deployments (V1.4)

`undrly_core::onchain`. A `Chain` is identified by its CAIP-2 id
(`eip155:<chain id>`, `solana:<32 base58 chars>`). A `Deployment` is an asset
on one chain: `(chain, ChainAsset)` where `ChainAsset` is a CAIP-19 asset
namespace and reference validated for the chain's namespace (`erc20` EVM
address, lowercase, EIP-55 verified when mixed case; `token` base58 32-byte
Solana mint; `slip44` native coin type). What a deployment is a form of is a
`REPRESENTS` relationship, never a field. See
[`v1.4-cross-ecosystem-identity.md`](v1.4-cross-ecosystem-identity.md).

## Market observations

`MarketObservation::new(subject, basis, price_type, price, bid_ask, unit,
source_id, observed_at, received_at)`:

- `subject`: `PriceSubject::Instrument(InstrumentId) | PriceSubject::Currency(CurrencyId)`
  (FX prices a currency, e.g. 1 EUR in USD).
- `basis`: `Venue(VenueId) | Aggregated | Derived`, so a venue quote always
  names its venue and derived values never pose as venue quotes.
- `price_type`: `last | mid | mark | reference`; `bid_ask` is optional and
  never crossed.
- `unit`: `PriceUnit::Currency(CurrencyId) | PriceUnit::Asset(InstrumentId)`.
  Codes/symbols are never unit identity. Self-denominated prices are rejected.
- `observed_at` (source time, `None` when the source states none) and
  `received_at` (Undrly time) are separate; no ordering is enforced (clocks
  differ). Freshness is computed at read time.

Canonical quotes come from observations through a named aggregation method
(`undrly_core::quote`): `latest-observation-v1`, or `mean-venue-mid-v1` for
BTC/USD. See `docs/hackathon-v1.md` §13.

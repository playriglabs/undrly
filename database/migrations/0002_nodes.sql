-- Canonical nodes. Every canonical id is registered in `nodes` with its
-- immutable category; each category table references (id, category), so a
-- row can only exist in the table matching its id's category.

CREATE TABLE nodes (
  -- uuid_extract_version() is NULL for non-RFC-variant UUIDs (e.g. nil), and a
  -- CHECK passes on NULL, so compare with IS NOT DISTINCT FROM.
  id         uuid PRIMARY KEY CHECK (uuid_extract_version(id) IS NOT DISTINCT FROM 7),
  category   text NOT NULL
               CHECK (category IN ('entity', 'instrument', 'listing', 'venue', 'currency')),
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (id, category)
);

CREATE TABLE entities (
  id          uuid PRIMARY KEY,
  category    text NOT NULL DEFAULT 'entity' CHECK (category = 'entity'),
  entity_kind text NOT NULL CHECK (entity_kind IN ('company')),
  name        display_name NOT NULL,
  FOREIGN KEY (id, category) REFERENCES nodes (id, category)
);

-- instrument_class is an attribute, not identity. Crypto assets (including
-- stablecoins) are instruments; fiat currencies are `currencies`.
CREATE TABLE instruments (
  id               uuid PRIMARY KEY,
  category         text NOT NULL DEFAULT 'instrument' CHECK (category = 'instrument'),
  instrument_class text NOT NULL CHECK (instrument_class IN ('equity', 'crypto_asset')),
  name             display_name NOT NULL,
  FOREIGN KEY (id, category) REFERENCES nodes (id, category)
);

CREATE TABLE venues (
  id       uuid PRIMARY KEY,
  category text NOT NULL DEFAULT 'venue' CHECK (category = 'venue'),
  name     display_name NOT NULL,
  FOREIGN KEY (id, category) REFERENCES nodes (id, category)
);

-- Fiat currencies. The ISO 4217 code is an identifier (see `identifiers`).
CREATE TABLE currencies (
  id       uuid PRIMARY KEY,
  category text NOT NULL DEFAULT 'currency' CHECK (category = 'currency'),
  name     display_name NOT NULL,
  FOREIGN KEY (id, category) REFERENCES nodes (id, category)
);

-- An instrument's listing on a venue. Symbols live in `listing_symbols`.
-- `LISTED_ON` is projected from this table, never stored as a graph edge.
CREATE TABLE listings (
  id            uuid PRIMARY KEY,
  category      text NOT NULL DEFAULT 'listing' CHECK (category = 'listing'),
  instrument_id uuid NOT NULL REFERENCES instruments (id),
  venue_id      uuid NOT NULL REFERENCES venues (id),
  source_id     source_id NOT NULL REFERENCES sources (id),
  received_at   timestamptz NOT NULL,
  FOREIGN KEY (id, category) REFERENCES nodes (id, category),
  UNIQUE (id, venue_id)
);
CREATE INDEX listings_instrument_idx ON listings (instrument_id);
CREATE INDEX listings_venue_idx ON listings (venue_id);

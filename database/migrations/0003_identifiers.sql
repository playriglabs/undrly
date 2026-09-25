-- Identifier layer. External identifiers map to canonical nodes; they are
-- never canonical identity. Namespace-specific check digits (ISIN, FIGI, LEI)
-- are validated in Rust; the database enforces shape, category, and
-- one-node-at-a-time.

-- Which global namespaces may identify which node categories.
-- Must equal undrly_core::Namespace::categories (tested by undrly-store).
CREATE TABLE identifier_schemes (
  scheme        text NOT NULL,
  node_category text NOT NULL,
  PRIMARY KEY (scheme, node_category)
);
INSERT INTO identifier_schemes (scheme, node_category) VALUES
  ('isin', 'instrument'),
  ('figi', 'instrument'),
  ('figi', 'listing'),
  ('lei', 'entity'),
  ('mic', 'venue'),
  ('iso4217', 'currency');

-- Globally scoped identifiers with validity periods.
CREATE TABLE identifiers (
  id            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  scheme        text NOT NULL,
  value         text NOT NULL,
  node_id       uuid NOT NULL,
  node_category text NOT NULL,
  valid_during  validity NOT NULL DEFAULT '(,)',
  source_id     source_id NOT NULL REFERENCES sources (id),
  received_at   timestamptz NOT NULL,
  FOREIGN KEY (node_id, node_category) REFERENCES nodes (id, category),
  FOREIGN KEY (scheme, node_category) REFERENCES identifier_schemes (scheme, node_category),
  CONSTRAINT identifiers_value_shape CHECK (CASE scheme
    WHEN 'isin'    THEN value ~ '^[A-Z]{2}[A-Z0-9]{9}[0-9]$'
    WHEN 'figi'    THEN value ~ '^[B-DF-HJ-NP-TV-Z0-9]{2}G[B-DF-HJ-NP-TV-Z0-9]{8}[0-9]$'
    WHEN 'lei'     THEN value ~ '^[A-Z0-9]{18}[0-9]{2}$'
    WHEN 'mic'     THEN value ~ '^[A-Z0-9]{4}$'
    WHEN 'iso4217' THEN value ~ '^[A-Z]{3}$'
    ELSE false
  END),
  -- An external identifier maps to at most one node at any instant.
  CONSTRAINT identifiers_one_node_at_a_time
    EXCLUDE USING gist (scheme WITH =, value WITH =, valid_during WITH &&),
  -- Lets identifier_conflicts reference (id, scheme, value) consistently.
  UNIQUE (id, scheme, value)
);
CREATE INDEX identifiers_node_idx ON identifiers (node_id);

-- Venue-scoped symbols. A symbol means something only on its venue, over a
-- period; symbols change and are reused. Case and punctuation are preserved.
CREATE TABLE listing_symbols (
  id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  listing_id   uuid NOT NULL,
  venue_id     uuid NOT NULL,
  symbol       text NOT NULL
                 CHECK (symbol <> '' AND symbol !~ '[[:space:][:cntrl:]]' AND char_length(symbol) <= 64),
  valid_during validity NOT NULL DEFAULT '(,)',
  source_id    source_id NOT NULL REFERENCES sources (id),
  received_at  timestamptz NOT NULL,
  -- The symbol's venue must be the listing's venue.
  FOREIGN KEY (listing_id, venue_id) REFERENCES listings (id, venue_id),
  CONSTRAINT listing_symbols_one_listing_at_a_time
    EXCLUDE USING gist (venue_id WITH =, symbol WITH =, valid_during WITH &&),
  CONSTRAINT listing_symbols_one_symbol_at_a_time
    EXCLUDE USING gist (listing_id WITH =, valid_during WITH &&),
  UNIQUE (id, venue_id, symbol)
);
CREATE INDEX listing_symbols_symbol_idx ON listing_symbols (symbol);

-- Quarantine for rejected conflicting claims: a source claimed an identifier
-- that is already mapped to a different node for an overlapping period.
-- Nothing here is authoritative; conflicts are never auto-merged and no
-- source is chosen as truth. Rows keep the full claim plus a reference to
-- the existing mapping it collided with, for investigation/reconciliation.
CREATE TABLE identifier_conflicts (
  id                            bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  namespace                     text NOT NULL
                                  CHECK (namespace IN ('isin', 'figi', 'lei', 'mic', 'iso4217', 'venue_symbol')),
  value                         text NOT NULL,
  -- Venue scope; required exactly for venue symbols.
  scope_venue_id                uuid REFERENCES venues (id),
  claimed_node_id               uuid NOT NULL,
  claimed_node_category         text NOT NULL,
  claimed_valid_during          validity NOT NULL,
  source_id                     source_id NOT NULL REFERENCES sources (id),
  received_at                   timestamptz NOT NULL,
  conflicting_identifier_id     bigint,
  conflicting_listing_symbol_id bigint,
  detected_at                   timestamptz NOT NULL DEFAULT now(),
  -- NULL for venue symbols, so the scheme FK below applies only to global namespaces.
  global_scheme                 text GENERATED ALWAYS AS
                                  (CASE WHEN namespace <> 'venue_symbol' THEN namespace END) STORED,
  FOREIGN KEY (claimed_node_id, claimed_node_category) REFERENCES nodes (id, category),
  FOREIGN KEY (global_scheme, claimed_node_category)
    REFERENCES identifier_schemes (scheme, node_category),
  -- The referenced existing mapping must be for the same identifier.
  FOREIGN KEY (conflicting_identifier_id, global_scheme, value)
    REFERENCES identifiers (id, scheme, value),
  FOREIGN KEY (conflicting_listing_symbol_id, scope_venue_id, value)
    REFERENCES listing_symbols (id, venue_id, symbol),
  CONSTRAINT identifier_conflicts_target CHECK (
    CASE WHEN namespace = 'venue_symbol'
      THEN scope_venue_id IS NOT NULL
           AND claimed_node_category = 'listing'
           AND conflicting_listing_symbol_id IS NOT NULL
           AND conflicting_identifier_id IS NULL
      ELSE scope_venue_id IS NULL
           AND conflicting_identifier_id IS NOT NULL
           AND conflicting_listing_symbol_id IS NULL
    END
  )
);
CREATE INDEX identifier_conflicts_lookup_idx ON identifier_conflicts (namespace, value);

-- SEC Central Index Key (CIK) as a global identifier namespace for entities.
-- Canonical spelling is EDGAR's 10-digit zero-padded form; CIK has no check
-- digit, and 0 is never assigned. Mirrors undrly_core::Cik.

INSERT INTO identifier_schemes (scheme, node_category) VALUES ('cik', 'entity');

ALTER TABLE identifiers
  DROP CONSTRAINT identifiers_value_shape,
  ADD CONSTRAINT identifiers_value_shape CHECK (CASE scheme
    WHEN 'isin'    THEN value ~ '^[A-Z]{2}[A-Z0-9]{9}[0-9]$'
    WHEN 'figi'    THEN value ~ '^[B-DF-HJ-NP-TV-Z0-9]{2}G[B-DF-HJ-NP-TV-Z0-9]{8}[0-9]$'
    WHEN 'lei'     THEN value ~ '^[A-Z0-9]{18}[0-9]{2}$'
    WHEN 'mic'     THEN value ~ '^[A-Z0-9]{4}$'
    WHEN 'iso4217' THEN value ~ '^[A-Z]{3}$'
    WHEN 'cik'     THEN value ~ '^[0-9]{10}$' AND value <> '0000000000'
    ELSE false
  END);

ALTER TABLE identifier_conflicts
  DROP CONSTRAINT identifier_conflicts_namespace_check,
  ADD CONSTRAINT identifier_conflicts_namespace_check
    CHECK (namespace IN ('isin', 'figi', 'lei', 'mic', 'iso4217', 'cik', 'venue_symbol'));

-- Foundation: extensions, shared value domains, sources.

-- Exclusion constraints over (equality, time range) need btree_gist.
CREATE EXTENSION IF NOT EXISTS btree_gist;

-- Mirrors undrly_core::DisplayName: non-empty, trimmed, no control
-- characters, at most 256 characters. Mutable display data, never identity.
CREATE DOMAIN display_name AS text
  CHECK (
    VALUE <> ''
    AND VALUE !~ '^\s|\s$'
    AND VALUE !~ '[[:cntrl:]]'
    AND char_length(VALUE) <= 256
  );

-- Mirrors rust_decimal::Decimal: scale <= 28 and |value| < 2^96.
-- Unconstrained numeric preserves scale ('183.4200' stays '183.4200').
-- NaN and +/-Infinity fail the abs() bound.
CREATE DOMAIN financial_decimal AS numeric
  CHECK (
    scale(VALUE) <= 28
    AND abs(VALUE) <= 79228162514264337593543950335
  );

-- Mirrors undrly_core::Validity: half-open [from, until), never empty.
CREATE DOMAIN validity AS tstzrange
  CHECK (
    NOT isempty(VALUE)
    AND (lower_inf(VALUE) OR lower_inc(VALUE))
    AND (upper_inf(VALUE) OR NOT upper_inc(VALUE))
  );

-- Mirrors undrly_core::SourceId.
CREATE DOMAIN source_id AS text
  CHECK (VALUE ~ '^[a-z0-9]+(-[a-z0-9]+)*$' AND length(VALUE) <= 64);

CREATE TABLE sources (
  id             source_id PRIMARY KEY,
  name           display_name NOT NULL,
  -- 'unknown' must be treated as restricted.
  redistribution text NOT NULL DEFAULT 'unknown'
                   CHECK (redistribution IN ('permitted', 'restricted', 'unknown')),
  created_at     timestamptz NOT NULL DEFAULT now()
);

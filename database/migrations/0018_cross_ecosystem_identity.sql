-- V1.4 cross-ecosystem financial identity (docs/v1.4-cross-ecosystem-identity.md).
-- All additive: two node categories (chain, deployment), one instrument class
-- (tokenized_security) and two relationship rules. No existing row changes.

-- Node categories. Must equal undrly_core::Category.
ALTER TABLE nodes
  DROP CONSTRAINT nodes_category_check,
  ADD CONSTRAINT nodes_category_check
    CHECK (category IN ('entity', 'instrument', 'listing', 'venue', 'currency',
                        'chain', 'deployment'));

-- Instrument classes. Must equal undrly_core::InstrumentClass.
-- `tokenized_security`: an instrument issued as tokens whose value is tied to
-- another instrument (a TOKENIZES or DERIVES_FROM edge says how). It is never
-- the security it refers to and never inherits that security's identifiers.
ALTER TABLE instruments
  DROP CONSTRAINT instruments_instrument_class_check,
  ADD CONSTRAINT instruments_instrument_class_check
    CHECK (instrument_class IN ('equity', 'crypto_asset', 'commodity', 'perpetual_future', 'fx',
                                'tokenized_security'));

-- A blockchain network, identified by its CAIP-2 chain id
-- (`<namespace>:<reference>`, e.g. `eip155:1`). The chain id is the chain's
-- primary external identifier; the canonical id is generated as for every
-- node. Only namespaces whose asset-address rules Undrly implements are
-- accepted. CAIP-2 ids are case-sensitive and stored exactly.
CREATE TABLE chains (
  id               uuid PRIMARY KEY,
  category         text NOT NULL DEFAULT 'chain' CHECK (category = 'chain'),
  name             display_name NOT NULL,
  caip2_namespace  text NOT NULL CHECK (caip2_namespace IN ('eip155', 'solana')),
  caip2_reference  text NOT NULL,
  source_record_id bigint NOT NULL REFERENCES source_records (id),
  FOREIGN KEY (id, category) REFERENCES nodes (id, category),
  CONSTRAINT chains_caip2_reference_shape CHECK (CASE caip2_namespace
    -- EIP-155 chain id: an unsigned integer, decimal, at most 32 digits (CAIP-2).
    WHEN 'eip155' THEN caip2_reference ~ '^[1-9][0-9]{0,31}$'
    -- The genesis hash truncated to 32 base58 characters.
    WHEN 'solana' THEN caip2_reference ~ '^[1-9A-HJ-NP-Za-km-z]{32}$'
    ELSE false
  END),
  -- One node per network.
  CONSTRAINT chains_one_per_caip2 UNIQUE (caip2_namespace, caip2_reference),
  -- Lets deployments reference the chain together with its namespace.
  UNIQUE (id, caip2_namespace)
);

-- An asset's existence on one chain: a token contract, a mint, or the chain's
-- native asset. Identified by (chain, CAIP-19 asset namespace, asset
-- reference): an address means nothing without its chain, so the same EVM
-- address on two chains is two deployments. What the deployment is a form of
-- is a REPRESENTS edge (with provenance), never a column: unknown stays
-- unknown. `DEPLOYED_ON` (deployment → chain) is projected from `chain_id`,
-- like `LISTED_ON` from listings, and is never stored as an edge.
CREATE TABLE deployments (
  id               uuid PRIMARY KEY,
  category         text NOT NULL DEFAULT 'deployment' CHECK (category = 'deployment'),
  chain_id         uuid NOT NULL,
  chain_namespace  text NOT NULL,
  asset_namespace  text NOT NULL CHECK (asset_namespace IN ('erc20', 'token', 'slip44')),
  asset_reference  text NOT NULL,
  source_record_id bigint NOT NULL REFERENCES source_records (id),
  FOREIGN KEY (id, category) REFERENCES nodes (id, category),
  -- The namespace is the chain's own: it cannot disagree with the chain.
  FOREIGN KEY (chain_id, chain_namespace) REFERENCES chains (id, caip2_namespace),
  -- ERC-20 contracts exist on EIP-155 chains, SPL token mints on Solana;
  -- SLIP-44 coin types name a chain's native asset on either.
  CONSTRAINT deployments_asset_namespace_on_chain CHECK (
    (asset_namespace = 'erc20' AND chain_namespace = 'eip155')
    OR (asset_namespace = 'token' AND chain_namespace = 'solana')
    OR asset_namespace = 'slip44'
  ),
  CONSTRAINT deployments_asset_reference_shape CHECK (CASE asset_namespace
    -- Canonical lowercase hex; EIP-55 checksums are verified in Rust on input.
    WHEN 'erc20'  THEN asset_reference ~ '^0x[0-9a-f]{40}$'
    -- A base58 public key (32 bytes); decoding is verified in Rust.
    WHEN 'token'  THEN asset_reference ~ '^[1-9A-HJ-NP-Za-km-z]{32,44}$'
    WHEN 'slip44' THEN asset_reference ~ '^(0|[1-9][0-9]{0,9})$'
    ELSE false
  END),
  CONSTRAINT deployments_one_per_asset UNIQUE (chain_id, asset_namespace, asset_reference)
);
CREATE INDEX deployments_reference_idx ON deployments (asset_reference);

-- deployment REPRESENTS instrument: the deployment is that instrument on its
-- chain (e.g. a USDC token contract → the USDC asset).
-- instrument TOKENIZES instrument: the subject is a token-form claim on the
-- object, which is held in custody or escrow (a wrapped or bridged asset, a
-- tokenized security backed one to one).
INSERT INTO relationship_rules (relationship_type, subject_category, object_category) VALUES
  ('REPRESENTS', 'deployment', 'instrument'),
  ('TOKENIZES',  'instrument', 'instrument');

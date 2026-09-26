-- Corporate actions (docs/v1.3-market-data.md §9). Additive.
--
-- One row per (source action, instrument it concerns). Terms are the
-- source's own numbers, kept as decimals; which columns a type uses is
-- fixed per type (see the column comments). A symbol of another security
-- (`other_symbol`: the acquirer, the spun-off company, the new name) is the
-- source's spelling, never an identifier. A newer record replaces an action
-- whose announced terms or dates changed.
CREATE TABLE corporate_actions (
  id               bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  instrument_id    uuid NOT NULL REFERENCES instruments (id),
  source_id        source_id NOT NULL,
  source_action_id text NOT NULL CHECK (source_action_id <> '' AND char_length(source_action_id) <= 64),
  action_type      text NOT NULL CHECK (action_type IN (
                     'cash_dividend', 'stock_dividend', 'forward_split', 'reverse_split',
                     'spin_off', 'cash_merger', 'stock_merger', 'stock_and_cash_merger',
                     'name_change')),
  -- What the instrument is in the action.
  role             text NOT NULL CHECK (role IN ('subject', 'acquiree', 'acquirer', 'spin_off_parent')),
  ex_date          date,
  record_date      date,
  payable_date     date,
  effective_date   date,
  process_date     date,
  -- Cash per share (dividends, cash in mergers), as the source states it;
  -- the source states no currency.
  cash_amount      financial_decimal CHECK (cash_amount >= 0),
  -- Shares received per share held (stock dividends).
  stock_rate       financial_decimal CHECK (stock_rate > 0),
  -- Old : new ratio (splits: old shares → new shares; mergers: acquiree :
  -- acquirer shares; spin-offs: parent : new shares).
  old_rate         financial_decimal CHECK (old_rate > 0),
  new_rate         financial_decimal CHECK (new_rate > 0),
  special          boolean,
  other_symbol     text CHECK (other_symbol <> '' AND char_length(other_symbol) <= 64),
  received_at      timestamptz NOT NULL,
  source_record_id bigint NOT NULL,
  CONSTRAINT corporate_actions_source_record_fkey
    FOREIGN KEY (source_record_id, source_id, received_at)
    REFERENCES source_records (id, source_id, received_at),
  CONSTRAINT corporate_actions_ratio CHECK ((old_rate IS NULL) = (new_rate IS NULL)),
  CONSTRAINT corporate_actions_one_per_instrument
    UNIQUE (source_id, source_action_id, instrument_id, role)
);
CREATE INDEX corporate_actions_instrument_idx
  ON corporate_actions (instrument_id, coalesce(ex_date, effective_date, process_date));

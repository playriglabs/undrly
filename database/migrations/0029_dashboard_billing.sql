-- Dashboard plans, billed through Mayarin subscriptions
-- (typescript/apps/dashboard/src/server/billing.ts).
--
-- Mayarin owns the billing schedule and the invoices; this table only links a
-- dashboard user to their Mayarin subscription and records how far access is
-- paid. `paid_through` moves forward only when the webhook handler has
-- re-read a cycle's payment as settled, never on the webhook alone.
CREATE TABLE dashboard.subscriptions (
  id                      text PRIMARY KEY,
  user_id                 text NOT NULL REFERENCES dashboard.users (id) ON DELETE CASCADE,
  plan                    text NOT NULL CHECK (plan IN ('pro', 'growth')),
  -- Mayarin's `sub_…` id. Creating one there is not idempotent, so a user
  -- has at most one live row and we check it before creating another.
  mayarin_subscription_id text NOT NULL UNIQUE,
  state                   text NOT NULL CHECK (state IN ('active', 'paused', 'cancelled')),
  paid_through            timestamptz,
  created_at              timestamptz NOT NULL DEFAULT now(),
  updated_at              timestamptz NOT NULL DEFAULT now()
);

-- One live (non-cancelled) subscription per user.
CREATE UNIQUE INDEX subscriptions_live_user
  ON dashboard.subscriptions (user_id) WHERE state <> 'cancelled';
CREATE INDEX subscriptions_user_id ON dashboard.subscriptions (user_id);

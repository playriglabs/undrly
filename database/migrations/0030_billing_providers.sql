-- A second billing provider: Polar (card, merchant of record) next to
-- Mayarin (crypto). Same table, one live subscription per user whichever
-- provider bills it (typescript/apps/dashboard/src/server/subscriptions.ts).
ALTER TABLE dashboard.subscriptions
  RENAME COLUMN mayarin_subscription_id TO provider_subscription_id;
ALTER TABLE dashboard.subscriptions
  DROP CONSTRAINT subscriptions_mayarin_subscription_id_key;

ALTER TABLE dashboard.subscriptions
  ADD COLUMN provider text NOT NULL DEFAULT 'mayarin' CHECK (provider IN ('mayarin', 'polar'));
ALTER TABLE dashboard.subscriptions ALTER COLUMN provider DROP DEFAULT;

ALTER TABLE dashboard.subscriptions
  ADD CONSTRAINT subscriptions_provider_subscription UNIQUE (provider, provider_subscription_id);

-- Polar cancels at the end of the paid period: still `active` until then.
ALTER TABLE dashboard.subscriptions
  ADD COLUMN cancel_at_period_end boolean NOT NULL DEFAULT false;

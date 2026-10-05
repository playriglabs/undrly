/**
 * `dashboard.subscriptions`: which plan a user pays for, through which
 * provider, and how far access is paid (database/migrations/0029, 0030).
 * Server-only. Providers: Mayarin (crypto, ./mayarin.ts) and Polar (card,
 * ./polar.ts).
 */
import type { PaidPlan } from "../lib/plans";
import { getPool } from "./db";

export type Provider = "mayarin" | "polar";
export type SubscriptionState = "active" | "paused" | "cancelled";

export type SubscriptionRow = {
  id: string;
  plan: PaidPlan;
  provider: Provider;
  provider_subscription_id: string;
  state: SubscriptionState;
  paid_through: Date | null;
  cancel_at_period_end: boolean;
};

export type Db = Pick<ReturnType<typeof getPool>, "query">;

const COLUMNS =
  "id, plan, provider, provider_subscription_id, state, paid_through, cancel_at_period_end";

/** The user's one live (not cancelled) subscription, if any. */
export async function liveRow(userId: string, db: Db = getPool()): Promise<SubscriptionRow | null> {
  const { rows } = await db.query<SubscriptionRow>(
    `SELECT ${COLUMNS} FROM subscriptions WHERE user_id = $1 AND state <> 'cancelled'`,
    [userId],
  );
  return rows[0] ?? null;
}

export async function rowByProviderId(
  provider: Provider,
  providerSubscriptionId: string,
  db: Db = getPool(),
): Promise<SubscriptionRow | null> {
  const { rows } = await db.query<SubscriptionRow>(
    `SELECT ${COLUMNS} FROM subscriptions WHERE provider = $1 AND provider_subscription_id = $2`,
    [provider, providerSubscriptionId],
  );
  return rows[0] ?? null;
}

/**
 * Serialize billing changes per user. A session lock, not a transaction: each
 * write commits as soon as its provider call has succeeded, so a later failure
 * never rolls the database back behind what the provider already did.
 */
export async function withUserLock<T>(userId: string, fn: (db: Db) => Promise<T>): Promise<T> {
  const conn = await getPool().connect();
  const key = `billing:${userId}`;
  try {
    await conn.query("SELECT pg_advisory_lock(hashtext($1))", [key]);
    try {
      return await fn(conn);
    } finally {
      await conn.query("SELECT pg_advisory_unlock(hashtext($1))", [key]);
    }
  } finally {
    conn.release();
  }
}

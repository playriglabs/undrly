/**
 * Crypto billing through Mayarin subscriptions
 * (https://docs.mayarin.xyz/guides/subscriptions). Server-only.
 *
 * Send-to-pay: Mayarin issues a USD invoice each month and emails its pay
 * link; the subscriber pays it in stablecoins or ETH. Access moves forward in
 * `settleCycle`, after the webhook's payment is re-read from the API as
 * settled.
 */
import { createMayarin, type MayarinClient } from "@mayarin/sdk";
import { type PaidPlan, planById } from "../lib/plans";
import { getPool } from "./db";
import type { SessionUser } from "./session";
import type { Db, SubscriptionRow } from "./subscriptions";

/** Testnet until Mayarin provisions mainnet; switch the URL and key together. */
const TESTNET_API = "https://api-testnet.mayarin.xyz";

export const mayarinConfigured = () => Boolean(process.env.MAYARIN_SECRET_KEY);
export const mayarinTestnet = () =>
  (process.env.MAYARIN_API_URL || TESTNET_API).includes("testnet");

let client: MayarinClient | undefined;
function mayarin(): MayarinClient {
  if (!client) {
    const secretKey = process.env.MAYARIN_SECRET_KEY;
    if (!secretKey) throw new Error("Crypto payments are not configured (MAYARIN_SECRET_KEY)");
    client = createMayarin({ baseUrl: process.env.MAYARIN_API_URL || TESTNET_API, secretKey });
  }
  return client;
}

const OWED = new Set(["issued", "partially_paid", "overdue"]);

/**
 * Money in US English. Mayarin's `display` follows the merchant's locale
 * ("$ 20,00"); `formatted` is the plain decimal ("20.00").
 */
function money({ formatted, asset }: { formatted: string; asset: string }): string {
  try {
    return new Intl.NumberFormat("en-US", { style: "currency", currency: asset }).format(
      Number(formatted),
    );
  } catch {
    return `${formatted} ${asset}`; // not an ISO currency, e.g. USDC
  }
}

/**
 * What Mayarin says about a subscription now: its state, next bill, the
 * oldest cycle owed, and how far access is paid. Reading it also settles the
 * newest paid cycle, so access does not wait for the webhook (or, in local
 * development, for a webhook that can't reach us).
 */
export async function mayarinStatus(row: SubscriptionRow) {
  const { subscription, cycles } = await mayarin().commerce.subscriptions.get(
    row.provider_subscription_id,
  );
  // Newest first: the first paid cycle is the latest, the last owed one the oldest.
  const lastPaid = cycles.find((c) => c.status === "paid");
  const owed = cycles.filter((c) => OWED.has(c.status)).at(-1);
  const paidThrough =
    (lastPaid?.issuedAt &&
      (await extendAccess(row.provider_subscription_id, lastPaid.issuedAt, subscription.state))) ||
    row.paid_through;
  return {
    state: subscription.state,
    paidThrough,
    nextBillingAt: subscription.nextBillingAt,
    amount: money(subscription.amount),
    due: owed
      ? {
          url: owed.url,
          dueAt: owed.dueAt,
          total: money(owed.outstanding),
          overdue: owed.status === "overdue",
        }
      : null,
  };
}

/**
 * Create the subscription at Mayarin and record it. Mayarin's create is not
 * idempotent: callers hold the user lock and have checked for a live row.
 * `startAt` null bills now; a date bills from then (a plan switch carries the
 * paid period over).
 */
export async function startMayarin(
  user: SessionUser,
  plan: PaidPlan,
  startAt: Date | null,
  db: Db,
): Promise<void> {
  const { name, usd } = planById(plan);
  if (!usd) throw new Error(`${name} has no price`);
  const created = await mayarin().commerce.subscriptions.create({
    customer: { name: user.name || user.email, email: user.email },
    description: `Undrly ${name} plan`,
    amount: { amount: usd, asset: "USD" },
    interval: "month",
    startAt: startAt?.toISOString(),
    daysUntilDue: 7,
  });
  try {
    await db.query(
      `INSERT INTO subscriptions (id, user_id, plan, provider, provider_subscription_id, state, paid_through)
       VALUES ($1, $2, $3, 'mayarin', $4, $5, $6)`,
      [crypto.randomUUID(), user.id, plan, created.id, created.state, startAt],
    );
  } catch (error) {
    // Never leave a subscription billing at Mayarin that the dashboard doesn't know about.
    await mayarin()
      .commerce.subscriptions.cancel(created.id)
      .catch(() => {});
    throw error;
  }
}

/**
 * Cancel at Mayarin and void the cycles nobody has started paying, so a
 * cancelled plan cannot be paid by mistake. Partly paid cycles stay payable.
 */
export async function endMayarin(row: SubscriptionRow, db: Db): Promise<void> {
  const api = mayarin();
  const { subscription, cycles } = await api.commerce.subscriptions.get(
    row.provider_subscription_id,
  );
  if (subscription.state !== "cancelled") await api.commerce.subscriptions.cancel(subscription.id);
  for (const cycle of cycles) {
    if (cycle.status === "issued" || cycle.status === "overdue") {
      await api.commerce.invoices.void(cycle.id);
    }
  }
  await db.query("UPDATE subscriptions SET state = 'cancelled', updated_at = now() WHERE id = $1", [
    row.id,
  ]);
}

/**
 * A `payment.state_changed` webhook named a subscription. Re-read the payment
 * (the webhook is a notification, not settlement truth) and, once it has
 * cleared, extend access to one month past the paid cycle's billing date.
 * GREATEST keeps redeliveries and out-of-order events harmless.
 */
export async function settleCycle(event: {
  subscriptionId: string;
  paymentIntentId: string;
  invoiceId: string | undefined;
}): Promise<void> {
  const payment = await mayarin().payment.get(event.paymentIntentId);
  if (payment.clearing?.state !== "SUCCESS") return;

  const { subscription, cycles } = await mayarin().commerce.subscriptions.get(event.subscriptionId);
  const cycle = cycles.find((c) => c.id === event.invoiceId);
  const billedAt = cycle?.issuedAt ?? new Date().toISOString();
  await extendAccess(event.subscriptionId, billedAt, subscription.state);
}

/**
 * Access runs one month past a paid cycle's billing date. Only ever moves
 * `paid_through` forward, so repeats and out-of-order calls are harmless.
 * Returns the new date, or null when it was already that far.
 */
async function extendAccess(
  subscriptionId: string,
  billedAt: string,
  state: SubscriptionRow["state"],
): Promise<Date | null> {
  const { rows } = await getPool().query<{ paid_through: Date }>(
    `UPDATE subscriptions
        SET paid_through = $2::timestamptz + interval '1 month', state = $3, updated_at = now()
      WHERE provider = 'mayarin' AND provider_subscription_id = $1
        AND (paid_through IS NULL OR paid_through < $2::timestamptz + interval '1 month')
      RETURNING paid_through`,
    [subscriptionId, billedAt, state],
  );
  return rows[0]?.paid_through ?? null;
}

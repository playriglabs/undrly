/**
 * Card billing through Polar, the merchant of record
 * (https://polar.sh/docs/integrate/sdk/adapters/better-auth). Server-only.
 *
 * Polar runs checkout, renewals, tax and the customer portal; it knows our
 * user by `external_customer_id` (the Better Auth user id, set by the
 * checkout plugin). Every webhook and every return from checkout calls
 * `syncPolar`, which re-reads the user's subscriptions from Polar and writes
 * the result, so event order and redelivery do not matter.
 */
import { checkout, polar, portal, webhooks } from "@polar-sh/better-auth";
import { createPolarCore, type PolarCore } from "@polar-sh/sdk/2026-10";
import {
  listSubscriptions,
  updateSubscriptions,
} from "@polar-sh/sdk/2026-10/services/subscriptions";
import { PAID_PLANS, type PaidPlan } from "../lib/plans";
import { endMayarin } from "./mayarin";
import { liveRow, type SubscriptionRow, withUserLock } from "./subscriptions";

export const polarConfigured = () => Boolean(process.env.POLAR_ACCESS_TOKEN);
/** Sandbox until POLAR_SERVER=production; the access token belongs to one or the other. */
export const polarSandbox = () => process.env.POLAR_SERVER !== "production";

let client: PolarCore | undefined;
function polarClient(): PolarCore {
  if (!client) {
    const accessToken = process.env.POLAR_ACCESS_TOKEN;
    if (!accessToken) throw new Error("Card payments are not configured (POLAR_ACCESS_TOKEN)");
    client = createPolarCore({
      accessToken,
      environment: polarSandbox() ? "sandbox" : "production",
    });
  }
  return client;
}

/** Each paid plan's Polar product, from POLAR_PRODUCT_PRO and POLAR_PRODUCT_GROWTH. */
function products(): { productId: string; slug: PaidPlan }[] {
  return PAID_PLANS.flatMap((slug) => {
    const productId = process.env[`POLAR_PRODUCT_${slug.toUpperCase()}`];
    return productId ? [{ productId, slug }] : [];
  });
}

/**
 * The Better Auth plugin: `/api/auth/checkout`, `/api/auth/customer/portal`
 * and the webhook at `/api/auth/polar/webhooks`. Empty when Polar is not
 * configured, so the dashboard runs without it.
 */
export function polarPlugins(baseUrl: string) {
  if (!polarConfigured()) return [];
  const sync = async (payload: { data: { customer: { external_id?: string | null } } }) => {
    const userId = payload.data.customer.external_id;
    if (userId) await syncPolar(userId);
  };
  return [
    polar({
      client: polarClient(),
      // Customers are created by checkout, so sign-up never waits on Polar.
      createCustomerOnSignUp: false,
      use: [
        checkout({
          products: products(),
          successUrl: "/billing?checkout_id={CHECKOUT_ID}",
          returnUrl: `${baseUrl}/billing`,
          authenticatedUsersOnly: true,
          theme: "dark",
        }),
        portal({ returnUrl: `${baseUrl}/billing`, theme: "dark" }),
        webhooks({
          secret: process.env.POLAR_WEBHOOK_SECRET ?? "",
          onSubscriptionCreated: sync,
          onSubscriptionUpdated: sync,
          onSubscriptionActive: sync,
          onSubscriptionCanceled: sync,
          onSubscriptionUncanceled: sync,
          onSubscriptionRevoked: sync,
          onSubscriptionCycled: sync,
          onSubscriptionPastDue: sync,
        }),
      ],
    }),
  ];
}

/** Polar statuses that still hold a subscription open; the rest have ended. */
const LIVE = new Set(["trialing", "active", "past_due", "unpaid", "paused"]);
/** Statuses whose current period is paid for. */
const PAID = new Set(["trialing", "active"]);

/**
 * Make the user's rows match Polar. The newest live Polar subscription
 * becomes (or updates) the user's live row, ending a Mayarin one if the user
 * moved to card; Polar rows Polar no longer holds open are cancelled, and
 * access stops now if it was revoked early.
 */
export async function syncPolar(userId: string): Promise<void> {
  const page = await listSubscriptions(polarClient())({ external_customer_id: userId, limit: 20 });
  const live = page.items
    .filter((s) => LIVE.has(s.status))
    .sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at))[0];
  const plan = live && products().find((p) => p.productId === live.product_id)?.slug;

  await withUserLock(userId, async (db) => {
    await db.query(
      `UPDATE subscriptions
          SET state = 'cancelled', paid_through = LEAST(paid_through, now()), updated_at = now()
        WHERE user_id = $1 AND provider = 'polar' AND state <> 'cancelled'
          AND provider_subscription_id IS DISTINCT FROM $2`,
      [userId, plan ? live.id : null],
    );
    if (!live || !plan) return;

    const current = await liveRow(userId, db);
    if (current?.provider === "mayarin") await endMayarin(current, db);
    await db.query(
      `INSERT INTO subscriptions
         (id, user_id, plan, provider, provider_subscription_id, state, paid_through, cancel_at_period_end)
       VALUES ($1, $2, $3, 'polar', $4, $5, $6, $7)
       ON CONFLICT (provider, provider_subscription_id) DO UPDATE
         SET plan = EXCLUDED.plan,
             state = EXCLUDED.state,
             paid_through = COALESCE(EXCLUDED.paid_through, subscriptions.paid_through),
             cancel_at_period_end = EXCLUDED.cancel_at_period_end,
             updated_at = now()`,
      [
        crypto.randomUUID(),
        userId,
        plan,
        live.id,
        live.status === "paused" ? "paused" : "active",
        PAID.has(live.status) ? live.current_period_end : null,
        live.cancel_at_period_end,
      ],
    );
  });
}

/** Move a Polar subscription to another plan now; Polar prorates the difference. */
export async function switchPolar(userId: string, row: SubscriptionRow, plan: PaidPlan) {
  const product = products().find((p) => p.slug === plan);
  if (!product)
    throw new Error(`No Polar product for ${plan} (POLAR_PRODUCT_${plan.toUpperCase()})`);
  await updateSubscriptions(polarClient())(row.provider_subscription_id, {
    product_id: product.productId,
    proration_behavior: "prorate",
  });
  await syncPolar(userId);
}

/** Cancel at the end of the paid period; the customer portal can undo it. */
export async function cancelPolar(userId: string, row: SubscriptionRow) {
  await updateSubscriptions(polarClient())(row.provider_subscription_id, {
    cancel_at_period_end: true,
  });
  await syncPolar(userId);
}

/** Whether a plan can be bought by card: Polar is configured and the plan has a product. */
export const polarPlans = (): PaidPlan[] =>
  polarConfigured() ? products().map((p) => p.slug) : [];

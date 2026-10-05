/**
 * The billing page's server functions. Two providers bill the same plans in
 * USD: Polar by card (./polar.ts) and Mayarin in crypto (./mayarin.ts). A
 * user has at most one live subscription (./subscriptions.ts); switching
 * plans stays with the provider that bills it.
 */
import { createServerFn } from "@tanstack/react-start";
import {
  isPaidPlan,
  PAID_PLANS,
  type PaidPlan,
  type PlanId,
  planById,
  TRIAL_DAYS,
} from "../lib/plans";
import { getPool } from "./db";
import {
  endMayarin,
  mayarinConfigured,
  mayarinStatus,
  mayarinTestnet,
  startMayarin,
} from "./mayarin";
import { cancelPolar, polarPlans, polarSandbox, switchPolar, syncPolar } from "./polar";
import { authMiddleware } from "./session";
import { liveRow, type Provider, type SubscriptionState, withUserLock } from "./subscriptions";

export type Billing = {
  /** The plan the account is on: its live subscription's, else the trial. */
  plan: PlanId;
  trialEndsAt: string;
  /** Whether the account may use paid-plan access right now. */
  paid: boolean;
  subscription: {
    plan: PaidPlan;
    provider: Provider;
    state: SubscriptionState;
    paidThrough: string | null;
    /** Card plans cancelled from the portal or here: active until `paidThrough`. */
    cancelAtPeriodEnd: boolean;
    nextBillingAt: string | null;
    amount: string;
  } | null;
  /** A crypto cycle still owed, with Mayarin's hosted pay page. */
  due: { url: string; dueAt: string | null; total: string; overdue: boolean } | null;
  /** Which plans each provider can sell right now. */
  methods: { card: PaidPlan[]; crypto: PaidPlan[] };
  /** Any configured provider is on its test network. */
  testMode: boolean;
};

export const getBilling = createServerFn({ method: "GET" })
  .middleware([authMiddleware])
  .handler(async ({ context }): Promise<Billing> => {
    const { rows: users } = await getPool().query<{ created_at: Date }>(
      "SELECT created_at FROM users WHERE id = $1",
      [context.user.id],
    );
    const createdAt = users[0]?.created_at ?? new Date();
    const card = polarPlans();
    const coin: PaidPlan[] = mayarinConfigured() ? [...PAID_PLANS] : [];
    const base = {
      trialEndsAt: new Date(createdAt.getTime() + TRIAL_DAYS * 86_400_000).toISOString(),
      methods: { card, crypto: coin },
      testMode: (card.length > 0 && polarSandbox()) || (coin.length > 0 && mayarinTestnet()),
    };

    const row = await liveRow(context.user.id);
    if (!row) return { ...base, plan: "starter", paid: false, subscription: null, due: null };

    const paidThrough = row.paid_through?.toISOString() ?? null;
    const paid = row.paid_through !== null && row.paid_through.getTime() > Date.now();
    if (row.provider === "polar") {
      const { price, period } = planById(row.plan);
      return {
        ...base,
        plan: row.plan,
        paid,
        subscription: {
          plan: row.plan,
          provider: "polar",
          state: row.state,
          paidThrough,
          cancelAtPeriodEnd: row.cancel_at_period_end,
          nextBillingAt: row.cancel_at_period_end || row.state !== "active" ? null : paidThrough,
          amount: `${price}${period}`,
        },
        due: null,
      };
    }

    const status = await mayarinStatus(row);
    return {
      ...base,
      plan: row.plan,
      paid: status.paidThrough !== null && status.paidThrough.getTime() > Date.now(),
      subscription: {
        plan: row.plan,
        provider: "mayarin",
        state: status.state,
        paidThrough: status.paidThrough?.toISOString() ?? null,
        cancelAtPeriodEnd: false,
        nextBillingAt: status.nextBillingAt,
        amount: `${status.amount}/month`,
      },
      due: status.due,
    };
  });

/**
 * Start a crypto plan, or switch the live subscription to another plan.
 * New card plans go through Polar's checkout from the browser instead
 * (`authClient.checkout`). A crypto switch cancels the old Mayarin
 * subscription and starts the new one when the paid period ends (Mayarin's
 * price and interval are immutable); a card switch prorates at Polar.
 */
export const subscribe = createServerFn({ method: "POST" })
  .middleware([authMiddleware])
  .validator((input: { plan: string }) => {
    if (!isPaidPlan(input.plan)) throw new Error(`Unknown plan ${input.plan}`);
    return { plan: input.plan };
  })
  .handler(async ({ data, context }) => {
    const user = context.user;
    const current = await liveRow(user.id);
    if (current?.plan === data.plan) return { ok: true as const };
    if (current?.provider === "polar") {
      await switchPolar(user.id, current, data.plan);
      return { ok: true as const };
    }

    await withUserLock(user.id, async (db) => {
      // Re-read under the lock: Mayarin's create is not idempotent.
      const locked = await liveRow(user.id, db);
      if (locked?.plan === data.plan) return;
      if (locked && locked.provider !== "mayarin")
        throw new Error("Plan changed elsewhere; reload");
      let startAt: Date | null = null;
      if (locked) {
        await endMayarin(locked, db);
        if (locked.paid_through && locked.paid_through.getTime() > Date.now()) {
          startAt = locked.paid_through;
        }
      }
      await startMayarin(user, data.plan, startAt, db);
    });
    return { ok: true as const };
  });

/** Stop future billing. Access already paid for runs until `paid_through`. */
export const cancelSubscription = createServerFn({ method: "POST" })
  .middleware([authMiddleware])
  .handler(async ({ context }) => {
    const current = await liveRow(context.user.id);
    if (current?.provider === "polar") await cancelPolar(context.user.id, current);
    else if (current) {
      await withUserLock(context.user.id, async (db) => {
        const locked = await liveRow(context.user.id, db);
        if (locked?.provider === "mayarin") await endMayarin(locked, db);
      });
    }
    return { ok: true as const };
  });

/** Back from Polar's checkout: read the new subscription now rather than wait for its webhook. */
export const refreshCardBilling = createServerFn({ method: "POST" })
  .middleware([authMiddleware])
  .handler(async ({ context }) => {
    if (polarPlans().length > 0) await syncPolar(context.user.id);
    return { ok: true as const };
  });

/**
 * The dashboard's plans, mirrored from the landing page
 * (landing/src/lib/components/Pricing.svelte). Prices are USD; Mayarin
 * invoices them in USD and the subscriber pays in stablecoins or ETH.
 */
export const PAID_PLANS = ["pro", "growth"] as const;
export type PaidPlan = (typeof PAID_PLANS)[number];
export type PlanId = "starter" | PaidPlan | "scale";

export type Plan = {
  id: PlanId;
  name: string;
  tagline: string;
  /** Decimal string in USD for paid plans, null for free or custom. */
  usd: string | null;
  price: string;
  period: string;
  features: string[];
  support: string;
};

export const PLANS: Plan[] = [
  {
    id: "starter",
    name: "Starter",
    tagline: "Try the core API free for 7 days.",
    usd: null,
    price: "Free",
    period: "7-day trial",
    features: [
      "10,000 requests / month",
      "Core markets across 5 asset classes",
      "Read-only MCP server for AI agents",
    ],
    support: "Community support",
  },
  {
    id: "pro",
    name: "Pro",
    tagline: "For builders and early teams.",
    usd: "20.00",
    price: "$20",
    period: "/month",
    features: [
      "250,000 requests / month",
      "Full universes: crypto top 500, S&P 1500, Nasdaq-100, tokenized stocks, every Perp",
      "Candles, reference rates and perpetual funding",
      "Read-only MCP server for AI agents",
    ],
    support: "Email support",
  },
  {
    id: "growth",
    name: "Growth",
    tagline: "For products in production.",
    usd: "99.00",
    price: "$99",
    period: "/month",
    features: [
      "2,000,000 requests / month",
      "Everything in Pro",
      "Source provenance behind every quote",
      "Central-bank reference rates, including Asia, Southeast Asia, Gulf FX",
    ],
    support: "Priority support",
  },
  {
    id: "scale",
    name: "Scale",
    tagline: "For platforms and data teams.",
    usd: null,
    price: "Custom",
    period: "",
    features: [
      "Custom API volume",
      "Everything in Growth",
      "Custom universes and sources",
      "Self-hosted deployment",
    ],
    support: "Dedicated support channel",
  },
];

export const TRIAL_DAYS = 7;

export function planById(id: PlanId): Plan {
  const plan = PLANS.find((p) => p.id === id);
  if (!plan) throw new Error(`Unknown plan ${id}`);
  return plan;
}

export function isPaidPlan(id: string): id is PaidPlan {
  return (PAID_PLANS as readonly string[]).includes(id);
}

/** Whole days left in a trial ending at `endsAt`, never negative. */
export function trialDaysLeft(endsAt: string): number {
  return Math.max(0, Math.ceil((Date.parse(endsAt) - Date.now()) / 86_400_000));
}

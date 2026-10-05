import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import clsx from "clsx";
import { type ReactNode, useEffect, useState } from "react";
import { Fields, Panel, Tag } from "../../components/ui";
import { authClient } from "../../lib/auth-client";
import { CONTACT_URL } from "../../lib/links";
import {
  type PaidPlan,
  PLANS,
  type Plan,
  type PlanId,
  planById,
  trialDaysLeft,
} from "../../lib/plans";
import { billingQuery } from "../../lib/queries";
import {
  type Billing,
  cancelSubscription,
  refreshCardBilling,
  subscribe,
} from "../../server/billing";

export const Route = createFileRoute("/_app/billing")({
  head: () => ({ meta: [{ title: "Plans & billing — Undrly" }] }),
  // Polar's checkout returns here with ?checkout_id=…
  validateSearch: (search: Record<string, unknown>): { checkout_id?: string } =>
    typeof search.checkout_id === "string" ? { checkout_id: search.checkout_id } : {},
  loader: ({ context }) => context.queryClient.ensureQueryData(billingQuery()),
  component: BillingPage,
});

const RAILS = ["USDC", "USDG", "EURC", "PYUSD", "ETH"];
const CHAINS = ["Base", "Arbitrum", "Ethereum", "Robinhood Chain", "Arc"];

/** Better Auth client calls answer `{ error }` instead of throwing. */
async function unwrap(call: Promise<{ error: { message?: string } | null }>): Promise<void> {
  const { error } = await call;
  if (error) throw new Error(error.message ?? "Payment provider unavailable");
}

function BillingPage() {
  const { user } = Route.useRouteContext();
  const { checkout_id: checkoutId } = Route.useSearch();
  const queryClient = useQueryClient();
  const { data: billing } = useQuery(billingQuery());
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["billing"] });

  // A new crypto plan bills now: its pay page opens as soon as Mayarin issues
  // the first invoice (its billing run is once a minute). Switches bill later.
  const [awaitingInvoice, setAwaitingInvoice] = useState(false);
  const crypto = useMutation({
    mutationFn: (plan: PlanId) => subscribe({ data: { plan } }),
    onSuccess: () => {
      if (!billing?.subscription) setAwaitingInvoice(true);
      return refresh();
    },
  });
  const payUrl = billing?.due?.url;
  useEffect(() => {
    if (awaitingInvoice && payUrl) window.location.assign(payUrl);
  }, [awaitingInvoice, payUrl]);
  // Redirects to Polar's hosted checkout; the page comes back with ?checkout_id.
  const card = useMutation({
    mutationFn: (plan: PaidPlan) => unwrap(authClient.checkout({ slug: plan })),
  });
  const portal = useMutation({ mutationFn: () => unwrap(authClient.customer.portal()) });
  const cancel = useMutation({ mutationFn: () => cancelSubscription(), onSuccess: refresh });
  const error = crypto.error ?? card.error ?? portal.error ?? cancel.error;
  const busy = crypto.isPending || card.isPending || cancel.isPending;

  useCheckoutReturn(checkoutId, billing);

  if (!billing) return null;
  return (
    <div className="pr-8 max-md:pr-4">
      <div>
        <div className="flex items-center gap-3">
          <h1 className="text-[29px] leading-none font-sans tracking-[-0.03em] max-md:text-[34px]">
            Plans & billing
          </h1>
          {billing.testMode && <Tag>Test mode</Tag>}
        </div>
        <p className="mt-3 max-w-150 text-[15px] leading-[1.6] text-muted">
          Prices are in US dollars. Pay by card and it renews on its own each month, or pay a
          monthly invoice in stablecoins from any wallet, anywhere.
        </p>
      </div>

      {awaitingInvoice && (
        <p className="mt-6 flex items-center gap-3 border border-line-strong px-5 py-3 text-[14px] text-muted">
          <span className="size-2 shrink-0 animate-pulse bg-forest" aria-hidden="true" />
          {payUrl
            ? "Opening the payment page…"
            : "Preparing your invoice. The payment page opens on its own in under a minute."}
        </p>
      )}
      {checkoutId && billing.subscription?.provider !== "polar" && (
        <p className="mt-6 flex items-center gap-3 border border-line-strong px-5 py-3 text-[14px] text-muted">
          <span className="size-2 shrink-0 animate-pulse bg-forest" aria-hidden="true" />
          Confirming your payment…
        </p>
      )}
      {error && (
        <p className="mt-6 border border-[#e3876b55] px-5 py-3 text-[14px] text-down">
          {error instanceof Error ? error.message : String(error)}
        </p>
      )}

      <div className="mt-8 grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-4 max-lg:grid-cols-1">
        <Panel title="Current plan">
          <Fields rows={planRows(billing, user.email)} />
        </Panel>
        <Panel title="Payment" className="flex flex-col">
          <PaymentState
            billing={billing}
            email={user.email}
            cancelling={cancel.isPending}
            onCancel={() => cancel.mutate()}
            portalPending={portal.isPending}
            onPortal={() => portal.mutate()}
          />
        </Panel>
      </div>

      <div className="mt-6 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
        {PLANS.map((plan) => (
          <PlanCard
            key={plan.id}
            plan={plan}
            billing={billing}
            cryptoPending={crypto.isPending && crypto.variables === plan.id}
            cardPending={card.isPending && card.variables === plan.id}
            disabled={busy}
            onCard={() => card.mutate(plan.id as PaidPlan)}
            onCrypto={() => crypto.mutate(plan.id)}
          />
        ))}
      </div>

      <HowPaying billing={billing} />
    </div>
  );
}

/**
 * Back from Polar's checkout: read the subscription from Polar until it shows
 * up (the webhook may not have arrived yet), then drop ?checkout_id.
 */
function useCheckoutReturn(checkoutId: string | undefined, billing: Billing | undefined) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const found = billing?.subscription?.provider === "polar";
  const [tries, setTries] = useState(0);
  useEffect(() => {
    if (!checkoutId) return;
    if (found || tries >= 6) {
      void navigate({ to: "/billing", search: {}, replace: true });
      return;
    }
    const timer = setTimeout(
      async () => {
        await refreshCardBilling().catch(() => {});
        await queryClient.invalidateQueries({ queryKey: ["billing"] });
        setTries((n) => n + 1);
      },
      tries === 0 ? 0 : 3_000,
    );
    return () => clearTimeout(timer);
  }, [checkoutId, found, tries, navigate, queryClient]);
}

function planRows(billing: Billing, email: string): [string, ReactNode][] {
  const plan = planById(billing.plan);
  const sub = billing.subscription;
  if (!sub) {
    return [
      ["Plan", `${plan.name} · free trial`],
      ["Status", <TrialStatus key="status" endsAt={billing.trialEndsAt} />],
      ["Trial ends", formatDate(billing.trialEndsAt)],
    ];
  }
  return [
    ["Plan", `${plan.name} · ${sub.amount}`],
    ["Status", <SubscriptionStatus key="status" billing={billing} />],
    ["Pays by", sub.provider === "polar" ? "Card" : "Crypto invoice"],
    ["Paid through", sub.paidThrough ? formatDate(sub.paidThrough) : "Not yet paid"],
    [sub.provider === "polar" ? "Renews" : "Next invoice", nextBilling(sub.nextBillingAt)],
    ["Receipts sent to", email],
  ];
}

function PaymentState(props: {
  billing: Billing;
  email: string;
  cancelling: boolean;
  onCancel: () => void;
  portalPending: boolean;
  onPortal: () => void;
}) {
  const { billing, email } = props;
  const sub = billing.subscription;
  if (!sub) {
    return (
      <p className="px-5 py-5 text-[14px] leading-[1.6] text-muted">
        You're on the free trial. Choose a plan below to keep access after{" "}
        {formatDate(billing.trialEndsAt)}.
      </p>
    );
  }

  const canCancel = sub.state === "active" && !sub.cancelAtPeriodEnd;
  const cancelButton = canCancel ? (
    <CancelButton pending={props.cancelling} onCancel={props.onCancel} />
  ) : null;

  if (sub.provider === "polar") {
    let message: string;
    if (sub.cancelAtPeriodEnd) {
      message = `Cancelled. Access stays on until ${
        sub.paidThrough ? formatDate(sub.paidThrough) : "the paid period ends"
      }; resume any time from the billing portal.`;
    } else if (!billing.paid) {
      message =
        "The last charge didn't go through. Update your card in the billing portal and Polar retries it.";
    } else {
      message = `Your card is charged ${sub.amount.split("/")[0]} on ${
        sub.nextBillingAt ? formatDate(sub.nextBillingAt) : "the renewal date"
      }. Receipts and invoices go to ${email}.`;
    }
    return (
      <PaymentBody message={message}>
        <button
          type="button"
          disabled={props.portalPending}
          onClick={props.onPortal}
          className={clsx(ghostButton, "w-auto disabled:opacity-50")}
        >
          {props.portalPending ? "Opening…" : "Manage billing ↗"}
        </button>
        {cancelButton}
      </PaymentBody>
    );
  }

  if (billing.due) {
    return (
      <div className="flex flex-1 flex-col justify-between gap-6 px-5 py-5">
        <div>
          <p className="text-[13px] text-muted">
            {billing.due.overdue ? "Overdue" : "Amount due"}
            {billing.due.dueAt && ` · due ${formatDate(billing.due.dueAt)}`}
          </p>
          <p className="mt-2 text-[34px] leading-none tracking-[-0.03em] tabular">
            {billing.due.total}
          </p>
          <p className="mt-3 text-[13px] leading-[1.6] text-faint">
            The same pay link was emailed to {email}. Access starts once the payment clears.
          </p>
        </div>
        <div className="flex items-center justify-between gap-4">
          <a
            href={billing.due.url}
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-2 bg-[#dbe4d3] px-4 py-2.5 text-[14px] text-[#1a2317] transition-colors hover:bg-[#eff5e9]"
          >
            Pay with crypto <span aria-hidden="true">↗</span>
          </a>
          {cancelButton}
        </div>
      </div>
    );
  }

  if (sub.state === "active" && !billing.paid) {
    return (
      <PaymentBody
        message={
          <span className="flex items-center gap-3">
            <span className="size-2 shrink-0 animate-pulse bg-forest" aria-hidden="true" />
            Preparing your first invoice. It appears here and in your inbox within a minute.
          </span>
        }
      >
        <span />
        {cancelButton}
      </PaymentBody>
    );
  }

  return (
    <PaymentBody
      message={
        sub.state === "active"
          ? `Nothing owed. The next invoice is emailed to ${email} on ${
              sub.nextBillingAt ? formatDate(sub.nextBillingAt) : "the next billing date"
            }.`
          : `Billing is ${sub.state}. Access stays on until ${
              sub.paidThrough ? formatDate(sub.paidThrough) : "the paid period ends"
            }.`
      }
    >
      <span />
      {cancelButton}
    </PaymentBody>
  );
}

function PaymentBody({ message, children }: { message: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-1 flex-col justify-between gap-6 px-5 py-5">
      <p className="text-[14px] leading-[1.6] text-muted">{message}</p>
      <div className="flex items-center justify-between gap-4">{children}</div>
    </div>
  );
}

function CancelButton({ pending, onCancel }: { pending: boolean; onCancel: () => void }) {
  const [confirming, setConfirming] = useState(false);
  return (
    <button
      type="button"
      disabled={pending}
      onClick={() => (confirming ? onCancel() : setConfirming(true))}
      onBlur={() => setConfirming(false)}
      className={clsx(
        "text-[13px] transition-colors disabled:opacity-50",
        confirming ? "text-down" : "text-faint hover:text-ink",
      )}
    >
      {pending ? "Cancelling…" : confirming ? "Confirm cancel" : "Cancel subscription"}
    </button>
  );
}

function PlanCard({
  plan,
  billing,
  cardPending,
  cryptoPending,
  disabled,
  onCard,
  onCrypto,
}: {
  plan: Plan;
  billing: Billing;
  cardPending: boolean;
  cryptoPending: boolean;
  disabled: boolean;
  onCard: () => void;
  onCrypto: () => void;
}) {
  const current = billing.plan === plan.id;
  const live = billing.subscription !== null && billing.subscription.state !== "cancelled";

  let action: ReactNode;
  if (current) {
    action = <ActionLabel>Current plan</ActionLabel>;
  } else if (plan.id === "starter") {
    action = <ActionLabel>Included with every account</ActionLabel>;
  } else if (plan.id === "scale") {
    action = (
      <a href={CONTACT_URL} className={ghostButton}>
        Contact us
      </a>
    );
  } else if (live) {
    // A switch stays with the provider that bills the live subscription.
    action = (
      <ConfirmButton
        primary={plan.id === "pro"}
        disabled={disabled}
        pending={cryptoPending}
        label={`Switch to ${plan.name}`}
        confirm={`Confirm · ${plan.price}${plan.period}`}
        onConfirm={onCrypto}
      />
    );
  } else {
    const byCard = billing.methods.card.includes(plan.id as PaidPlan);
    const byCrypto = billing.methods.crypto.includes(plan.id as PaidPlan);
    action =
      byCard || byCrypto ? (
        <div className="grid gap-2">
          {byCard && (
            <button
              type="button"
              disabled={disabled}
              onClick={onCard}
              className={clsx(primaryButton, "disabled:opacity-50")}
            >
              {cardPending ? "Opening checkout…" : "Pay with card"}
            </button>
          )}
          {byCrypto && (
            <ConfirmButton
              primary={!byCard}
              disabled={disabled}
              pending={cryptoPending}
              label="Pay with crypto"
              confirm={`Confirm · ${plan.price}${plan.period} in crypto`}
              onConfirm={onCrypto}
            />
          )}
        </div>
      ) : (
        <ActionLabel>Payments not set up</ActionLabel>
      );
  }

  return (
    <article
      className={clsx(
        "flex flex-col border bg-panel p-7",
        current ? "border-forest" : "border-line-strong",
      )}
    >
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-[24px] tracking-[-0.02em]">{plan.name}</h2>
        {current && <Tag tone="up">Current</Tag>}
      </div>
      <p className="mt-2 text-[14px] text-muted">{plan.tagline}</p>
      <p className="mt-8 mb-7 text-[38px] leading-none tracking-[-0.03em]">
        {plan.price}
        {plan.period && (
          <span className="ml-2 text-[14px] tracking-normal text-muted">{plan.period}</span>
        )}
      </p>
      {action}
      <ul className="mt-7 grid gap-3 border-t border-line-strong pt-6">
        {plan.features.map((feature, i) => (
          <li
            key={feature}
            className={clsx(
              "flex gap-3 text-[14px] leading-normal",
              i === 0 ? "text-ink" : "text-[#c9cfc5]",
            )}
          >
            <Check />
            {feature}
          </li>
        ))}
      </ul>
      {/* Support sits at the card bottom so it lines up across plans. */}
      <div className="mt-auto pt-6">
        <p className="flex gap-3 border-t border-line-strong pt-5 text-[14px] text-[#c9cfc5]">
          <Check />
          {plan.support}
        </p>
      </div>
    </article>
  );
}

/** Two clicks, no dialog: the first arms the button, the second commits. */
function ConfirmButton(props: {
  primary: boolean;
  disabled: boolean;
  pending: boolean;
  label: string;
  confirm: string;
  onConfirm: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  return (
    <button
      type="button"
      disabled={props.disabled}
      onClick={() => {
        if (!confirming) return setConfirming(true);
        setConfirming(false);
        props.onConfirm();
      }}
      onBlur={() => setConfirming(false)}
      className={clsx(
        props.primary || confirming ? primaryButton : ghostButton,
        "disabled:opacity-50",
      )}
    >
      {props.pending ? "Starting…" : confirming ? props.confirm : props.label}
    </button>
  );
}

function HowPaying({ billing }: { billing: Billing }) {
  const ways: [string, string][] = [];
  if (billing.methods.card.length > 0) {
    ways.push([
      "Card",
      "Visa, Mastercard, Amex and more through Polar, our merchant of record: it runs checkout, sales tax and VAT, and receipts. Renews each month on its own; switch plans any time, prorated.",
    ]);
  }
  if (billing.methods.crypto.length > 0) {
    ways.push([
      "Crypto",
      `A USD invoice by email each month, paid in ${RAILS.join(", ")} on ${CHAINS.join(", ")}. No card or bank account needed. Processed by Mayarin.`,
    ]);
  }
  if (ways.length === 0) return null;
  return (
    <section className="mt-6 border border-line" aria-label="How paying works">
      <h2 className="border-b border-line bg-panel px-5 py-3.5 font-mono text-[11px] tracking-[0.1em] text-faint uppercase">
        How paying works
      </h2>
      <div
        className={clsx(
          "grid divide-x divide-line max-md:grid-cols-1 max-md:divide-x-0 max-md:divide-y",
          ways.length === 2 ? "grid-cols-2" : "grid-cols-1",
        )}
      >
        {ways.map(([title, body]) => (
          <div key={title} className="px-5 py-5">
            <p className="text-[15px] text-ink">{title}</p>
            <p className="mt-1.5 text-[13px] leading-[1.6] text-muted">{body}</p>
          </div>
        ))}
      </div>
      <p className="border-t border-line px-5 py-3 text-[12px] text-faint">
        Cancel any time; access runs to the end of the paid month.
      </p>
    </section>
  );
}

const primaryButton =
  "flex h-11 w-full items-center justify-center bg-[#dbe4d3] px-4 text-[14px] text-[#1a2317] transition-colors hover:bg-[#eff5e9]";
const ghostButton =
  "flex h-11 w-full items-center justify-center border border-line-strong px-4 text-[14px] text-ink transition-colors hover:bg-card";

function ActionLabel({ children }: { children: ReactNode }) {
  return (
    <span className="flex h-11 w-full items-center justify-center border border-dashed border-line-strong px-4 text-[13px] text-faint">
      {children}
    </span>
  );
}

function SubscriptionStatus({ billing }: { billing: Billing }) {
  const sub = billing.subscription;
  if (!sub) return null;
  if (sub.state === "cancelled") return <Tag>Cancelled</Tag>;
  if (sub.state === "paused") return <Tag>Paused</Tag>;
  if (sub.cancelAtPeriodEnd) return <Tag>Cancels at period end</Tag>;
  if (billing.due?.overdue) return <Tag tone="down">Overdue</Tag>;
  if (billing.paid) return <Tag tone="up">Active</Tag>;
  return sub.provider === "polar" ? (
    <Tag tone="down">Payment failed</Tag>
  ) : (
    <Tag>Awaiting payment</Tag>
  );
}

function TrialStatus({ endsAt }: { endsAt: string }) {
  const days = trialDaysLeft(endsAt);
  return days > 0 ? (
    <Tag tone="up">{`${days} day${days === 1 ? "" : "s"} left`}</Tag>
  ) : (
    <Tag tone="down">Ended</Tag>
  );
}

/**
 * Mayarin moves `nextBillingAt` forward only once a cycle is fully issued and
 * emailed; until then it still names the cycle being billed.
 */
function nextBilling(iso: string | null): string {
  if (!iso) return "Never";
  return Date.parse(iso) > Date.now() ? formatDate(iso) : "Processing";
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    timeZone: "UTC",
  });
}

function Check() {
  return (
    <svg
      className="mt-0.5 size-4 shrink-0 text-[#9aa596]"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="m3 8.5 3 3 7-7" />
    </svg>
  );
}

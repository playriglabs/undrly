import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useRouter } from "@tanstack/react-router";
import type { v1 } from "@undrly/contracts";
import clsx from "clsx";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { ClassIcon } from "../../components/ClassIcon";
import { type ChartMode, TradingChart } from "../../components/TradingChart";
import { Age, Change, ErrorState, Tag } from "../../components/ui";
import { RANGES, type Range } from "../../lib/api";
import { useCrumb } from "../../lib/crumb";
import {
  changeLabel,
  displayName,
  formatDecimal,
  formatTimestamp,
  STATUS_LABEL,
  subjectClass,
  unitCode,
} from "../../lib/format";
import { API_ORIGIN, DOCS_URL } from "../../lib/links";
import { marketDetailQuery, seriesQuery } from "../../lib/queries";

type Search = { unit?: string; range?: Range };

export const Route = createFileRoute("/_app/markets/$id")({
  validateSearch: (search: Record<string, unknown>): Search => {
    const unit = typeof search.unit === "string" ? search.unit : undefined;
    const range = Object.hasOwn(RANGES, String(search.range)) ? (search.range as Range) : undefined;
    return { ...(unit ? { unit } : {}), ...(range ? { range } : {}) };
  },
  loaderDeps: ({ search }) => ({
    unit: search.unit,
    range: search.range ?? "24H",
  }),
  loader: async ({ context, params, deps }) => {
    await Promise.all([
      context.queryClient.ensureQueryData(marketDetailQuery(params.id, deps.unit)),
      context.queryClient.ensureQueryData(seriesQuery(params.id, deps.unit, deps.range)),
    ]);
  },
  component: MarketPage,
});

function MarketPage() {
  const { id } = Route.useParams();
  const { unit, range = "24H" } = Route.useSearch();
  const { data } = useQuery(marketDetailQuery(id, unit));
  const market = data?.market;
  useCrumb(market?.ok ? displayName(market.data.subject.name) : null);
  if (!data || !market) return null;
  const { quote, explain, derivatives } = data;

  if (!market.ok) {
    return (
      <div className="pr-8 max-md:pr-4">
        <Toolbar request={null} />
        <div className="mt-6">
          <ErrorState title="This market has no quote right now" message={market.message} />
        </div>
      </div>
    );
  }
  const m = market.data;
  const candidate = explain.ok ? explain.data.candidates[0] : undefined;
  // Readable when the API finds one (`/v1/quote/NVDA`), else ids.
  const request = data.query.ok
    ? `/v1/quote/${data.query.data.query}`
    : `/v1/quote/${m.subject.id}?unit=${m.unit.id}`;

  return (
    // The layout bleeds to the right edge (for Explore's table); this page keeps its gutter.
    <div className="grid items-start gap-6 pr-8 max-md:pr-4 xl:grid-cols-[minmax(0,1fr)_400px]">
      <div className="min-w-0">
        <Toolbar
          request={request}
          title={
            data.query.ok && data.query.data.readable
              ? data.query.data.query
              : `${displayName(m.subject.name)}/${unitCode(m.unit)}`
          }
        />

        <div className="mt-5 grid gap-3 md:grid-cols-[minmax(0,1.3fr)_repeat(3,minmax(0,1fr))]">
          <div className="flex min-w-0 items-center gap-4 py-2">
            <ClassIcon subject={m.subject} size={52} />
            <div className="min-w-0">
              <h1 className="truncate font-sans text-[22px] leading-tight text-ink">
                {displayName(m.subject.name)}
              </h1>
              <p className="mt-1 truncate text-[12px] tracking-[0.03em] text-faint uppercase">
                Undrly price in {unitCode(m.unit)} · {subjectClass(m.subject)}
              </p>
            </div>
          </div>
          <StatCard label="Price">
            <span className="truncate text-[18px] text-ink tabular">{formatDecimal(m.price)}</span>
            <CopyButton value={m.price} label="Copy price" />
          </StatCard>
          <StatCard label={changeLabel(m.statistics)}>
            <Change percent={m.statistics?.changePercent} className="text-[18px]" />
          </StatCard>
          <StatCard label="Status">
            <span className="flex items-center gap-2">
              <Tag tone={m.freshness === "fresh" ? "up" : "down"}>{m.freshness}</Tag>
              <span className="text-[13px] text-muted">
                {m.marketStatus ? STATUS_LABEL[m.marketStatus] : "Reference"}
              </span>
            </span>
          </StatCard>
        </div>

        <ChartCard id={id} unit={unit} range={range} />
      </div>

      <div className="space-y-4 xl:pt-13">
        <UseCard request={request} />

        <Section title="Market metadata" icon="list">
          <Fields
            rows={[
              ["ID", <Mono key="id" value={m.subject.id} copy />],
              ["Quote", <Mono key="unit" value={unitCode(m.unit)} copy={m.unit.id} />],
              ["Price type", m.priceType],
              ["Basis", m.basis],
              ...(quote.ok && quote.data.basis === "venue"
                ? ([["Venue", quote.data.venue.name]] as Row[])
                : []),
              ...(quote.ok
                ? ([
                    ["Method", <Mono key="method" value={quote.data.aggregation.method} />],
                    ["Observations", quote.data.aggregation.eligibleObservations],
                    ["Spread", quote.data.spreadBps ? `${quote.data.spreadBps} bps` : "—"],
                  ] as Row[])
                : []),
            ]}
          />
        </Section>

        <Section title="Data coverage" icon="calendar">
          <Fields
            rows={[
              ["Session", m.marketStatus ? STATUS_LABEL[m.marketStatus] : "Published reference"],
              [
                "Statistics",
                m.statistics
                  ? m.statistics.window === "session"
                    ? "Exchange session"
                    : "Rolling 24h"
                  : "—",
              ],
              ["Last quote", formatTimestamp(m.asOf)],
              ["Age", <Age key="age" iso={m.asOf} />],
              ...(m.statistics
                ? ([
                    ["High", formatDecimal(m.statistics.high)],
                    ["Low", formatDecimal(m.statistics.low)],
                    ["Volume", m.statistics.volume ? formatDecimal(m.statistics.volume) : "—"],
                  ] as Row[])
                : []),
            ]}
          />
        </Section>

        {derivatives?.ok ? <DerivativesSection d={derivatives.data} /> : null}

        <IdentitySection subject={m.subject} candidate={candidate} />
      </div>
    </div>
  );
}

type Row = [string, ReactNode];

function Toolbar({ request, title }: { request: string | null; title?: string }) {
  const router = useRouter();
  return (
    <div className="flex items-center justify-between gap-3">
      <button
        type="button"
        onClick={() =>
          window.history.length > 1 ? router.history.back() : router.navigate({ to: "/" })
        }
        className="flex items-center gap-2 border border-line-strong px-3 py-1.5 text-[14px] text-ink transition-colors hover:bg-card"
      >
        <span aria-hidden="true">←</span> Back
      </button>
      {request ? (
        <div className="flex items-center gap-2">
          <CopyButton
            value={typeof window === "undefined" ? request : window.location.href}
            label="Copy link"
            boxed
          />
          <CodeMenu request={request} title={title ?? request} />
        </div>
      ) : null}
    </div>
  );
}

/** The request as a copyable curl, one argument per line. */
const curlOf = (request: string) =>
  `curl -s \\\n  "${API_ORIGIN}${request}" \\\n  -H "Authorization: Bearer $UNDRLY_API_KEY"`;

/** "‹/› Code": a dropdown with the market's request as curl, to copy. */
function CodeMenu({ request, title }: { request: string; title: string }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent | KeyboardEvent) => {
      if (
        e instanceof KeyboardEvent ? e.key === "Escape" : !root.current?.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", close);
    };
  }, [open]);
  const curl = curlOf(request);
  return (
    <div ref={root} className="relative">
      <button
        type="button"
        aria-expanded={open}
        aria-haspopup="dialog"
        onClick={() => setOpen((o) => !o)}
        className="inline-flex items-center gap-2 bg-[#dbe4d3] px-3 py-1.5 text-[14px] text-[#1a2317] transition-colors hover:bg-[#eff5e9]"
      >
        <span aria-hidden="true">‹/›</span> Code
        <svg
          className={clsx("size-3.5 transition-transform", open && "rotate-180")}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.75"
          aria-hidden="true"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      {open ? (
        <div
          role="dialog"
          aria-label="Request code"
          className="absolute top-full right-0 z-30 mt-2 w-[min(560px,calc(100vw-2rem))] border border-line-strong bg-panel shadow-[0_16px_40px_rgba(0,0,0,0.45)]"
        >
          <div className="flex items-center justify-between gap-4 px-5 pt-4 pb-3">
            <p className="truncate text-[15px] text-ink">{title}</p>
            <CopyButton value={curl} label="Copy request" text="Copy" />
          </div>
          <pre className="mx-5 overflow-x-auto border border-line bg-paper px-4 py-3 font-mono text-[12.5px] leading-6 text-ink">
            <code>
              {"curl -s \\\n  "}
              <span className="text-up">{`"${API_ORIGIN}${request}"`}</span>
              {" \\\n  -H "}
              <span className="text-up">{'"Authorization: Bearer $UNDRLY_API_KEY"'}</span>
            </code>
          </pre>
          <div className="mt-4 flex items-center gap-2 border-t border-line px-5 py-3">
            <a
              href={DOCS_URL}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-2 border border-line-strong px-3 py-1.5 text-[13px] text-ink transition-colors hover:bg-card"
            >
              Documentation <span aria-hidden="true">↗</span>
            </a>
            <Link
              to="/api-keys"
              className="inline-flex items-center gap-2 border border-line-strong px-3 py-1.5 text-[13px] text-ink transition-colors hover:bg-card"
            >
              Get an API key
            </Link>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function StatCard({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0 border border-line bg-panel px-4 py-3">
      <p className="text-[13px] text-muted">{label}</p>
      <div className="mt-1.5 flex items-center justify-between gap-2">{children}</div>
    </div>
  );
}

function ChartCard({ id, unit, range }: { id: string; unit: string | undefined; range: Range }) {
  const { data: series, isFetching } = useQuery(seriesQuery(id, unit, range));
  const [mode, setMode] = useState<ChartMode>("candles");
  const [now] = useState(() => new Date());
  const bars = series?.ok ? series.data.bars : [];
  const candles = series?.ok && series.data.kind === "candles";
  return (
    <section className="mt-5 border border-line bg-panel">
      <div className="flex items-center justify-between gap-4 px-4 pt-4">
        <div className="flex items-center gap-3">
          <fieldset className="flex border border-line-strong">
            <legend className="sr-only">Chart type</legend>
            {(["candles", "line"] as const).map((m) => (
              <button
                key={m}
                type="button"
                aria-pressed={mode === m}
                aria-label={m === "candles" ? "Candles" : "Line"}
                title={m === "candles" ? "Candles" : "Line"}
                disabled={m === "candles" && !candles}
                onClick={() => setMode(m)}
                className={clsx(
                  "flex size-8 items-center justify-center transition-colors disabled:opacity-30",
                  mode === m && (m === "line" || candles)
                    ? "bg-card text-ink"
                    : "text-faint hover:text-ink",
                )}
              >
                <svg
                  className="size-4"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  aria-hidden="true"
                >
                  <path
                    d={
                      m === "candles"
                        ? "M7 4v3M7 17v3M5 7h4v10H5zM17 3v4M17 15v4M15 7h4v8h-4z"
                        : "M3 17l5-6 4 3 4-6 5 5"
                    }
                  />
                </svg>
              </button>
            ))}
          </fieldset>
          <span className="text-[13px] text-muted">
            {series?.ok && series.data.kind === "history"
              ? "Published values"
              : series?.ok && series.data.kind === "cross"
                ? "Cross rate (derived)"
                : "Price"}
          </span>
        </div>
        <span className="text-[12px] text-faint">
          {candles ? `${RANGES[range].interval} bars` : ""}
        </span>
      </div>
      <div className={clsx("px-2 pt-3 pb-2 transition-opacity", isFetching && "opacity-60")}>
        {bars.length > 1 ? (
          <TradingChart
            bars={bars}
            mode={candles ? mode : "line"}
            intraday={RANGES[range].interval !== "1d"}
          />
        ) : (
          <div className="flex h-95 items-center justify-center text-[14px] text-faint max-md:h-65">
            {series && !series.ok ? series.message : "Not enough data for this range"}
          </div>
        )}
      </div>
      <div className="flex items-center justify-between gap-4 border-t border-line px-3">
        <nav className="flex" aria-label="Chart range">
          {(Object.keys(RANGES) as Range[]).map((r) => (
            <Link
              key={r}
              from={Route.fullPath}
              search={(prev) => ({
                ...prev,
                range: r === "24H" ? undefined : r,
              })}
              replace
              resetScroll={false}
              className={clsx(
                "relative px-3 py-3 text-[13px] transition-colors",
                r === range
                  ? "text-ink after:absolute after:inset-x-2 after:bottom-0 after:h-px after:bg-forest"
                  : "text-faint hover:text-ink",
              )}
            >
              {r}
            </Link>
          ))}
        </nav>
        <span className="pr-2 text-[13px] text-ink tabular" suppressHydrationWarning>
          {now.toLocaleTimeString("en-US", {
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
            hourCycle: "h23",
            timeZone: "UTC",
          })}{" "}
          UTC
        </span>
      </div>
    </section>
  );
}

/** Where a developer goes next: the same market over the API. */
function UseCard({ request }: { request: string }) {
  return (
    <section className="relative overflow-hidden border border-[#3a4a33] bg-[linear-gradient(135deg,#1d2a19_0%,#10160f_60%)] p-6">
      <h2 className="text-[24px] leading-tight tracking-[-0.01em] text-ink">Use this market</h2>
      <p className="mt-2 text-[14px] leading-[1.6] text-[#c2ccbd]">
        The same quote, with its unit and freshness, from one request.
      </p>
      <pre className="mt-4 overflow-x-auto border border-[#3a4a33] bg-[#0a0d09cc] px-3 py-2.5 font-mono text-[11.5px] leading-[1.7] text-[#dfe3dc]">
        GET {request.replace(/undrly:[a-z]+:[0-9a-z]+([0-9a-z]{6})/g, "…$1")}
      </pre>
      <div className="mt-4">
        <CopyButton
          value={curlOf(request)}
          label="Copy request"
          text="Copy request"
          boxed
          primary
        />
      </div>
    </section>
  );
}

const ICONS: Record<string, string> = {
  list: "M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01",
  calendar: "M4 6h16v14H4zM4 10h16M8 3v4M16 3v4",
  graph:
    "M6 6m-2 0a2 2 0 1 0 4 0a2 2 0 1 0-4 0M18 18m-2 0a2 2 0 1 0 4 0a2 2 0 1 0-4 0M18 6m-2 0a2 2 0 1 0 4 0a2 2 0 1 0-4 0M8 6h8M7.5 7.5l9 9M18 8v8",
  perp: "M19.5 12a7.5 7.5 0 0 1-13.4 4.6M4.5 12a7.5 7.5 0 0 1 13.4-4.6M18.2 3.8v3.9h-3.9M5.8 20.2v-3.9h3.9M9 13.5l2-2 1.6 1.6L15 10.5",
};

function Section({
  title,
  icon,
  children,
}: {
  title: string;
  icon: keyof typeof ICONS;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(true);
  return (
    <section>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex w-full items-center gap-2.5 px-1 py-2.5 text-[14px] text-ink"
      >
        <svg
          className="size-4 text-forest"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          aria-hidden="true"
        >
          <path d={ICONS[icon]} />
        </svg>
        <span className="flex-1 text-left">{title}</span>
        <svg
          className={clsx("size-4 text-faint transition-transform", open && "rotate-180")}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          aria-hidden="true"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      {open ? <div className="border border-line bg-panel pt-1">{children}</div> : null}
    </section>
  );
}

function Fields({ rows }: { rows: Row[] }) {
  return (
    <dl>
      {rows.map(([label, value]) => (
        <div
          // Labels repeat only for relationships, whose values are names.
          key={typeof value === "string" ? `${label}:${value}` : label}
          className="flex items-center justify-between gap-6 px-5 py-2.5 text-[14px]"
        >
          <dt className="shrink-0 text-muted first-letter:uppercase">{label}</dt>
          <dd className="min-w-0 truncate text-right text-ink tabular">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function DerivativesSection({ d }: { d: v1.DerivativesV1 }) {
  return (
    <Section title="Perpetual" icon="perp">
      <Fields
        rows={[
          ["Mark", formatDecimal(d.markPrice)],
          ["Index", d.indexPrice ? formatDecimal(d.indexPrice) : "—"],
          ["Funding", d.fundingRate ? `${d.fundingRate} / ${d.fundingIntervalHours ?? "?"}h` : "—"],
          ["Open interest", d.openInterest ? formatDecimal(d.openInterest) : "—"],
        ]}
      />
    </Section>
  );
}

function Mono({ value, copy }: { value: string; copy?: boolean | string }) {
  return (
    <span className="inline-flex max-w-full items-center gap-2">
      <span className="truncate font-mono text-[12.5px]" title={value}>
        {value}
      </span>
      {copy ? (
        <CopyButton value={typeof copy === "string" ? copy : value} label={`Copy ${value}`} />
      ) : null}
    </span>
  );
}

function CopyButton(props: {
  value: string;
  label: string;
  text?: string;
  boxed?: boolean;
  primary?: boolean;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      aria-label={props.label}
      title={props.label}
      onClick={() => {
        void navigator.clipboard.writeText(props.value).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        });
      }}
      className={clsx(
        "inline-flex shrink-0 items-center gap-2 transition-colors",
        props.boxed ? "px-3 py-1.5 text-[14px]" : "text-faint hover:text-ink",
        props.boxed && !props.primary && "border border-line-strong text-ink hover:bg-card",
        props.primary && "bg-[#dbe4d3] text-[#1a2317] hover:bg-[#eff5e9]",
      )}
    >
      {props.text ? (copied ? "Copied" : props.text) : null}
      {props.text ? null : copied ? (
        <svg
          className="size-3.5 text-up"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          aria-hidden="true"
        >
          <path d="m5 12 5 5 9-10" />
        </svg>
      ) : (
        <svg
          className="size-3.5"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          aria-hidden="true"
        >
          <rect x="8" y="8" width="12" height="12" />
          <path d="M16 8V4H4v12h4" />
        </svg>
      )}
    </button>
  );
}

type Candidate = v1.ExplainV1["candidates"][number];

/**
 * Identifiers and relationships, where they say something: not for crypto or
 * forex, and never TRADES_ON (where it trades is the quote's venue, shown above).
 */
function IdentitySection({
  subject,
  candidate,
}: {
  subject: v1.PriceSubjectV1;
  candidate: Candidate | undefined;
}) {
  const cls = subject.kind === "instrument" ? subject.class : null;
  if (!candidate || cls === "crypto_asset" || cls === "fx" || subject.kind === "currency") {
    return null;
  }
  const relationships = uniqueRelationships(candidate.relationships).filter(
    (r) => r.relationshipType !== "TRADES_ON",
  );
  const rows: Row[] = [
    ...candidate.identifiers.map(
      (i) => [i.namespace.toUpperCase(), <Mono key={i.namespace} value={i.value} copy />] as Row,
    ),
    ...relationships
      .slice(0, 8)
      .map(
        (r) =>
          [
            r.relationshipType.replaceAll("_", " ").toLowerCase(),
            displayName(r.object.name),
          ] as Row,
      ),
  ];
  if (rows.length === 0) return null;
  return (
    <Section title="Identity" icon="graph">
      <Fields rows={rows} />
    </Section>
  );
}

/** One row per (type, object): explain lists an edge once per provenance. */
function uniqueRelationships(rows: Candidate["relationships"]) {
  const seen = new Set<string>();
  return rows.filter((r) => {
    const key = `${r.relationshipType}:${r.object.id}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

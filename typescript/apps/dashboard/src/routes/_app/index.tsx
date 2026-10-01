import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { v1 } from "@undrly/contracts";
import clsx from "clsx";
import { useEffect, useState } from "react";
import { ClassIcon } from "../../components/ClassIcon";
import { Sparkline } from "../../components/Sparkline";
import { Change, ErrorState } from "../../components/ui";
import { PAGE_SIZE } from "../../lib/api";
import {
  CLASS_LABEL,
  CLASS_ORDER,
  changeBasis,
  direction,
  displayName,
  formatDecimal,
  subjectClass,
  unitCode,
} from "../../lib/format";
import { marketCountsQuery, marketsQuery } from "../../lib/queries";

type Search = { class?: string; q?: string; page?: number };

/** `class` in the URL is a comma-separated list, like the API's. */
const classesOf = (value: string | undefined): v1.InstrumentClass[] =>
  (value ?? "")
    .split(",")
    .filter((c): c is v1.InstrumentClass =>
      (v1.INSTRUMENT_CLASSES as readonly string[]).includes(c),
    );

export const Route = createFileRoute("/_app/")({
  validateSearch: (search: Record<string, unknown>): Search => {
    const classes = classesOf(typeof search.class === "string" ? search.class : undefined);
    const q = typeof search.q === "string" ? search.q.trim().slice(0, 64) : "";
    const page = Number(search.page);
    return {
      ...(classes.length ? { class: classes.join(",") } : {}),
      ...(q ? { q } : {}),
      ...(Number.isInteger(page) && page > 1 ? { page } : {}),
    };
  },
  loaderDeps: ({ search }) => ({
    classes: classesOf(search.class),
    q: search.q ?? "",
    page: search.page ?? 1,
  }),
  loader: ({ context, deps }) => context.queryClient.ensureQueryData(marketsQuery(deps)),
  head: () => ({ meta: [{ title: "Explore — Undrly" }] }),
  component: Explore,
});

function Explore() {
  const search = Route.useSearch();
  const filter = {
    classes: classesOf(search.class),
    q: search.q ?? "",
    page: search.page ?? 1,
  };
  const { data: result, isFetching } = useQuery(marketsQuery(filter));

  return (
    <div className="grid items-start gap-6 lg:grid-cols-[300px_minmax(0,1fr)]">
      <Filters classes={filter.classes} q={filter.q} />
      {result && !result.ok ? (
        <ErrorState title="Markets are unavailable" message={result.message} />
      ) : result?.ok ? (
        <MarketTable data={result.data} page={filter.page} fetching={isFetching} />
      ) : null}
    </div>
  );
}

function Filters({ classes, q }: { classes: v1.InstrumentClass[]; q: string }) {
  const navigate = useNavigate({ from: Route.fullPath });
  const { data } = useQuery(marketCountsQuery());
  const counts = data?.ok ? data.data : [];
  const [text, setText] = useState(q);
  const [open, setOpen] = useState(true);

  useEffect(() => setText(q), [q]);
  // Quick search follows typing, a beat later.
  useEffect(() => {
    if (text.trim() === q) return;
    const t = setTimeout(() => {
      void navigate({
        search: (prev) => ({
          ...prev,
          q: text.trim() || undefined,
          page: undefined,
        }),
        replace: true,
      });
    }, 250);
    return () => clearTimeout(t);
  }, [text, q, navigate]);

  const toggle = (cls: v1.InstrumentClass) => {
    const next = classes.includes(cls) ? classes.filter((c) => c !== cls) : [...classes, cls];
    void navigate({
      search: (prev) => ({
        ...prev,
        class: next.length ? next.join(",") : undefined,
        page: undefined,
      }),
    });
  };

  return (
    <aside className="lg:sticky lg:top-22">
      <label className="flex items-center gap-2.5 border border-line-strong bg-panel px-3 py-2.5 focus-within:border-forest">
        <svg
          className="size-4 shrink-0 text-faint"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          aria-hidden="true"
        >
          <circle cx="11" cy="11" r="6.5" />
          <path d="m16 16 4.5 4.5" />
        </svg>
        <input
          type="search"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Quick search…"
          aria-label="Quick search"
          className="min-w-0 flex-1 bg-transparent text-[14px] text-ink placeholder:text-faint focus:outline-none"
        />
      </label>

      <div className="mt-5 border border-line bg-panel">
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <span className="text-[14px] text-ink">Filters</span>
          {classes.length || q ? (
            <Link to="/" search={{}} className="text-[13px] text-forest hover:text-ink">
              Clear
            </Link>
          ) : null}
        </div>
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          className="flex w-full items-center justify-between px-4 py-3 text-[14px] text-ink"
        >
          Asset type
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
        {open ? (
          <ul className="px-2 pb-3">
            {CLASS_ORDER.map((cls) => {
              const n = counts.find((c) => c.class === cls)?.count ?? 0;
              if (n === 0) return null;
              const on = classes.includes(cls);
              return (
                <li key={cls}>
                  <label className="flex cursor-pointer items-center gap-3 px-2 py-2 text-[15px] hover:bg-card">
                    <input
                      type="checkbox"
                      checked={on}
                      onChange={() => toggle(cls)}
                      className="peer sr-only"
                    />
                    <span
                      className={clsx(
                        "flex size-4.5 items-center justify-center border peer-focus-visible:outline-2 peer-focus-visible:outline-forest",
                        on ? "border-forest bg-forest text-[#1a2317]" : "border-line-strong",
                      )}
                      aria-hidden="true"
                    >
                      {on ? (
                        <svg
                          className="size-3"
                          aria-hidden="true"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="3"
                        >
                          <path d="m5 12 5 5 9-10" />
                        </svg>
                      ) : null}
                    </span>
                    <span className={clsx("flex-1", on ? "text-ink" : "text-muted")}>
                      {CLASS_LABEL[cls]}
                    </span>
                    <span className="border border-line-strong px-1.5 text-[11px] text-faint tabular">
                      {n}
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>
        ) : null}
      </div>
    </aside>
  );
}

const th =
  "px-4 py-3.5 text-left text-[12px] font-normal tracking-[0.06em] whitespace-nowrap text-faint uppercase";

function MarketTable({
  data,
  page,
  fetching,
}: {
  data: v1.MarketsV1;
  page: number;
  fetching: boolean;
}) {
  const pages = Math.max(1, Math.ceil(data.total / PAGE_SIZE));
  return (
    <div
      className={clsx(
        "border border-r-0 border-line bg-panel transition-opacity",
        fetching && "opacity-80",
      )}
    >
      <div className="overflow-x-auto">
        <table className="w-full min-w-245 border-collapse">
          <thead className="border-b border-line">
            <tr>
              <th className={th}>Name</th>
              <th className={th}>Asset</th>
              <th className={th}>Quote</th>
              <th className={clsx(th, "text-right")}>Price</th>
              <th className={clsx(th, "text-right")}>24h %</th>
              <th className={clsx(th, "text-right")}>24h change</th>
              <th className={clsx(th, "pl-6")}>24h trend</th>
            </tr>
          </thead>
          <tbody>
            {data.markets.length === 0 ? (
              <tr>
                <td colSpan={7} className="px-4 py-16 text-center text-[14px] text-faint">
                  No markets match these filters.
                </td>
              </tr>
            ) : (
              data.markets.map((row) => <Row key={`${row.subject.id}:${row.unit.id}`} row={row} />)
            )}
          </tbody>
        </table>
      </div>
      <div className="flex items-center justify-between border-t border-line px-4 py-3 text-[13px] text-muted">
        <span className="tabular">{data.total.toLocaleString("en-US")} results</span>
        {pages > 1 ? (
          <div className="flex items-center gap-2">
            <PageLink page={page - 1} disabled={page <= 1} label="Previous" />
            <span className="px-2 text-[12px] text-faint tabular">
              {page} / {pages}
            </span>
            <PageLink page={page + 1} disabled={page >= pages} label="Next" />
          </div>
        ) : null}
      </div>
    </div>
  );
}

function PageLink({ page, disabled, label }: { page: number; disabled: boolean; label: string }) {
  const cls = "border border-line-strong px-3 py-1.5 transition-colors";
  if (disabled) return <span className={clsx(cls, "text-faint opacity-50")}>{label}</span>;
  return (
    <Link
      from={Route.fullPath}
      search={(prev) => ({ ...prev, page: page > 1 ? page : undefined })}
      className={clsx(cls, "text-ink hover:bg-card")}
    >
      {label}
    </Link>
  );
}

function Row({ row }: { row: v1.MarketsRowV1 }) {
  const m = row.market;
  const stats = m?.statistics;
  const unit = unitCode(row.unit);
  const dir = direction(stats?.change);
  return (
    <tr className="group relative border-b border-line last:border-b-0 hover:bg-[#ffffff06]">
      <td className="px-4 py-3">
        <Link
          to="/markets/$id"
          params={{ id: row.subject.id }}
          search={{ unit: row.unit.id }}
          className="flex items-center gap-3.5 after:absolute after:inset-0"
        >
          <ClassIcon subject={row.subject} />
          <span className="min-w-0">
            <span className="block max-w-75 truncate text-[15px] text-ink group-hover:text-forest">
              {displayName(row.subject.name)}
            </span>
            <span className="mt-0.5 block text-[11.5px] tracking-[0.03em] text-faint uppercase">
              Undrly price in {unit}
              {m && m.freshness === "stale" ? " · stale" : ""}
            </span>
          </span>
        </Link>
      </td>
      <td className="px-4 py-3 text-[14px] text-muted">{subjectClass(row.subject)}</td>
      <td className="px-4 py-3 text-[14px] text-muted">{unit}</td>
      <td className="px-4 py-3 text-right text-[15px] tabular">
        {m ? formatDecimal(m.price) : <span className="text-faint">—</span>}
      </td>
      <td className="px-4 py-3 text-right text-[14px]">
        <Change percent={stats?.changePercent} />
        {changeBasis(stats) ? (
          <span
            className="ml-1.5 border border-line-strong px-1 font-mono text-[10px] text-faint uppercase"
            title="Change since the previous publication, not the last 24 hours"
          >
            {changeBasis(stats)}
          </span>
        ) : null}
      </td>
      <td
        className={clsx(
          "px-4 py-3 text-right text-[14px] tabular",
          dir === "up" ? "text-up" : dir === "down" ? "text-down" : "text-faint",
        )}
      >
        {stats ? formatDecimal(stats.change) : "—"}
      </td>
      <td className="py-3 pr-4 pl-6">
        <Sparkline values={row.sparkline} />
      </td>
    </tr>
  );
}

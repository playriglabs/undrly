import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { v1 } from "@undrly/contracts";
import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { ClassIcon } from "../../components/ClassIcon";
import { Sparkline } from "../../components/Sparkline";
import { Change, ErrorState } from "../../components/ui";
import { type MarketsSort, SORT_COLUMNS, type SortColumn } from "../../lib/api";
import {
  CLASS_LABEL,
  CLASS_ORDER,
  changeBasis,
  direction,
  displayName,
  formatPrice,
  subjectClass,
  unitCode,
} from "../../lib/format";
import { marketCountsQuery, marketsQuery } from "../../lib/queries";

type Search = { class?: string; q?: string; sort?: SortColumn; order?: "asc" | "desc" };

/** The table's order from the URL: none (by name), or a column and its direction. */
const sortOf = (search: Search): MarketsSort =>
  search.sort ? { column: search.sort, order: search.order ?? "asc" } : null;

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
    const sort = (SORT_COLUMNS as readonly unknown[]).includes(search.sort)
      ? (search.sort as SortColumn)
      : undefined;
    return {
      ...(classes.length ? { class: classes.join(",") } : {}),
      ...(q ? { q } : {}),
      ...(sort ? { sort, ...(search.order === "desc" ? { order: "desc" as const } : {}) } : {}),
    };
  },
  loaderDeps: ({ search }) => ({
    classes: classesOf(search.class),
    q: search.q ?? "",
    sort: sortOf(search),
  }),
  loader: ({ context, deps }) => context.queryClient.ensureInfiniteQueryData(marketsQuery(deps)),
  head: () => ({ meta: [{ title: "Explore — Undrly" }] }),
  component: Explore,
});

function Explore() {
  const search = Route.useSearch();
  const filter = { classes: classesOf(search.class), q: search.q ?? "", sort: sortOf(search) };
  const query = useInfiniteQuery(marketsQuery(filter));
  const pages = query.data?.pages ?? [];
  const failed = pages.find((p) => !p.ok);
  const loaded = pages.flatMap((p) => (p.ok ? p.data.markets : []));
  const first = pages[0];

  return (
    // -mb-10: main's 64px bottom padding becomes 24px here, matching the top.
    <div className="grid items-start gap-6 lg:-mb-10 lg:grid-cols-[300px_minmax(0,1fr)]">
      <Filters classes={filter.classes} q={filter.q} />
      {failed && !failed.ok ? (
        <ErrorState title="Markets are unavailable" message={failed.message} />
      ) : first?.ok ? (
        <MarketTable
          sort={filter.sort}
          rows={loaded}
          total={first.data.total}
          fetching={query.isFetching && !query.isFetchingNextPage}
          hasMore={query.hasNextPage}
          loadingMore={query.isFetchingNextPage}
          loadMore={() => void query.fetchNextPage()}
        />
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

/**
 * A sortable column header: low to high, then high to low, then back to
 * the name order. Markets without the value stay last either way.
 */
function SortHeader({
  column,
  label,
  sort,
  align,
}: {
  column: SortColumn;
  label: string;
  sort: MarketsSort;
  align: "left" | "right";
}) {
  const navigate = useNavigate({ from: Route.fullPath });
  const active = sort?.column === column ? sort.order : null;
  const next: MarketsSort =
    active === null
      ? { column, order: "asc" }
      : active === "asc"
        ? { column, order: "desc" }
        : null;
  const title =
    active === "asc" ? "Sorted low to high" : active === "desc" ? "Sorted high to low" : "Sort";
  return (
    <th
      className={clsx(th, align === "right" ? "text-right" : "pl-6")}
      aria-sort={active === "asc" ? "ascending" : active === "desc" ? "descending" : "none"}
    >
      <button
        type="button"
        title={title}
        onClick={() =>
          void navigate({
            search: (prev) => ({
              ...prev,
              sort: next?.column,
              order: next?.order === "desc" ? "desc" : undefined,
            }),
            replace: true,
          })
        }
        className={clsx(
          "inline-flex items-center gap-1.5 uppercase transition-colors hover:text-ink",
          align === "right" && "flex-row-reverse",
          active && "text-ink",
        )}
      >
        {label}
        <svg
          className="size-3 shrink-0"
          viewBox="0 0 12 12"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.3"
          aria-hidden="true"
        >
          <path
            d="M3.5 4.5 6 2l2.5 2.5"
            className={active === "desc" ? "opacity-25" : active === "asc" ? "" : "opacity-50"}
          />
          <path
            d="M3.5 7.5 6 10l2.5-2.5"
            className={active === "asc" ? "opacity-25" : active === "desc" ? "" : "opacity-50"}
          />
        </svg>
      </button>
    </th>
  );
}

function MarketTable({
  sort,
  rows,
  total,
  fetching,
  hasMore,
  loadingMore,
  loadMore,
}: {
  sort: MarketsSort;
  rows: v1.MarketsRowV1[];
  total: number;
  fetching: boolean;
  hasMore: boolean;
  loadingMore: boolean;
  loadMore: () => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const sentinel = useRef<HTMLTableRowElement>(null);
  // Load the next page when the last rows come into view inside the table.
  useEffect(() => {
    const el = sentinel.current;
    if (!el || !hasMore) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting) && !loadingMore) loadMore();
      },
      { root: scroller.current, rootMargin: "400px 0px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasMore, loadingMore, loadMore]);

  return (
    <div
      className={clsx(
        // Fills the viewport below the header (64px) with the page's 24px
        // above and below, so rows scroll inside the table, not the page.
        "flex flex-col border border-r-0 border-line bg-panel transition-opacity lg:h-[calc(100dvh-7rem)]",
        fetching && "opacity-80",
      )}
    >
      <div ref={scroller} className="overflow-auto lg:min-h-0 lg:flex-1">
        <table className="w-full min-w-245 border-collapse">
          <thead className="sticky top-0 z-10 bg-panel shadow-[inset_0_-1px_0_var(--color-line)]">
            <tr>
              <th className={th}>Name</th>
              <th className={th}>Asset</th>
              <th className={th}>Quote</th>
              <SortHeader column="price" label="Price" sort={sort} align="right" />
              <SortHeader column="changePercent" label="24h %" sort={sort} align="right" />
              <SortHeader column="change" label="24h change" sort={sort} align="right" />
              <SortHeader column="trend" label="24h trend" sort={sort} align="left" />
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={7} className="px-4 py-16 text-center text-[14px] text-faint">
                  No markets match these filters.
                </td>
              </tr>
            ) : (
              rows.map((row) => <Row key={`${row.subject.id}:${row.unit.id}`} row={row} />)
            )}
            {hasMore ? (
              <tr ref={sentinel}>
                <td colSpan={7} className="px-4 py-5 text-center text-[13px] text-faint">
                  {loadingMore ? "Loading more markets…" : ""}
                </td>
              </tr>
            ) : null}
          </tbody>
        </table>
      </div>
      <div className="flex shrink-0 items-center justify-between border-t border-line px-4 py-3 text-[13px] text-muted">
        <span className="tabular">
          {rows.length.toLocaleString("en-US")} of {total.toLocaleString("en-US")} markets
        </span>
        {loadingMore ? <span className="text-faint">Loading…</span> : null}
      </div>
    </div>
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
        {m ? formatPrice(m.price) : <span className="text-[14px] text-faint">No price</span>}
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
        {stats ? formatPrice(stats.change) : "No data"}
      </td>
      <td className="py-3 pr-4 pl-6">
        <Sparkline values={row.sparkline} />
      </td>
    </tr>
  );
}

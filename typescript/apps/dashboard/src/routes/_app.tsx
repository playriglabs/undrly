import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createFileRoute,
  Link,
  Outlet,
  redirect,
  useNavigate,
  useRouter,
  useRouterState,
} from "@tanstack/react-router";
import type { v1 } from "@undrly/contracts";
import clsx from "clsx";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { ClassGlyph } from "../components/ClassIcon";
import { UserAvatar } from "../components/UserAvatar";
import { authClient } from "../lib/auth-client";
import { CrumbContext } from "../lib/crumb";
import { CLASS_LABEL, CLASS_ORDER } from "../lib/format";
import { DOCS_URL } from "../lib/links";
import { marketCountsQuery } from "../lib/queries";
import { getSessionUser } from "../server/session";

/** Everything under this layout needs a session; the server functions enforce it too. */
export const Route = createFileRoute("/_app")({
  beforeLoad: async ({ location }) => {
    const user = await getSessionUser();
    if (!user) throw redirect({ to: "/login", search: { redirect: location.href } });
    return { user };
  },
  loader: ({ context }) => context.queryClient.ensureQueryData(marketCountsQuery()),
  component: AppLayout,
});

function AppLayout() {
  const [collapsed, setCollapsed] = useState(false);
  const [crumb, setCrumb] = useState<string | null>(null);
  return (
    <CrumbContext.Provider value={setCrumb}>
      <div className="flex min-h-screen">
        <Sidebar collapsed={collapsed} />
        <div className="min-w-0 flex-1">
          <AppHeader crumb={crumb} onToggle={() => setCollapsed((c) => !c)} />
          <main className="pt-6 pb-16 pl-8 max-md:pl-4">
            <Outlet />
          </main>
        </div>
      </div>
    </CrumbContext.Provider>
  );
}

function Sidebar({ collapsed }: { collapsed: boolean }) {
  const { user } = Route.useRouteContext();
  const { data } = useQuery(marketCountsQuery());
  const counts: v1.MarketsV1["counts"] = data?.ok ? data.data : [];
  const count = (cls: v1.InstrumentClass) => counts.find((c) => c.class === cls)?.count ?? 0;
  const total = counts.reduce((n, c) => n + c.count, 0);
  const search = useRouterState({
    select: (s) => s.location.search as { class?: string },
  });
  const path = useRouterState({ select: (s) => s.location.pathname });
  const activeClass = path === "/" ? (search.class ?? "") : null;

  return (
    <aside
      className={clsx(
        "sticky top-0 flex h-screen w-75 shrink-0 flex-col border-r border-line bg-panel max-lg:hidden",
        collapsed && "hidden!",
      )}
    >
      <Link to="/" className="flex h-16 items-center gap-2.5 px-6" aria-label="Undrly markets">
        <span className="font-display text-[30px] leading-none font-medium tracking-[-0.5px]">
          undrly
        </span>
      </Link>

      <nav className="flex-1 overflow-y-auto px-3 pt-2" aria-label="Markets">
        <SideLink
          to="/"
          search={{}}
          active={activeClass === ""}
          icon="all"
          label="All markets"
          n={total}
        />
        <div className="mx-3 my-3 border-t border-line" />
        {CLASS_ORDER.filter((c) => count(c) > 0).map((c) => (
          <SideLink
            key={c}
            to="/"
            search={{ class: c }}
            active={activeClass === c}
            icon={c}
            label={CLASS_LABEL[c]}
            n={count(c)}
          />
        ))}
      </nav>

      <div className="px-3 pb-2">
        <a
          href={DOCS_URL}
          target="_blank"
          rel="noreferrer"
          className="flex items-center gap-3 px-3 py-2.5 text-[15px] text-muted transition-colors hover:bg-card hover:text-ink"
        >
          <DocsIcon /> Documentation
        </a>
        <Link
          to="/api-keys"
          className="flex items-center gap-3 px-3 py-2.5 text-[15px] text-muted transition-colors hover:bg-card hover:text-ink data-[status=active]:bg-card data-[status=active]:text-ink"
        >
          <KeyIcon /> API keys
        </Link>
      </div>
      <UserBlock email={user.email} />
    </aside>
  );
}

function SideLink(props: {
  to: "/";
  search: { class?: v1.InstrumentClass };
  active: boolean;
  icon: v1.InstrumentClass | "all";
  label: string;
  n: number;
}) {
  return (
    <Link
      to={props.to}
      search={props.search}
      className={clsx(
        "flex items-center gap-3 px-3 py-2.5 text-[15px] transition-colors",
        props.active ? "bg-card text-ink" : "text-muted hover:bg-card hover:text-ink",
      )}
    >
      <ClassGlyph cls={props.icon} className="size-4.5 shrink-0" />
      <span className="flex-1">{props.label}</span>
      <span className="text-[13px] text-faint tabular">{props.n.toLocaleString("en-US")}</span>
    </Link>
  );
}

function UserBlock({ email }: { email: string }) {
  const router = useRouter();
  const queryClient = useQueryClient();
  const signOut = async () => {
    await authClient.signOut();
    queryClient.clear();
    await router.invalidate();
    await router.navigate({ to: "/login" });
  };
  return (
    <div className="border-t border-line px-5 pt-4 pb-5">
      <div className="flex items-center gap-2.5">
        <UserAvatar seed={email} />
        <span className="min-w-0">
          <span className="block truncate text-[14px] text-ink" title={email}>
            {email}
          </span>
          <span className="block text-[12px] text-faint">7D Trial</span>
        </span>
      </div>
      <div className="mt-4 grid grid-cols-2 gap-2">
        <Link
          to="/api-keys"
          className="grid h-10 grid-cols-[1fr_auto_1fr] items-center bg-[#dbe4d3] px-3 text-[13px] text-[#1a2317] transition-colors hover:bg-[#eff5e9]"
        >
          <span className="mr-2 justify-self-end">
            <KeyIcon />
          </span>
          <span>API keys</span>
          <span aria-hidden="true" />
        </Link>
        <button
          type="button"
          onClick={signOut}
          className="flex h-10 items-center justify-center border border-line-strong px-3 text-[13px] text-ink transition-colors hover:bg-card"
        >
          Sign out
        </button>
      </div>
    </div>
  );
}

function AppHeader({ crumb, onToggle }: { crumb: string | null; onToggle: () => void }) {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const section = path.startsWith("/api-keys") ? "API keys" : "Explore";
  return (
    <header className="sticky top-0 z-30 flex h-16 items-center justify-between gap-6 border-b border-line bg-[#090b0ad9] px-8 backdrop-blur-xl max-md:px-4">
      <div className="flex min-w-0 items-center gap-4">
        <button
          type="button"
          onClick={onToggle}
          className="p-1.5 text-muted transition-colors hover:text-ink max-lg:hidden"
          aria-label="Toggle sidebar"
        >
          <svg
            className="size-5"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            aria-hidden="true"
          >
            <rect x="3.5" y="4.5" width="17" height="15" />
            <path d="M9 4.5v15" />
          </svg>
        </button>
        <span className="h-5 w-px bg-line max-lg:hidden" aria-hidden="true" />
        <nav className="flex min-w-0 items-center gap-2 text-[15px]" aria-label="Breadcrumb">
          {crumb ? (
            <>
              <Link to="/" className="text-muted hover:text-ink">
                {section}
              </Link>
              <span className="text-faint" aria-hidden="true">
                ›
              </span>
              <span className="truncate text-ink">{crumb}</span>
            </>
          ) : (
            <span className="text-ink">{section}</span>
          )}
        </nav>
      </div>
      <SearchBox />
    </header>
  );
}

/** Global search: ⌘K focuses, Enter opens Explore filtered by the text. */
function SearchBox() {
  const navigate = useNavigate();
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        input.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  return (
    <form
      aria-label="Search markets"
      className="flex w-75 items-center gap-2.5 border border-line-strong bg-panel px-3 py-2 focus-within:border-forest max-md:w-auto max-md:flex-1"
      onSubmit={(e) => {
        e.preventDefault();
        const q = input.current?.value.trim() ?? "";
        void navigate({ to: "/", search: q ? { q } : {} });
        input.current?.blur();
      }}
    >
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
        ref={input}
        type="search"
        placeholder="Search markets…"
        aria-label="Search markets"
        className="min-w-0 flex-1 bg-transparent text-[14px] text-ink placeholder:text-faint focus:outline-none"
      />
      <kbd className="border border-line-strong px-1.5 font-mono text-[11px] text-faint max-md:hidden">
        ⌘K
      </kbd>
    </form>
  );
}

function DocsIcon(): ReactNode {
  return (
    <svg
      className="size-4 shrink-0"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
    >
      <path d="M5 4.5h9.5L19 9v10.5H5z" />
      <path d="M14.5 4.5V9H19M8.5 13h7M8.5 16.5h5" />
    </svg>
  );
}

function KeyIcon(): ReactNode {
  return (
    <svg
      className="size-4 shrink-0"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
    >
      <circle cx="8" cy="15" r="4" />
      <path d="m11 12 9-9M17 6l2.5 2.5M14.5 8.5 17 11" />
    </svg>
  );
}

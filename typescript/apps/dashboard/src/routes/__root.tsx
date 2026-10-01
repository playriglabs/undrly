import type { QueryClient } from "@tanstack/react-query";
import {
  createRootRouteWithContext,
  HeadContent,
  Link,
  Outlet,
  Scripts,
} from "@tanstack/react-router";
import type { ReactNode } from "react";
import styles from "../styles.css?url";

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()({
  head: () => ({
    meta: [
      { charSet: "utf-8" },
      { name: "viewport", content: "width=device-width, initial-scale=1" },
      { title: "Undrly — Markets" },
    ],
    links: [
      { rel: "stylesheet", href: styles },
      {
        rel: "preload",
        href: "/fonts/Pilat-Book.woff2",
        as: "font",
        type: "font/woff2",
        crossOrigin: "anonymous",
      },
    ],
  }),
  component: RootComponent,
  errorComponent: ({ error, reset }) => (
    <RootDocument>
      <div className="flex min-h-screen items-center justify-center px-4">
        <div className="max-w-[440px] border border-line bg-panel p-8">
          <p className="text-[28px] leading-tight">Something went wrong</p>
          <p className="mt-3 font-mono text-[12px] break-words text-faint">
            {error instanceof Error ? error.message : String(error)}
          </p>
          <div className="mt-6 flex gap-2">
            <button
              type="button"
              onClick={() => window.location.reload()}
              className="bg-[#dbe4d3] px-4 py-2 text-[14px] text-[#1a2317] hover:bg-[#eff5e9]"
            >
              Reload
            </button>
            <button
              type="button"
              onClick={reset}
              className="border border-line-strong px-4 py-2 text-[14px] text-ink hover:bg-card"
            >
              Try again
            </button>
          </div>
        </div>
      </div>
    </RootDocument>
  ),
  notFoundComponent: () => (
    <div className="px-10 py-24">
      <h1 className="text-4xl">Not found</h1>
      <Link to="/" className="mt-6 inline-block text-forest">
        Back to markets
      </Link>
    </div>
  ),
});

function RootComponent() {
  return (
    <RootDocument>
      <Outlet />
    </RootDocument>
  );
}

function RootDocument({ children }: Readonly<{ children: ReactNode }>) {
  return (
    <html lang="en">
      <head>
        <HeadContent />
      </head>
      <body>
        {children}
        <Scripts />
      </body>
    </html>
  );
}

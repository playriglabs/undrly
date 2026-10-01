import { QueryClient } from "@tanstack/react-query";
import { createRouter } from "@tanstack/react-router";
import { setupRouterSsrQueryIntegration } from "@tanstack/react-router-ssr-query";
import { routeTree } from "./routeTree.gen";

export function getRouter() {
  // One QueryClient per request on the server, one per tab in the browser.
  const queryClient = new QueryClient({
    defaultOptions: { queries: { staleTime: 10_000, refetchOnWindowFocus: false } },
  });
  const router = createRouter({
    routeTree,
    context: { queryClient },
    scrollRestoration: true,
    defaultPreload: "intent",
    // Query owns freshness; the router never caches loader data on its own.
    defaultPreloadStaleTime: 0,
  });
  // Dehydrates the queries loaders fetched during SSR and wraps the app in QueryClientProvider.
  setupRouterSsrQueryIntegration({ router, queryClient });
  return router;
}

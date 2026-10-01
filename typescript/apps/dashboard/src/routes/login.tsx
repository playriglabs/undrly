import { createFileRoute, redirect } from "@tanstack/react-router";
import { AuthForm, safeRedirect } from "../components/AuthForm";
import { getSessionUser } from "../server/session";

export const Route = createFileRoute("/login")({
  validateSearch: (search: Record<string, unknown>): { redirect?: string } => {
    const to = safeRedirect(search.redirect);
    return to ? { redirect: to } : {};
  },
  beforeLoad: async ({ search }) => {
    if (await getSessionUser()) throw redirect({ href: search.redirect ?? "/" });
  },
  head: () => ({ meta: [{ title: "Sign in — Undrly" }] }),
  component: Page,
});

function Page() {
  const { redirect: to } = Route.useSearch();
  return <AuthForm mode="login" redirectTo={to} />;
}

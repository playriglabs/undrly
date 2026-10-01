import { createMiddleware, createServerFn } from "@tanstack/react-start";
import { getRequest } from "@tanstack/react-start/server";
import { getAuth } from "./auth";

export type SessionUser = { id: string; name: string; email: string };

async function currentUser(): Promise<SessionUser | null> {
  const session = await getAuth().api.getSession({ headers: getRequest().headers });
  if (!session) return null;
  const { id, name, email } = session.user;
  return { id, name, email };
}

/** The signed-in user from the session cookie, or null. Used by route guards for UX. */
export const getSessionUser = createServerFn({ method: "GET" }).handler(() => currentUser());

/**
 * The data boundary: every server function reading private data uses this.
 * Route guards only redirect; this rejects direct RPC calls without a session.
 */
export const authMiddleware = createMiddleware({ type: "function" }).server(async ({ next }) => {
  const user = await currentUser();
  if (!user) throw new Error("Unauthorized");
  return next({ context: { user } });
});

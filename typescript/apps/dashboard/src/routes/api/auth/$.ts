import { createFileRoute } from "@tanstack/react-router";
import { getAuth } from "../../../server/auth";

// Better Auth's endpoints: /api/auth/sign-in/email, /sign-up/email, /sign-out, /get-session, ...
export const Route = createFileRoute("/api/auth/$")({
  server: {
    handlers: {
      GET: ({ request }) => getAuth().handler(request),
      POST: ({ request }) => getAuth().handler(request),
    },
  },
});

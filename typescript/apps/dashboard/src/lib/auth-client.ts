import { polarClient } from "@polar-sh/better-auth/client";
import { createAuthClient } from "better-auth/react";

// Same origin as the dashboard: talks to /api/auth/*, the session lives in an httpOnly cookie.
// polarClient adds `checkout` and `customer.portal` (card billing, src/server/polar.ts).
export const authClient = createAuthClient({ plugins: [polarClient()] });

import { createAuthClient } from "better-auth/react";

// Same origin as the dashboard: talks to /api/auth/*, the session lives in an httpOnly cookie.
export const authClient = createAuthClient();

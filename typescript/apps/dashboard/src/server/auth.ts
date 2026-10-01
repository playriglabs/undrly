/**
 * Better Auth: email/password accounts with an httpOnly session cookie.
 * Server-only. Tables live in the `dashboard` schema
 * (database/migrations/0023_dashboard_auth.sql); models and fields are mapped
 * to those snake_case names here.
 */
import { betterAuth } from "better-auth";
import { tanstackStartCookies } from "better-auth/tanstack-start";
import { Pool } from "pg";

const timestamps = { createdAt: "created_at", updatedAt: "updated_at" };

function createAuth() {
  // Created on first use, so env is read at runtime on the server, never at module scope.
  const env = (name: string) => {
    const value = process.env[name];
    if (!value) throw new Error(`${name} is not set`);
    return value;
  };
  return betterAuth({
    secret: env("BETTER_AUTH_SECRET"),
    baseURL: env("BETTER_AUTH_URL"),
    trustedOrigins: trustedOrigins(env("BETTER_AUTH_URL")),
    database: new Pool({
      connectionString: env("DATABASE_URL"),
      options: "-c search_path=dashboard",
      max: 5,
    }),
    emailAndPassword: { enabled: true, minPasswordLength: 10, autoSignIn: true },
    session: {
      expiresIn: 60 * 60 * 24 * 7,
      updateAge: 60 * 60 * 24,
      modelName: "sessions",
      fields: {
        ...timestamps,
        userId: "user_id",
        expiresAt: "expires_at",
        ipAddress: "ip_address",
        userAgent: "user_agent",
      },
    },
    user: {
      modelName: "users",
      fields: { ...timestamps, emailVerified: "email_verified" },
    },
    account: {
      modelName: "accounts",
      fields: {
        ...timestamps,
        userId: "user_id",
        accountId: "account_id",
        providerId: "provider_id",
        accessToken: "access_token",
        refreshToken: "refresh_token",
        idToken: "id_token",
        accessTokenExpiresAt: "access_token_expires_at",
        refreshTokenExpiresAt: "refresh_token_expires_at",
      },
    },
    verification: {
      modelName: "verifications",
      fields: { ...timestamps, expiresAt: "expires_at" },
    },
    // Credential stuffing guard on sign-in and sign-up (in-memory, per process).
    // Production only, like Better Auth's default: local testing hits it fast.
    rateLimit: {
      enabled: process.env.NODE_ENV === "production",
      window: 60,
      max: 100,
      customRules: {
        "/sign-in/email": { window: 60, max: 5 },
        "/sign-up/email": { window: 60, max: 3 },
      },
    },
    advanced: { useSecureCookies: env("BETTER_AUTH_URL").startsWith("https://") },
    // Must be last: sets cookies through TanStack Start's response.
    plugins: [tanstackStartCookies()],
  });
}

/**
 * Origins allowed to call /api/auth/*: the dashboard's own, its loopback twin
 * in local development (localhost and 127.0.0.1 are different origins to a
 * browser), and any listed in BETTER_AUTH_TRUSTED_ORIGINS (comma-separated).
 */
function trustedOrigins(base: string): string[] {
  const url = new URL(base);
  const origins = [url.origin];
  const twin = { localhost: "127.0.0.1", "127.0.0.1": "localhost" }[url.hostname];
  if (twin) origins.push(`${url.protocol}//${twin}${url.port ? `:${url.port}` : ""}`);
  const extra = process.env.BETTER_AUTH_TRUSTED_ORIGINS ?? "";
  for (const o of extra.split(",").map((x) => x.trim())) if (o) origins.push(new URL(o).origin);
  return origins;
}

let instance: ReturnType<typeof createAuth> | undefined;
export function getAuth() {
  instance ??= createAuth();
  return instance;
}

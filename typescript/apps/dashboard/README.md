# Undrly dashboard

Market explorer over the Undrly API: TanStack Start (React, SSR), TanStack
Query, Tailwind with the landing page's brand tokens, Better Auth accounts.

```sh
cp .env.example .env     # API URL, DATABASE_URL, BETTER_AUTH_SECRET, BETTER_AUTH_URL
bun run dev              # http://127.0.0.1:3000
bun run build && bun run start
```

- **Data:** server functions (`src/lib/api.ts`) call the API from the server, so
  `UNDRLY_API_KEY` never reaches the browser. Pages read them through TanStack
  Query (`src/lib/queries.ts`); loaders prefetch during SSR and the client
  refreshes prices every 15 s. The table uses `GET /v1/markets`.
- **Auth:** email and password via Better Auth (`/api/auth/*`), session in an
  httpOnly cookie. Accounts live in the `dashboard` schema
  (`database/migrations/0023_dashboard_auth.sql`). Every page except
  `/login` and `/register` needs a session: the `_app` layout redirects, and
  `authMiddleware` rejects data server functions called without one.
- **API keys:** the page is UI only; the API does not issue or check keys yet.

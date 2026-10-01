-- Dashboard accounts (typescript/apps/dashboard, Better Auth).
--
-- Kept in their own schema: market data lives in `public` and never refers
-- to these tables. The dashboard connects with search_path=dashboard and
-- maps Better Auth's models and fields onto these names
-- (apps/dashboard/src/server/auth.ts). Ids are Better Auth's own text ids.
CREATE SCHEMA dashboard;

CREATE TABLE dashboard.users (
  id             text PRIMARY KEY,
  name           text NOT NULL,
  email          text NOT NULL UNIQUE,
  email_verified boolean NOT NULL,
  image          text,
  created_at     timestamptz NOT NULL,
  updated_at     timestamptz NOT NULL
);

CREATE TABLE dashboard.sessions (
  id         text PRIMARY KEY,
  user_id    text NOT NULL REFERENCES dashboard.users (id) ON DELETE CASCADE,
  -- The cookie's session token; looked up on every authenticated request.
  token      text NOT NULL UNIQUE,
  expires_at timestamptz NOT NULL,
  ip_address text,
  user_agent text,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL
);
CREATE INDEX sessions_user_id ON dashboard.sessions (user_id);

-- One row per sign-in method; email/password keeps the password hash here.
CREATE TABLE dashboard.accounts (
  id                       text PRIMARY KEY,
  user_id                  text NOT NULL REFERENCES dashboard.users (id) ON DELETE CASCADE,
  account_id               text NOT NULL,
  provider_id              text NOT NULL,
  access_token             text,
  refresh_token            text,
  id_token                 text,
  access_token_expires_at  timestamptz,
  refresh_token_expires_at timestamptz,
  scope                    text,
  password                 text,
  created_at               timestamptz NOT NULL,
  updated_at               timestamptz NOT NULL
);
CREATE INDEX accounts_user_id ON dashboard.accounts (user_id);

CREATE TABLE dashboard.verifications (
  id         text PRIMARY KEY,
  identifier text NOT NULL,
  value      text NOT NULL,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL,
  updated_at timestamptz NOT NULL
);
CREATE INDEX verifications_identifier ON dashboard.verifications (identifier);

/**
 * The dashboard's Postgres pool, shared by Better Auth and billing. Server-only;
 * connects with search_path=dashboard (database/migrations/0023, 0029).
 */
import { Pool } from "pg";

let pool: Pool | undefined;
export function getPool(): Pool {
  if (!pool) {
    const connectionString = process.env.DATABASE_URL;
    if (!connectionString) throw new Error("DATABASE_URL is not set");
    pool = new Pool({ connectionString, options: "-c search_path=dashboard", max: 5 });
  }
  return pool;
}

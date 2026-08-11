#!/bin/sh
# Applies the REAL migration chains from both repos, in order, plus the
# PostgREST-compatible stub — no hand-recreated tables. Run inside the
# `migrate` compose service, which mounts valori-ui's actual migration
# directories read-only (see docker-compose.yml).
set -eu

PGHOST="${PGHOST:-postgres}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"
PGDATABASE="${PGDATABASE:-postgres}"
export PGPASSWORD="${PGPASSWORD:-postgres}"

psql_exec() {
  psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$PGDATABASE" -v ON_ERROR_STOP=1 "$@"
}

echo "==> Waiting for Postgres..."
until psql_exec -c 'select 1' >/dev/null 2>&1; do sleep 1; done

# Idempotency guard: `docker compose run`/`up` on an already-migrated
# volume (pg-data persists across container restarts) re-triggers this
# service's dependency check every invocation, but the migration files
# themselves are raw one-shot DDL (CREATE TABLE, not CREATE TABLE IF NOT
# EXISTS) — reapplying them errors. A marker table is the same pattern
# any real migration runner (sqlx, flyway) uses to track "already
# applied"; it is not a stand-in for the migrations themselves, which
# still run for real, exactly once, against a fresh volume.
ALREADY_APPLIED=$(psql_exec -tA -c "select to_regclass('public._e2e_migrations_applied') is not null" 2>/dev/null || echo f)
if [ "$ALREADY_APPLIED" = "t" ]; then
  echo "==> Already applied on this volume (public._e2e_migrations_applied exists) — skipping."
  exit 0
fi

echo "==> Supabase-compatible stub (auth schema, roles, extensions)"
psql_exec -f /migrations/00_supabase_stub.sql

echo "==> valori-ui/backend migrations (infra schema)"
for f in /backend-migrations/*.sql; do
  base=$(basename "$f")
  # 0014 locks down sqlx's OWN migration-tracking table
  # (public._sqlx_migrations), which only exists when migrations are
  # applied via sqlx's runner, not raw psql — irrelevant to this
  # environment's schema and would fail with "relation does not exist".
  if [ "$base" = "0014_lock_down_sqlx_migrations.sql" ]; then
    echo "    skip $base (sqlx-runner-only, not applicable to psql apply)"
    continue
  fi
  echo "    $base"
  psql_exec -f "$f"
done

echo "==> valori-ui/supabase migrations (public schema, RPCs, RLS)"
for f in /supabase-migrations/*.sql; do
  base=$(basename "$f")
  echo "    $base"
  psql_exec -f "$f"
done

echo "==> Grant service_role full access to public (BYPASSRLS already set; this covers any lingering column-grant narrowing)"
psql_exec -c "grant all privileges on all tables in schema public to service_role;"

echo "==> E2E fixtures (seed user/org, low rate-limit tier for testing)"
psql_exec -f /migrations/01_seed_e2e.sql

psql_exec -c "create table public._e2e_migrations_applied (applied_at timestamptz not null default now());" >/dev/null
psql_exec -c "insert into public._e2e_migrations_applied default values;" >/dev/null

echo "==> Migrations applied."

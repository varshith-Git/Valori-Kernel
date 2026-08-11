-- PostgREST-compatible Supabase stub for the local Cloud E2E environment.
--
-- This is NOT a local Supabase (no GoTrue/Storage/Realtime/Edge Functions)
-- — see docs/reviews/local-cloud-e2e-audit.md §9 for exactly what is and
-- isn't reproduced, and why that's sufficient for what this environment
-- needs to prove. What's here is the minimum real Postgres schema/roles
-- Supabase provides at the platform level that this repo's own migrations
-- assume already exist: the `anon`/`authenticated`/`service_role` roles,
-- an `auth.users` table + `auth.uid()`, and pgcrypto/uuid-ossp.

create schema if not exists auth;
create schema if not exists extensions;
create schema if not exists infra;

do $$
begin
  if not exists (select 1 from pg_roles where rolname = 'anon') then
    create role anon nologin;
  end if;
  if not exists (select 1 from pg_roles where rolname = 'authenticated') then
    create role authenticated nologin;
  end if;
  if not exists (select 1 from pg_roles where rolname = 'service_role') then
    create role service_role nologin;
  end if;
end $$;

-- Real Supabase grants service_role BYPASSRLS as a platform default —
-- outside anything this repo's own migrations manage. Found the hard way
-- in Valori-Kernel's project-api-key-P2.3 phase: without this,
-- getWorkerAuthToken()'s service-role query hits the exact same
-- column-privilege wall a regular `authenticated` caller would.
alter role service_role bypassrls;

grant usage on schema public to anon, authenticated, service_role;
grant usage on schema extensions to anon, authenticated, service_role;

create extension if not exists pgcrypto with schema extensions;
create extension if not exists "uuid-ossp" with schema extensions;

create table if not exists auth.users (
  id uuid primary key default gen_random_uuid(),
  email text unique,
  raw_user_meta_data jsonb default '{}'::jsonb
);

-- Matches real Supabase's auth.uid(): PostgREST 11+ sets the whole JWT
-- claims object as one JSON GUC (`request.jwt.claims`), not the older
-- per-claim GUCs (`request.jwt.claim.sub`) some docs still describe —
-- both are read here, JSON form preferred, so this works regardless of
-- which PostgREST version signs the request.
create or replace function auth.uid() returns uuid
language sql stable
as $$
  select nullif(
    coalesce(
      current_setting('request.jwt.claims', true)::jsonb ->> 'sub',
      current_setting('request.jwt.claim.sub', true)
    ),
    ''
  )::uuid
$$;

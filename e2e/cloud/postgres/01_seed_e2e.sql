-- Fixture data for the local Cloud E2E environment — a signed-up user +
-- their org, in exactly the shape a real signup would produce. This
-- exists because GoTrue (real Supabase Auth) isn't running here (see
-- docs/reviews/local-cloud-e2e-audit.md §9); it is NOT a substitute for
-- or approximation of anything the E2E tests actually verify —
-- authentication of the resulting API keys, project resolution, and
-- worker routing all go through the real, unmodified functions.
--
-- Also seeds a `plans`/`subscriptions` row on the real, unmodified
-- 'free' tier (60/min — its real production default; see
-- supabase/migrations/20260723040000_api_usage_and_rate_limits.sql).
-- An earlier draft of this file lowered 'free' globally to 5/min "for
-- fast testing" — that broke every OTHER test sharing this seeded org's
-- key, since the real limiter counts every vlk_-authenticated call
-- (create/search/insert/delete/namespaces all resolve through the same
-- verify_api_key() check point), not just the dedicated rate-limit test.
-- test_limits.py instead preconditions its OWN dedicated key's
-- `api_keys.rate_limit_window_count` directly (via service_role, the
-- same substitution this file's docstring already documents elsewhere)
-- to sit one request below the REAL 60/min ceiling, so it can trip the
-- real limiter in 2 requests without touching global plan config other
-- tests depend on.

insert into auth.users (id, email)
values ('00000000-0000-0000-0000-0000000e2e01', 'e2e@local.test')
on conflict (id) do nothing;

insert into public.organizations (id, name, slug)
values ('00000000-0000-0000-0000-0000000e2e0a', 'E2E Org', 'e2e-org')
on conflict (id) do nothing;

insert into public.org_members (org_id, user_id, role)
values ('00000000-0000-0000-0000-0000000e2e0a', '00000000-0000-0000-0000-0000000e2e01', 'owner')
on conflict do nothing;

insert into public.subscriptions (org_id, plan)
values ('00000000-0000-0000-0000-0000000e2e0a', 'free')
on conflict (org_id) do nothing;

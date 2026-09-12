-- SH2 real-E2E fixtures: two Free orgs + one Pro org, same shape as
-- 01_seed_e2e.sql. Used only by this manual verification pass, not part
-- of the automated pytest suite.

insert into auth.users (id, email) values
  ('f2e00001-0000-0000-0000-0000000f2e01', 'free-a@local.test'),
  ('f2e00002-0000-0000-0000-0000000f2e02', 'free-b@local.test'),
  ('f2e00003-0000-0000-0000-0000000f2e03', 'pro-c@local.test')
on conflict (id) do nothing;

insert into public.organizations (id, name, slug) values
  ('00000000-0000-0000-0000-0000000f2e0a', 'SH2 Free Org A', 'sh2-free-org-a'),
  ('00000000-0000-0000-0000-0000000f2e0b', 'SH2 Free Org B', 'sh2-free-org-b'),
  ('00000000-0000-0000-0000-0000000f2e0c', 'SH2 Pro Org C', 'sh2-pro-org-c')
on conflict (id) do nothing;

insert into public.org_members (org_id, user_id, role) values
  ('00000000-0000-0000-0000-0000000f2e0a', 'f2e00001-0000-0000-0000-0000000f2e01', 'owner'),
  ('00000000-0000-0000-0000-0000000f2e0b', 'f2e00002-0000-0000-0000-0000000f2e02', 'owner'),
  ('00000000-0000-0000-0000-0000000f2e0c', 'f2e00003-0000-0000-0000-0000000f2e03', 'owner')
on conflict do nothing;

insert into public.subscriptions (org_id, plan) values
  ('00000000-0000-0000-0000-0000000f2e0a', 'free'),
  ('00000000-0000-0000-0000-0000000f2e0b', 'free'),
  ('00000000-0000-0000-0000-0000000f2e0c', 'pro')
on conflict (org_id) do nothing;

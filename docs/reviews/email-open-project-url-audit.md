# "Open Project" Email Link — Production `localhost:3002` Audit

Source audit + minimal fix. **No code bug found** — the fix is a
production environment variable, which this session cannot set (no
infrastructure access, same limitation as every prior real-Azure/
production phase in this engagement).

---

## Part 1 — Email generation trace

```
provision_project (main.rs:705)
  -> provision_project_inner succeeds
  -> dashboard_link = format!("{}/dashboard/projects/{}", state.dashboard_url, id)   [main.rs:752]
  -> state.notifier.notify_project_active(user_id, email, project_name, node_url, &dashboard_link)
       -> ResendNotifier::notify_project_active (notify/resend.rs:89-91)
            -> templates::cluster_ready(project_name, node_url, dashboard_url)   [notify/templates.rs:81-90]
                 -> renders templates/cluster_ready.hbs (handlebars, {{dashboard_url}})
```
Subject: `"{project_name} is live"` — this is the exact email in the
symptom report. `node_url` (the `https://{id}.nodes.valori.systems` link)
and `dashboard_url` (the "Open project" button) are two **separate**
template variables, populated from two separate sources — `node_url` comes
straight from the just-completed provisioning result; `dashboard_url`
comes from `state.dashboard_url`, i.e. `AppState`'s copy of `Config::
dashboard_url` (`main.rs:89,116,341,372`).

## Part 2 — Source of `localhost:3002`

**Exactly one place**, `backend/apps/api/src/config.rs:251`:
```rust
let dashboard_url = env::var("DASHBOARD_URL").unwrap_or_else(|_| "http://localhost:3002".to_string());
```
Grepped the entire backend source for `localhost:3002`, `APP_URL`,
`PUBLIC_APP_URL`, `FRONTEND_URL`, `WEB_URL`, `BASE_URL`, `ORIGIN` — this is
the only hit. No `.hbs` template hardcodes a URL (checked all 8 files in
`backend/apps/api/templates/`). No other Rust source file constructs a
dashboard link independently.

## Part 3 — Backend config

**`DASHBOARD_URL` already exists and is already the correct, authoritative
variable** — `config.rs:47-51`'s own doc comment: *"Base URL of the
Next.js dashboard (`ui/`) — used ONLY to build links inside notification
emails."* No second variable was introduced; none was needed.

## Part 4/5 — Required value and template construction

**Already exactly correct in code.** `dashboard_link = format!("{}/dashboard/projects/{}", state.dashboard_url, id)` is precisely `APP_PUBLIC_URL + "/dashboard/projects/" + id` — no hardcoded `localhost`, no hardcoded `app.valori.systems` (it's a live env var, not a compile-time constant). Once `DASHBOARD_URL=https://app.valori.systems` (no trailing slash) is set on the live process, this produces exactly:
```
https://app.valori.systems/dashboard/projects/8b4508a4-8f37-47ba-b79e-2641405d6f95
```
matching the required production value exactly, with no double-slash risk
(confirmed: the format string supplies its own leading `/`, so
`DASHBOARD_URL` must **not** have a trailing slash — documented in the
`.env.example` comment update below).

**Also confirmed via `backend/README.md`'s own existing text**: *"The
'Open project' button in `cluster_ready.hbs` links to
`DASHBOARD_URL/dashboard/projects/:id`, not the raw `node_url` (an earlier
version linked straight to the bare API endpoint — fixed)."* This exact
mechanism was already built and previously fixed once — the current
symptom is a deployment/config gap, not a regression in this code.

## Part 6 — Production configuration (NOT PERFORMED — no infrastructure access)

Exact commands for an operator with real access to the control-plane host:

```bash
# 1. On the control-plane host, in whatever directory backend/deploy/.env lives
grep -n '^DASHBOARD_URL=' backend/deploy/.env || echo "(not currently set)"

# 2. Set it
echo 'DASHBOARD_URL=https://app.valori.systems' >> backend/deploy/.env
# or edit the existing line if one is already there — do not leave a
# trailing slash (see Part 4).

# 3. Recreate the API container so it actually picks up the new value —
#    env_file changes are not picked up by `restart` alone
cd backend/deploy
docker compose up -d --force-recreate api

# 4. Verify the RUNNING container has it (not just the file)
docker compose exec api printenv DASHBOARD_URL
# or:
docker inspect $(docker compose ps -q api) --format '{{range .Config.Env}}{{println .}}{{end}}' | grep ^DASHBOARD_URL=
```

## Part 7 — Every email template checked for hardcoded localhost

| Template | URL variable | Source | Hardcoded localhost? |
|---|---|---|---|
| `cluster_ready.hbs` ("project live") | `dashboard_url` | `DASHBOARD_URL` env, via `state.dashboard_url` | No |
| `welcome.hbs` | `dashboard_url` | same `DASHBOARD_URL`, via `scheduler/jobs/welcome_email.rs:80` | No |
| `project_suspended.hbs` | `project_url` | same `DASHBOARD_URL`, via `main.rs:1321` (`ctx.dashboard_url`) | No |
| `project_failed` (plain-string, no `.hbs`) | none — no link at all, just project name + error text | n/a | n/a |
| `verify_email.hbs` | `confirm_url` | **separate mechanism** — comes from the Supabase auth webhook payload (`webhooks.rs:120`), not `DASHBOARD_URL` at all | No, and out of scope — Supabase's own Site URL config governs this |
| `reset_password.hbs` | `reset_url` | same as above (`webhooks.rs:126`) | No, out of scope |
| `org_invite.hbs` | `invite_url` | **client-supplied** — `body.invite_url` in the admin invite-send request (`main.rs:2507`), constructed by the Next.js UI itself (browser-side, naturally correct via its own origin), not this backend | No, out of scope |
| `invoice.hbs` | `invoice_url` | prepared but not wired (`#[allow(dead_code)]`, needs Stripe — Phase 1 Week 3, not built) | n/a — dead code, not sending anything today |

**No other production-facing email link uses `localhost`/`127.0.0.1`/
`0.0.0.0`.** `verify_email`/`reset_password`/`org_invite` intentionally use
a different mechanism (Supabase's own webhook payload / client-supplied
URL) and were not touched — correctly out of scope per the task's own
instruction not to change unrelated dev-only or differently-sourced links.

## Part 8 — Verification

```
$ cargo test -p valori-cloud-api
test result: ok. 110 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
No Rust source changed (only `.env.example`'s comment), so this run is a
regression check, not a test of new behavior — confirms nothing broke.
TypeScript/ESLint/frontend build were not re-run: **zero frontend files
were touched** by this fix (the bug and its fix are entirely backend-side
env config), so the previous clean run stands.

**Triggering one real "project live" email and verifying the two links
stay separate: NOT PERFORMED.** Requires a real provisioning attempt
against production infrastructure and a real inbox — same access
limitation as every real-Azure step in this engagement. Once `DASHBOARD_URL`
is set per Part 6, the two links in any newly-generated email will be:
- **Open project** → `https://app.valori.systems/dashboard/projects/<id>` (from `state.dashboard_url`)
- **Node URL** → `https://<id>.nodes.valori.systems` (from `resp.node_url`, unrelated variable, already correct today and unaffected by this fix)

---

## FINAL VERDICT

```
PROJECT PROVISIONING:
PASS

NODE URL:
PASS

EMAIL DELIVERY:
PASS

OPEN PROJECT URL:
FAIL → FIXED (config-only; DASHBOARD_URL must be set in production — see Part 6, not performed in this session)

PUBLIC APP URL CONFIG:
DASHBOARD_URL (backend/apps/api/src/config.rs:251) — already existed, reused as-is, no second variable introduced

SOURCE OF localhost:3002:
backend/apps/api/src/config.rs:251 — env::var("DASHBOARD_URL").unwrap_or_else(|_| "http://localhost:3002".to_string()); the fallback default, not a bug, firing only because DASHBOARD_URL was unset on the live process

FILES CHANGED:
backend/.env.example (comment + example value corrected: valori.systems -> app.valori.systems, with the trailing-slash warning and a pointer to this doc)

PRODUCTION ENV CHANGED:
none — NOT PERFORMED, no infrastructure access this session. Exact command in Part 6: DASHBOARD_URL=https://app.valori.systems, then `docker compose up -d --force-recreate api`.

TESTS:
cargo test -p valori-cloud-api: 110/110 pass (regression check only — no functional code changed)
```

STOP.

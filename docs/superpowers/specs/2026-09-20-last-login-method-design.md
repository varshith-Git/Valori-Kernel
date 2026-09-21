# Last Used Login Method — Design

**Repo:** `valori-ui` (this design doc lives in `Valori-Kernel` per this project's existing convention of centralizing specs here — see `docs/superpowers/specs/2026-09-12-shared-hosting-hardening-design.md` for precedent).

## Goal

When a signed-out visitor returns to the Valori Cloud login page, they should
immediately see which of the three auth methods (Google, GitHub, email/password)
they most recently used to successfully sign in, so they don't have to guess or
try each one. Server-rendered, no flicker, no new auth semantics.

## Audit findings (confirmed against real code, not assumed)

| Question | Answer |
|---|---|
| Where Google OAuth starts | `login/page.tsx`'s `signInWithOAuth('google', next, desktop)` → `supabase.auth.signInWithOAuth({provider: 'google', ...})` |
| Where GitHub OAuth starts | Same function, `signInWithOAuth('github', ...)` |
| Where OAuth becomes definitively successful | `auth/callback/route.ts`, immediately after `supabase.auth.exchangeCodeForSession(code)` returns with no error |
| Where email/password becomes definitively successful | `login/actions.ts`'s `login()`, immediately after `supabase.auth.signInWithPassword(...)` returns with no error |
| Cookie config for `valori.systems`/`app.valori.systems` | `utils/supabase/cookieOptions.ts`'s `supabaseCookieOptions(hostname)` — returns `{domain: '.valori.systems'}` only when `hostname` ends with `valori.systems`, else `undefined` (so localhost/Vercel previews never get an invalid Domain attribute). Single source of truth already used by all 3 Supabase client constructors (browser/server/middleware). |
| Existing preferences/profile table | None. `admin_users` and `personal_access_tokens` are the closest per-user tables but are domain-specific; a new `public.user_preferences` table is needed. |
| Reliable OAuth provider detection (no query param) | **Existing precedent found**: `Header.tsx:124` and `AppSidebar.tsx:160` both already do `user?.app_metadata?.provider \|\| user?.identities?.[0]?.provider \|\| null` to display the signed-in provider. Reusing this exact pattern in the callback route, read off `data.session.user` right after `exchangeCodeForSession()` succeeds. |
| Test framework | `vitest` is configured; colocated `*.test.ts` files exist (`proxy.test.ts`, `hosts.test.ts`, `lib/server/demo-rag/*.test.ts`). New tests follow this same colocation convention. |
| Existing Badge component | `components/ui/badge.tsx` (cva variants: default/secondary/destructive/outline/ghost/link). Use `variant="secondary"` — muted, no custom colors. |
| Logout | Implemented via `supabase.auth.signOut()`, called from `Header.tsx`/`AppSidebar.tsx` client-side — does not touch cookies directly at all (Supabase's own client clears its own session cookies; nothing in this app's logout path does a blanket cookie wipe). The new preference cookie is untouched by this path since nothing there references it. |

## Data model

`LoginMethod = 'google' | 'github' | 'email'` — one shared type, one shared
cookie name, one parser. Defined in a new module:
`ui/src/lib/server/loginMethod.ts` (matches the existing `lib/server/{admin,mfa,app-url,project}.ts`
naming convention — server-only, since it needs `next/headers`).

```ts
export type LoginMethod = 'google' | 'github' | 'email'
export const LAST_LOGIN_METHOD_COOKIE = 'valori_last_login_method'

export function parseLoginMethod(value: string | undefined | null): LoginMethod | null
export async function getLastLoginMethod(): Promise<LoginMethod | null>   // reads the cookie, server-side
export async function recordSuccessfulLogin(
  supabase: SupabaseClient, userId: string, method: LoginMethod
): Promise<void>   // sets the cookie + best-effort upserts user_preferences
```

`recordSuccessfulLogin` is the ONE place that writes the cookie and the DB row —
`auth/callback/route.ts` and `login/actions.ts` both call it; neither
independently implements cookie-writing logic (per the no-duplication
requirement).

Cookie options reuse `supabaseCookieOptions(hostname)` for the `domain` field
(same hostname read via `headers()` that `utils/supabase/server.ts` already
does) — no separate domain-detection logic. `secure` mirrors the same
`hostname?.endsWith('valori.systems')` check, so it's never set on localhost.
`httpOnly: false` (this is a non-sensitive UI hint, not a session token).
`maxAge`: 1 year. `sameSite: 'lax'`. `path: '/'`.

The cookie value is ONLY ever one of the three literal strings — never an
email, user id, provider id, or any other identifying data.

## Provider determination (OAuth)

`auth/callback/route.ts`, right after `exchangeCodeForSession()` succeeds:

```ts
const rawProvider = data.session?.user?.app_metadata?.provider
  ?? data.session?.user?.identities?.[0]?.provider
  ?? null
const method = parseLoginMethod(rawProvider)   // null if not 'google'/'github'/'email'
if (method) await recordSuccessfulLogin(supabase, data.session.user.id, method)
```

Never trusts a raw `?provider=` query param — this value comes from Supabase's
own verified user object, the same field already trusted elsewhere in this
codebase for provider display.

## Email/password

`login/actions.ts`'s `login()`, right after the existing
`supabase.auth.signInWithPassword(...)` error check (no new error path — if it
errors, the function already redirects to `/error` and returns before reaching
this point):

```ts
const { data, error } = await supabase.auth.signInWithPassword({...})
if (error) { redirect(...) }   // existing behavior, unchanged
if (data.user) await recordSuccessfulLogin(supabase, data.user.id, 'email')
// existing redirect(...) unchanged
```

## Server-rendered, no flicker

`login/page.tsx` currently has `'use client'` as its first line, making the
whole file (including the default-exported `LoginPage`) a client component.
Split:

- **New** `login/LoginForm.tsx` — the existing `LoginForm` client component
  (needs `useSearchParams`, unchanged reason), moved as-is, plus a new
  `lastUsedMethod: LoginMethod | null` prop it uses to render the badge.
- **Rewritten** `login/page.tsx` — becomes a plain Server Component (no
  `'use client'`), calls `getLastLoginMethod()` (a `next/headers` cookie read,
  server-side, before first paint), and renders
  `<Suspense><LoginForm lastUsedMethod={...} /></Suspense>`.

This is the minimum split that satisfies "don't convert the whole page to
client merely to read this" — only a thin wrapper is added; the existing
client boundary and its existing reason (`useSearchParams`) are untouched.

## UI placement

- Google/GitHub: badge appended inline inside the existing single-line button
  content (`<GoogleIcon/> Continue with Google <Badge>Last used</Badge>`),
  right-aligned via `justify-between` — no new layout row, no reordering.
- Email: a small `Email <Badge>Last used</Badge>` line is added directly above
  the "Email address" field, but ONLY rendered when `lastUsedMethod === 'email'`
  — when it's not, the email section renders exactly as it does today (no new
  persistent "Email" heading for the non-last-used case, keeping the diff
  visually minimal for the common case).
- Exactly one of the three renders the badge at a time (mutually exclusive by
  construction — `lastUsedMethod` is a single value).
- `Badge` uses `variant="secondary"` — muted, existing token colors, no custom
  hex, no animation.

## Database

New migration `supabase/migrations/20260920000000_user_login_preferences.sql`,
following the exact convention of `20260723050000_personal_access_tokens.sql`:

```sql
create table public.user_preferences (
  user_id           uuid primary key references auth.users(id) on delete cascade,
  last_login_method text check (last_login_method is null or last_login_method in ('google', 'github', 'email')),
  updated_at        timestamptz not null default now()
);

alter table public.user_preferences enable row level security;

create policy user_preferences_select on public.user_preferences
  for select to authenticated using (user_id = auth.uid());

create policy user_preferences_insert on public.user_preferences
  for insert to authenticated with check (user_id = auth.uid());

create policy user_preferences_update on public.user_preferences
  for update to authenticated using (user_id = auth.uid()) with check (user_id = auth.uid());

revoke all on public.user_preferences from public, anon, authenticated;
grant select, insert, update (last_login_method, updated_at) on public.user_preferences to authenticated;
```

The upsert in `recordSuccessfulLogin` runs through the caller's own
authenticated Supabase client (the same one that just completed
`exchangeCodeForSession`/`signInWithPassword`) — never a service-role client,
matching this app's existing pattern of doing user-owned writes through RLS
rather than bypassing it. No service-role credential is introduced.

## Failure isolation

`recordSuccessfulLogin`'s DB upsert is wrapped in `try/catch`; a failure is
`console.error`-logged (matching this codebase's existing convention of
non-blocking side effects, e.g. `notify_project_active`) and never thrown —
authentication and the existing redirect always proceed regardless of whether
the preference write succeeds. The cookie write itself (via `next/headers`
`cookies().set()`) is synchronous and not expected to fail in a way that needs
separate handling.

## Invalid/stale cookie handling

`parseLoginMethod()` is the only place that reads the raw cookie string; it
returns `LoginMethod | null`, never casts (`as LoginMethod`). Any value outside
the three literals (missing cookie, `"facebook"`, empty string, garbage)
resolves to `null`, and the login page renders with no badge, same as a
first-time visitor — never throws, never crashes the page.

## Out of scope (explicitly, per your spec)

- No email-first login flow, no lookup-by-email endpoint (would enable account
  enumeration).
- No cross-device read path today — `user_preferences` is written on every
  login but nothing reads it yet; that's future work if Valori ever moves to
  an identity-first login flow.
- No change to session lifetime, JWT expiry, refresh tokens, PKCE, middleware
  auth checks, or any existing redirect logic.

## Testing plan

Colocated `*.test.ts`, matching existing convention:

- `loginMethod.test.ts`: `parseLoginMethod()` — google/github/email pass
  through, `"facebook"`/`"abc"`/empty/undefined all → `null`.
- `login/actions.test.ts` (or extend existing coverage if a login actions test
  already exists — confirm during implementation): successful email login →
  cookie set to `'email'`, preference upsert attempted; failed login → neither
  changes.
- `auth/callback` behavior: successful Google/GitHub callback → cookie set to
  the correct provider; failed `exchangeCodeForSession` → cookie untouched.
- Preference upsert failure (mocked) → login still succeeds, redirect still
  happens.
- UI: given each of `'google' | 'github' | 'email' | null | 'invalid'` as the
  `lastUsedMethod` prop, exactly the right element shows the badge (or none
  do), and an invalid value never crashes rendering.
- Logout: confirm it doesn't clear `valori_last_login_method` (by inspection —
  logout goes through `supabase.auth.signOut()` only, which doesn't reference
  this cookie name at all; call this out explicitly in the test comment rather
  than skipping it just because no exploit path currently exists).

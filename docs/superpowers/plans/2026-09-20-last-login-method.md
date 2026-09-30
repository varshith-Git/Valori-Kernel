# Last Used Login Method Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a subtle "Last used" badge next to whichever of Google/GitHub/email the visitor most recently used to successfully sign in, server-rendered with no flicker, without changing any existing auth semantics.

**Architecture:** One shared server-only module (`lib/server/loginMethod.ts`) owns the cookie name, the `LoginMethod` type, a strict parser, a cookie reader, and a "record success" writer (cookie + best-effort DB upsert). The two real success points (`login/actions.ts`'s `login()` for email, `auth/callback/route.ts`'s `GET()` for OAuth) each call the one writer function — no duplicated cookie logic. The login page is split into a thin Server Component (`page.tsx`, reads the cookie before first paint) and the existing client form (`LoginForm.tsx`, now accepting the resolved method as a prop).

**Tech Stack:** Next.js 16 (App Router, Server Components + Server Actions), `@supabase/ssr`, `next/headers`, Vitest (Node environment, no DOM/RTL — see Task 8 for why UI logic is tested without a rendering library), Supabase Postgres migrations.

## Global Constraints

- `LoginMethod = 'google' | 'github' | 'email'` — never store or accept any other string.
- Only a *successful* auth event may change the cookie or DB row — failed password, invalid email, cancelled/errored OAuth, expired state, or a failed code exchange must never touch it.
- Cookie name: `valori_last_login_method`. `Path=/`, `SameSite=Lax`, `Max-Age` ≈ 1 year, `HttpOnly=false`. `Domain=.valori.systems` and `Secure=true` ONLY when the request host actually ends with `valori.systems` (reuse `supabaseCookieOptions()`'s exact hostname check — do not reimplement it).
- The cookie must never contain an email, user id, provider id, access/refresh token, session id, org id, or project id — only one of the three literal strings.
- Never trust a raw, unvalidated `?provider=` query param for OAuth provider detection. Read `user.app_metadata.provider ?? user.identities?.[0]?.provider`, matching the existing precedent in `Header.tsx`/`AppSidebar.tsx`.
- `parseLoginMethod()` never does `value as LoginMethod` — it validates against the literal set and returns `null` for anything else, including empty/missing.
- The `user_preferences` DB write is best-effort: a failure must be logged and must never prevent login, never throw past `recordSuccessfulLogin`, and must never block or alter the existing `redirect()` calls.
- Do not convert the entire login page to a client component. Only add a thin Server Component wrapper; the existing client component's existing reason for being client (`useSearchParams`) is untouched.
- Do not create an endpoint that looks up `last_login_method` by email — that enables account enumeration.
- Do not change session lifetime, JWT expiry, refresh tokens, PKCE, middleware auth checks, or any existing redirect target/logic.
- `user_preferences` RLS: a user may only read/write their own row (`auth.uid() = user_id`). No `USING (true)` / `WITH CHECK (true)` policies. No service-role client introduced for this feature — writes go through the caller's own authenticated Supabase client, same as every other user-owned write in this codebase.
- Follow this repo's existing migration convention exactly (see `supabase/migrations/20260723050000_personal_access_tokens.sql`): `revoke all ... from public, anon, authenticated` followed by explicit narrow `grant`s.

---

## File Structure

| File | Responsibility |
|---|---|
| `ui/src/lib/server/loginMethod.ts` (new) | `LoginMethod` type, `LAST_LOGIN_METHOD_COOKIE`, `parseLoginMethod()`, `getLastLoginMethod()`, `recordSuccessfulLogin()`, `shouldShowLastUsedBadge()`. The ONLY place cookie options/name are defined. |
| `ui/src/lib/server/loginMethod.test.ts` (new) | Unit tests for the five exports above. |
| `ui/vitest.config.ts` (modify) | Add the new test file's path to `test.include` — **without this, Vitest silently never runs it**, since this config uses an explicit allowlist, not a project-wide glob. |
| `supabase/migrations/20260920000000_user_login_preferences.sql` (new) | `public.user_preferences` table + RLS. |
| `ui/src/app/login/LoginForm.tsx` (new) | The existing client-side login form, moved as-is from `page.tsx`, plus a new `lastUsedMethod` prop driving the three badge spots. |
| `ui/src/app/login/page.tsx` (rewrite) | Becomes a Server Component: reads the cookie via `getLastLoginMethod()`, renders `<LoginForm lastUsedMethod={...} />`. |
| `ui/src/app/login/actions.ts` (modify) | `login()` calls `recordSuccessfulLogin(supabase, data.user.id, 'email')` right after the existing error check passes. |
| `ui/src/app/login/actions.test.ts` (new) | Verifies `login()` calls (or doesn't call) `recordSuccessfulLogin` correctly on success/failure. |
| `ui/src/app/auth/callback/route.ts` (modify) | `GET()` derives the provider from `data.session.user`, validates it, calls `recordSuccessfulLogin`. |
| `ui/src/app/auth/callback/route.test.ts` (new) | Verifies `GET()` calls (or doesn't call) `recordSuccessfulLogin` correctly on success/failure, and that an unrecognized provider is never recorded. |

---

### Task 1: `parseLoginMethod()` — the strict validator

**Files:**
- Create: `ui/src/lib/server/loginMethod.ts`
- Test: `ui/src/lib/server/loginMethod.test.ts`

**Interfaces:**
- Produces: `export type LoginMethod = 'google' | 'github' | 'email'`, `export const LAST_LOGIN_METHOD_COOKIE = 'valori_last_login_method'`, `export function parseLoginMethod(value: string | undefined | null): LoginMethod | null`

- [ ] **Step 1: Write the failing test**

```ts
// ui/src/lib/server/loginMethod.test.ts
import { describe, it, expect } from 'vitest'
import { parseLoginMethod, LAST_LOGIN_METHOD_COOKIE } from './loginMethod'

describe('parseLoginMethod', () => {
  it('accepts the three known methods', () => {
    expect(parseLoginMethod('google')).toBe('google')
    expect(parseLoginMethod('github')).toBe('github')
    expect(parseLoginMethod('email')).toBe('email')
  })

  it('rejects an unknown provider string', () => {
    expect(parseLoginMethod('facebook')).toBeNull()
  })

  it('rejects garbage input', () => {
    expect(parseLoginMethod('abc')).toBeNull()
  })

  it('rejects empty string, undefined, and null', () => {
    expect(parseLoginMethod('')).toBeNull()
    expect(parseLoginMethod(undefined)).toBeNull()
    expect(parseLoginMethod(null)).toBeNull()
  })
})

describe('LAST_LOGIN_METHOD_COOKIE', () => {
  it('is the exact cookie name the feature is specified against', () => {
    expect(LAST_LOGIN_METHOD_COOKIE).toBe('valori_last_login_method')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `ui/`): `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: FAIL — `loginMethod.ts` doesn't exist yet (module not found).

- [ ] **Step 3: Write minimal implementation**

```ts
// ui/src/lib/server/loginMethod.ts
export type LoginMethod = 'google' | 'github' | 'email'

export const LAST_LOGIN_METHOD_COOKIE = 'valori_last_login_method'

const KNOWN_METHODS: readonly LoginMethod[] = ['google', 'github', 'email']

/**
 * Cookies (and Supabase's app_metadata.provider) are untrusted input —
 * never `value as LoginMethod`. Anything outside the three known methods,
 * including missing/empty, resolves to null and the caller treats that
 * exactly like "no preference recorded yet."
 */
export function parseLoginMethod(value: string | undefined | null): LoginMethod | null {
  if (!value) return null
  return (KNOWN_METHODS as readonly string[]).includes(value) ? (value as LoginMethod) : null
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: PASS (6 tests)

- [ ] **Step 5: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/src/lib/server/loginMethod.ts ui/src/lib/server/loginMethod.test.ts
git commit -m "feat(auth): add LoginMethod type and strict cookie-value parser"
```

---

### Task 2: `getLastLoginMethod()` — server-side cookie read

**Files:**
- Modify: `ui/src/lib/server/loginMethod.ts`
- Modify: `ui/src/lib/server/loginMethod.test.ts`

**Interfaces:**
- Consumes: `parseLoginMethod` (Task 1), `next/headers`'s `cookies()`.
- Produces: `export async function getLastLoginMethod(): Promise<LoginMethod | null>`

- [ ] **Step 1: Write the failing test**

Append to `loginMethod.test.ts`:

```ts
import { getLastLoginMethod } from './loginMethod'

const mockCookieGet = vi.fn()
vi.mock('next/headers', () => ({
  cookies: async () => ({ get: mockCookieGet }),
}))

describe('getLastLoginMethod', () => {
  it('returns the parsed cookie value when present and valid', async () => {
    mockCookieGet.mockReturnValueOnce({ value: 'github' })
    expect(await getLastLoginMethod()).toBe('github')
  })

  it('returns null when the cookie is absent', async () => {
    mockCookieGet.mockReturnValueOnce(undefined)
    expect(await getLastLoginMethod()).toBeNull()
  })

  it('returns null when the cookie holds an invalid value', async () => {
    mockCookieGet.mockReturnValueOnce({ value: 'facebook' })
    expect(await getLastLoginMethod()).toBeNull()
  })
})
```

Add `import { vi } from 'vitest'` to the existing `vitest` import at the top of the file (it currently only imports `describe, it, expect`).

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: FAIL — `getLastLoginMethod is not a function`

- [ ] **Step 3: Write minimal implementation**

Append to `loginMethod.ts`:

```ts
import { cookies } from 'next/headers'

/** Server-side read only — call this from a Server Component or Server
 * Action/Route Handler, never from client code (there is no client-side
 * equivalent; the login page reads this before first paint instead of
 * after hydration, see login/page.tsx). */
export async function getLastLoginMethod(): Promise<LoginMethod | null> {
  const cookieStore = await cookies()
  return parseLoginMethod(cookieStore.get(LAST_LOGIN_METHOD_COOKIE)?.value)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: PASS (9 tests)

- [ ] **Step 5: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/src/lib/server/loginMethod.ts ui/src/lib/server/loginMethod.test.ts
git commit -m "feat(auth): add server-side last-login-method cookie reader"
```

---

### Task 3: `recordSuccessfulLogin()` — the one writer (cookie + best-effort DB)

**Files:**
- Modify: `ui/src/lib/server/loginMethod.ts`
- Modify: `ui/src/lib/server/loginMethod.test.ts`

**Interfaces:**
- Consumes: `LoginMethod`, `LAST_LOGIN_METHOD_COOKIE` (Task 1); `supabaseCookieOptions` from `./cookieOptions.ts` (existing); `next/headers`'s `cookies()` and `headers()`.
- Produces: `export async function recordSuccessfulLogin(supabase: SupabaseClient, userId: string, method: LoginMethod): Promise<void>` — never throws.

- [ ] **Step 1: Write the failing test**

Append to `loginMethod.test.ts`:

```ts
import { recordSuccessfulLogin } from './loginMethod'

const mockCookieSet = vi.fn()
const mockHeadersGet = vi.fn()
vi.mock('next/headers', () => ({
  cookies: async () => ({ get: mockCookieGet, set: mockCookieSet }),
  headers: async () => ({ get: mockHeadersGet }),
}))

function fakeSupabase(upsertResult: { error: unknown }) {
  const upsert = vi.fn(async () => upsertResult)
  return { from: vi.fn(() => ({ upsert })), _upsert: upsert }
}

describe('recordSuccessfulLogin', () => {
  it('sets the cookie with the given method', async () => {
    mockHeadersGet.mockReturnValue('app.valori.systems')
    const supabase = fakeSupabase({ error: null })
    await recordSuccessfulLogin(supabase as never, 'user-1', 'google')
    expect(mockCookieSet).toHaveBeenCalledWith(
      LAST_LOGIN_METHOD_COOKIE,
      'google',
      expect.objectContaining({
        path: '/',
        sameSite: 'lax',
        httpOnly: false,
        domain: '.valori.systems',
        secure: true,
      }),
    )
  })

  it('does not set domain/secure on a non-valori.systems host (e.g. localhost)', async () => {
    mockHeadersGet.mockReturnValue('localhost:3002')
    const supabase = fakeSupabase({ error: null })
    await recordSuccessfulLogin(supabase as never, 'user-1', 'email')
    const options = mockCookieSet.mock.calls.at(-1)?.[2]
    expect(options.domain).toBeUndefined()
    expect(options.secure).toBe(false)
  })

  it('attempts to upsert user_preferences with the user id and method', async () => {
    mockHeadersGet.mockReturnValue('app.valori.systems')
    const supabase = fakeSupabase({ error: null })
    await recordSuccessfulLogin(supabase as never, 'user-42', 'github')
    expect(supabase.from).toHaveBeenCalledWith('user_preferences')
    expect(supabase._upsert).toHaveBeenCalledWith(
      expect.objectContaining({ user_id: 'user-42', last_login_method: 'github' }),
    )
  })

  it('never throws when the DB upsert fails — cookie is still set', async () => {
    mockHeadersGet.mockReturnValue('app.valori.systems')
    const supabase = fakeSupabase({ error: new Error('db down') })
    await expect(recordSuccessfulLogin(supabase as never, 'user-1', 'email')).resolves.toBeUndefined()
    expect(mockCookieSet).toHaveBeenCalled()
  })

  it('never throws when the DB upsert itself rejects', async () => {
    mockHeadersGet.mockReturnValue('app.valori.systems')
    const supabase = { from: vi.fn(() => ({ upsert: vi.fn(async () => { throw new Error('network') }) })) }
    await expect(recordSuccessfulLogin(supabase as never, 'user-1', 'email')).resolves.toBeUndefined()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: FAIL — `recordSuccessfulLogin is not a function`

- [ ] **Step 3: Write minimal implementation**

Add this type import and function to `loginMethod.ts` (keep the existing `cookies` import, add `headers` alongside it, and import `supabaseCookieOptions`):

```ts
import { cookies, headers } from 'next/headers'
import type { SupabaseClient } from '@supabase/supabase-js'
import { supabaseCookieOptions } from '@/utils/supabase/cookieOptions'

const ONE_YEAR_SECONDS = 60 * 60 * 24 * 365

/**
 * The ONE place that writes the last-used-login-method cookie and the
 * user_preferences row. Both auth/callback/route.ts (OAuth) and
 * login/actions.ts (email) call this — neither implements its own
 * cookie-writing logic.
 *
 * The DB write is best-effort: a failure here must never prevent a
 * successful authentication from completing. The cookie write (the part
 * that actually drives the UI) happens first and unconditionally.
 */
export async function recordSuccessfulLogin(
  supabase: SupabaseClient,
  userId: string,
  method: LoginMethod,
): Promise<void> {
  const hostname = (await headers()).get('host')
  const domainOptions = supabaseCookieOptions(hostname ?? undefined)
  const isValoriHost = Boolean(hostname?.endsWith('valori.systems'))

  const cookieStore = await cookies()
  cookieStore.set(LAST_LOGIN_METHOD_COOKIE, method, {
    path: '/',
    sameSite: 'lax',
    httpOnly: false,
    maxAge: ONE_YEAR_SECONDS,
    domain: domainOptions?.domain,
    secure: isValoriHost,
  })

  try {
    const { error } = await supabase
      .from('user_preferences')
      .upsert({ user_id: userId, last_login_method: method, updated_at: new Date().toISOString() })
    if (error) {
      console.error('[recordSuccessfulLogin] user_preferences upsert failed', error)
    }
  } catch (e) {
    console.error('[recordSuccessfulLogin] user_preferences upsert threw', e)
  }
}
```

Note: `supabaseCookieOptions(hostname)` returns `{ domain: '.valori.systems' }` or `undefined` — reused verbatim, no new domain-detection logic. `isValoriHost` mirrors that exact same condition for `secure`, per the Global Constraints.

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: PASS (14 tests)

- [ ] **Step 5: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/src/lib/server/loginMethod.ts ui/src/lib/server/loginMethod.test.ts
git commit -m "feat(auth): add recordSuccessfulLogin (cookie + best-effort user_preferences write)"
```

---

### Task 4: `shouldShowLastUsedBadge()` — the testable UI decision

**Why this exists:** This codebase's Vitest setup runs in a Node environment with no DOM/React Testing Library (see `vitest.config.ts`) — there is no existing precedent for rendering-based component tests, and adding one (installing `@testing-library/react` + `jsdom`) would be new test infrastructure beyond this feature's scope, contradicting the "no unnecessary dependencies" constraint. To still give the *exact* per-method badge logic real automated coverage (not just eyeballing it), it's pulled out as a pure function `LoginForm.tsx` calls three times, instead of being three inline JSX ternaries nobody can unit-test.

**Files:**
- Modify: `ui/src/lib/server/loginMethod.ts`
- Modify: `ui/src/lib/server/loginMethod.test.ts`

**Interfaces:**
- Consumes: `LoginMethod` (Task 1)
- Produces: `export function shouldShowLastUsedBadge(method: LoginMethod, lastUsed: LoginMethod | null): boolean`

- [ ] **Step 1: Write the failing test**

Append to `loginMethod.test.ts`:

```ts
import { shouldShowLastUsedBadge } from './loginMethod'

describe('shouldShowLastUsedBadge', () => {
  it('is true only for the matching method', () => {
    expect(shouldShowLastUsedBadge('google', 'google')).toBe(true)
    expect(shouldShowLastUsedBadge('github', 'google')).toBe(false)
    expect(shouldShowLastUsedBadge('email', 'google')).toBe(false)
  })

  it('is false for every method when nothing was recorded yet', () => {
    expect(shouldShowLastUsedBadge('google', null)).toBe(false)
    expect(shouldShowLastUsedBadge('github', null)).toBe(false)
    expect(shouldShowLastUsedBadge('email', null)).toBe(false)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: FAIL — `shouldShowLastUsedBadge is not a function`

- [ ] **Step 3: Write minimal implementation**

Append to `loginMethod.ts`:

```ts
/** Exactly one of the three login-page spots should ever show the badge
 * at a time — this is the single decision point both the OAuth buttons
 * and the email section call, so that invariant lives in one tested
 * place instead of three copies of `lastUsed === 'x'`. */
export function shouldShowLastUsedBadge(method: LoginMethod, lastUsed: LoginMethod | null): boolean {
  return lastUsed === method
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/lib/server/loginMethod.test.ts`
Expected: PASS (16 tests)

- [ ] **Step 5: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/src/lib/server/loginMethod.ts ui/src/lib/server/loginMethod.test.ts
git commit -m "feat(auth): add shouldShowLastUsedBadge decision helper"
```

---

### Task 5: Register the new tests in Vitest's include list

**Files:**
- Modify: `ui/vitest.config.ts`

**Interfaces:**
- Consumes: nothing new.
- Produces: nothing new — this task only makes Tasks 1-4's tests (and Tasks 6-7's, added next) actually run under `npm run test:demo-rag`.

- [ ] **Step 1: Confirm the gap** (this "test" is the run itself, not a new file)

Run (from `ui/`): `npm run test:demo-rag`
Expected: the suite passes, but its summary does NOT mention `loginMethod.test.ts` at all — `vitest.config.ts`'s `test.include` is an explicit allowlist, not a project-wide glob, so a new test file outside the listed paths is silently never picked up.

- [ ] **Step 2: Add the new paths**

In `ui/vitest.config.ts`, add these two lines inside the existing `include` array (alongside `'src/lib/hosts.test.ts'` and `'src/proxy.test.ts'` at the end):

```ts
      'src/lib/server/loginMethod.test.ts',
      'src/app/login/actions.test.ts',
      'src/app/auth/callback/route.test.ts',
```

(The last two don't exist yet — they're created in Tasks 6-7 — adding all three paths now avoids a second edit to this file later.)

- [ ] **Step 3: Run test to verify the new file is now picked up**

Run: `npm run test:demo-rag`
Expected: the summary now lists `src/lib/server/loginMethod.test.ts` with its 16 passing tests, alongside the pre-existing suites (still all green — nothing pre-existing should regress from this config-only change).

- [ ] **Step 4: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/vitest.config.ts
git commit -m "test(auth): register last-login-method test files in vitest include list"
```

---

### Task 6: Wire `recordSuccessfulLogin` into email login

**Files:**
- Modify: `ui/src/app/login/actions.ts:11-69` (the `login()` function)
- Test: `ui/src/app/login/actions.test.ts`

**Interfaces:**
- Consumes: `recordSuccessfulLogin` (Task 3), from `@/lib/server/loginMethod`.
- Produces: nothing new for other tasks — this is a leaf integration point.

- [ ] **Step 1: Write the failing test**

```ts
// ui/src/app/login/actions.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockRecordSuccessfulLogin = vi.fn()
vi.mock('@/lib/server/loginMethod', () => ({
  recordSuccessfulLogin: mockRecordSuccessfulLogin,
}))

const mockRedirect = vi.fn((url: string) => {
  throw new Error(`REDIRECT:${url}`)
})
vi.mock('next/navigation', () => ({ redirect: mockRedirect }))
vi.mock('next/cache', () => ({ revalidatePath: vi.fn() }))
vi.mock('@/lib/server/mfa', () => ({ mfaChallengeRedirect: vi.fn(async () => null) }))
vi.mock('@/lib/server/app-url', () => ({ appRedirectUrl: vi.fn(async (path: string) => path) }))
vi.mock('@/lib/server/http', () => ({
  getRequestContext: vi.fn(async () => ({ ip: '127.0.0.1', userAgent: 'test' })),
}))

function fakeSupabase(signInResult: { data: { user: { id: string } | null }; error: unknown }) {
  return {
    rpc: vi.fn(async () => ({ data: true })),
    auth: { signInWithPassword: vi.fn(async () => signInResult) },
  }
}

let currentSupabase: ReturnType<typeof fakeSupabase>
vi.mock('@/utils/supabase/server', () => ({
  createClient: async () => currentSupabase,
}))

function loginFormData(email: string, password: string) {
  const fd = new FormData()
  fd.set('email', email)
  fd.set('password', password)
  fd.set('next', '/dashboard')
  return fd
}

describe('login() — last-login-method recording', () => {
  beforeEach(() => {
    mockRecordSuccessfulLogin.mockClear()
    mockRedirect.mockClear()
  })

  it('records "email" after a successful sign-in', async () => {
    currentSupabase = fakeSupabase({ data: { user: { id: 'user-1' } }, error: null })
    const { login } = await import('./actions')
    await expect(login(loginFormData('a@b.com', 'correct-password'))).rejects.toThrow('REDIRECT:')
    expect(mockRecordSuccessfulLogin).toHaveBeenCalledWith(currentSupabase, 'user-1', 'email')
  })

  it('does not record anything after a failed sign-in', async () => {
    currentSupabase = fakeSupabase({ data: { user: null }, error: { message: 'Invalid credentials' } })
    const { login } = await import('./actions')
    await expect(login(loginFormData('a@b.com', 'wrong-password'))).rejects.toThrow('REDIRECT:')
    expect(mockRecordSuccessfulLogin).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/app/login/actions.test.ts`
Expected: FAIL on the first assertion — `recordSuccessfulLogin` was never called, since `actions.ts` doesn't call it yet.

- [ ] **Step 3: Write minimal implementation**

In `ui/src/app/login/actions.ts`, add the import at the top (alongside the existing ones):

```ts
import { recordSuccessfulLogin } from '@/lib/server/loginMethod'
```

Change line 40 from:

```ts
    const { error } = await supabase.auth.signInWithPassword({
        email,
        password,
    })
```

to:

```ts
    const { data, error } = await supabase.auth.signInWithPassword({
        email,
        password,
    })
```

Then, right after the existing `if (error) { redirect(...) }` block (after line 63) and before `revalidatePath('/', 'layout')` (line 65), add:

```ts
    if (data.user) {
        await recordSuccessfulLogin(supabase, data.user.id, 'email')
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/app/login/actions.test.ts`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/src/app/login/actions.ts ui/src/app/login/actions.test.ts
git commit -m "feat(auth): record 'email' as last login method on successful sign-in"
```

---

### Task 7: Wire `recordSuccessfulLogin` into the OAuth callback

**Files:**
- Modify: `ui/src/app/auth/callback/route.ts`
- Test: `ui/src/app/auth/callback/route.test.ts`

**Interfaces:**
- Consumes: `recordSuccessfulLogin`, `parseLoginMethod` (Tasks 1, 3), from `@/lib/server/loginMethod`.
- Produces: nothing new for other tasks — leaf integration point.

- [ ] **Step 1: Write the failing test**

```ts
// ui/src/app/auth/callback/route.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockRecordSuccessfulLogin = vi.fn()
vi.mock('@/lib/server/loginMethod', async () => {
  const actual = await vi.importActual<typeof import('@/lib/server/loginMethod')>('@/lib/server/loginMethod')
  return { ...actual, recordSuccessfulLogin: mockRecordSuccessfulLogin }
})

vi.mock('@/lib/server/mfa', () => ({ mfaChallengeRedirect: vi.fn(async () => null) }))
vi.mock('@/lib/server/app-url', () => ({ appRedirectUrl: vi.fn(async (path: string) => path) }))

type ExchangeResult = {
  data: { session: { user: { id: string; app_metadata?: { provider?: string }; identities?: { provider: string }[] } } | null }
  error: { message: string } | null
}

let currentExchangeResult: ExchangeResult
vi.mock('@/utils/supabase/server', () => ({
  createClient: async () => ({
    auth: { exchangeCodeForSession: async () => currentExchangeResult },
  }),
}))

describe('GET /auth/callback — last-login-method recording', () => {
  beforeEach(() => {
    mockRecordSuccessfulLogin.mockClear()
  })

  it('records "google" from app_metadata.provider on a successful exchange', async () => {
    currentExchangeResult = {
      data: { session: { user: { id: 'user-1', app_metadata: { provider: 'google' } } } },
      error: null,
    }
    const { GET } = await import('./route')
    await GET(new Request('https://app.valori.systems/auth/callback?code=abc'))
    expect(mockRecordSuccessfulLogin).toHaveBeenCalledWith(expect.anything(), 'user-1', 'google')
  })

  it('falls back to identities[0].provider when app_metadata.provider is absent', async () => {
    currentExchangeResult = {
      data: { session: { user: { id: 'user-2', identities: [{ provider: 'github' }] } } },
      error: null,
    }
    const { GET } = await import('./route')
    await GET(new Request('https://app.valori.systems/auth/callback?code=abc'))
    expect(mockRecordSuccessfulLogin).toHaveBeenCalledWith(expect.anything(), 'user-2', 'github')
  })

  it('never records when exchangeCodeForSession fails', async () => {
    currentExchangeResult = { data: { session: null }, error: { message: 'invalid grant' } }
    const { GET } = await import('./route')
    await GET(new Request('https://app.valori.systems/auth/callback?code=bad'))
    expect(mockRecordSuccessfulLogin).not.toHaveBeenCalled()
  })

  it('never records an unrecognized provider value', async () => {
    currentExchangeResult = {
      data: { session: { user: { id: 'user-3', app_metadata: { provider: 'facebook' } } } },
      error: null,
    }
    const { GET } = await import('./route')
    await GET(new Request('https://app.valori.systems/auth/callback?code=abc'))
    expect(mockRecordSuccessfulLogin).not.toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run src/app/auth/callback/route.test.ts`
Expected: FAIL on the first assertion — `route.ts` doesn't derive or record a provider yet.

- [ ] **Step 3: Write minimal implementation**

In `ui/src/app/auth/callback/route.ts`, add the import:

```ts
import { recordSuccessfulLogin, parseLoginMethod } from '@/lib/server/loginMethod'
```

Right after `const { data, error } = await supabase.auth.exchangeCodeForSession(code)` and its `if (!error) {` opening (i.e., as the first statement inside that block, before `const mfaRedirect = await mfaChallengeRedirect(supabase, next)`), add:

```ts
            const rawProvider = data.session?.user?.app_metadata?.provider
                ?? data.session?.user?.identities?.[0]?.provider
                ?? null
            const method = parseLoginMethod(rawProvider)
            if (method && data.session?.user) {
                await recordSuccessfulLogin(supabase, data.session.user.id, method)
            }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run src/app/auth/callback/route.test.ts`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/src/app/auth/callback/route.ts ui/src/app/auth/callback/route.test.ts
git commit -m "feat(auth): record google/github as last login method after OAuth callback succeeds"
```

---

### Task 8: Split the login page — Server Component wrapper + client `LoginForm`

**Files:**
- Create: `ui/src/app/login/LoginForm.tsx`
- Rewrite: `ui/src/app/login/page.tsx`

**Interfaces:**
- Consumes: `getLastLoginMethod`, `shouldShowLastUsedBadge`, `LoginMethod` (Tasks 1, 2, 4).
- Produces: nothing consumed by later tasks.

No new automated test here (see Task 4's note — JSX rendering isn't covered by this repo's Node-only Vitest setup); Step 6 below is a manual browser check instead, same verification depth `preview_start`/browser-pane testing already used earlier in this session for this exact login page.

- [ ] **Step 1: Create `LoginForm.tsx` with the existing form logic, plus the prop and badges**

```tsx
// ui/src/app/login/LoginForm.tsx
'use client'

import { login, signup } from './actions'
import { createClient } from '@/utils/supabase/client'
import { Github } from 'lucide-react'
import { GoogleIcon } from '@/components/icons/google-icon'
import { AuthShell } from '@/components/auth/auth-shell'
import { useSearchParams } from 'next/navigation'
import Link from 'next/link'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { shouldShowLastUsedBadge, type LoginMethod } from '@/lib/server/loginMethod'

async function signInWithOAuth(provider: 'google' | 'github', next?: string | null, desktop?: boolean) {
    const supabase = createClient()
    const canonicalOrigin = window.location.origin.replace('://www.', '://')
    const params = new URLSearchParams()
    if (next) params.set('next', next)
    if (desktop) params.set('desktop', '1')
    const query = params.toString()
    const redirectUrl = `${canonicalOrigin}/auth/callback${query ? `?${query}` : ''}`

    const { data, error } = await supabase.auth.signInWithOAuth({
        provider,
        options: {
            redirectTo: redirectUrl,
            queryParams: {
                access_type: 'offline',
                prompt: 'consent',
            },
        },
    })

    if (error) {
        window.location.href = `/error?message=${encodeURIComponent(error.message)}`
        return
    }

    if (data.url) {
        window.location.href = data.url
    }
}

export function LoginForm({ lastUsedMethod }: { lastUsedMethod: LoginMethod | null }) {
    const searchParams = useSearchParams()
    const next = searchParams.get('next')
    const desktop = searchParams.get('desktop') === '1'

    return (
        <AuthShell
            title="Sign in to Valori Cloud"
            description="Access your projects, collections, and vector infrastructure."
            footer={
                <>
                    Don&apos;t have an account?{' '}
                    <Button type="submit" form="login-form" formAction={signup} variant="link" className="px-1">
                        Create account
                    </Button>
                </>
            }
        >
            <div className="space-y-3">
                <Button
                    variant="outline"
                    className="flex w-full items-center justify-between gap-3 font-medium"
                    onClick={() => signInWithOAuth('google', next, desktop)}
                >
                    <span className="flex items-center gap-3">
                        <GoogleIcon className="h-4 w-4" />
                        Continue with Google
                    </span>
                    {shouldShowLastUsedBadge('google', lastUsedMethod) && (
                        <Badge variant="secondary">Last used</Badge>
                    )}
                </Button>
                <Button
                    variant="outline"
                    className="flex w-full items-center justify-between gap-3 font-medium"
                    onClick={() => signInWithOAuth('github', next, desktop)}
                >
                    <span className="flex items-center gap-3">
                        <Github size={16} />
                        Continue with GitHub
                    </span>
                    {shouldShowLastUsedBadge('github', lastUsedMethod) && (
                        <Badge variant="secondary">Last used</Badge>
                    )}
                </Button>
            </div>

            <div className="my-6 flex items-center gap-3">
                <div className="h-px flex-1 bg-border" />
                <span className="text-xs text-muted-foreground">or</span>
                <div className="h-px flex-1 bg-border" />
            </div>

            {shouldShowLastUsedBadge('email', lastUsedMethod) && (
                <div className="mb-3 flex items-center justify-between">
                    <span className="text-2xs font-semibold text-muted-foreground uppercase tracking-wider">Email</span>
                    <Badge variant="secondary">Last used</Badge>
                </div>
            )}

            <form id="login-form" className="space-y-4">
                <input type="hidden" name="next" value={next ?? ''} />
                <div>
                    <label htmlFor="email" className="mb-1.5 block text-sm font-medium text-foreground">
                        Email address
                    </label>
                    <input
                        id="email"
                        name="email"
                        type="email"
                        autoComplete="username"
                        required
                        className="block w-full rounded-lg border border-input bg-background px-3 py-2.5 text-sm text-foreground placeholder:text-muted-foreground focus-visible:border-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                        placeholder="you@example.com"
                    />
                </div>
                <div>
                    <div className="mb-1.5 flex items-center justify-between">
                        <label htmlFor="password" className="block text-sm font-medium text-foreground">
                            Password
                        </label>
                        <Link href="/forgot-password" className="text-xs text-muted-foreground hover:text-primary">
                            Forgot password?
                        </Link>
                    </div>
                    <input
                        id="password"
                        name="password"
                        type="password"
                        autoComplete="current-password"
                        required
                        className="block w-full rounded-lg border border-input bg-background px-3 py-2.5 text-sm text-foreground placeholder:text-muted-foreground focus-visible:border-ring focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                        placeholder="••••••••"
                    />
                </div>

                <Button type="submit" formAction={login} className="w-full">
                    Sign in
                </Button>
            </form>
        </AuthShell>
    )
}
```

- [ ] **Step 2: Rewrite `page.tsx` as a Server Component**

```tsx
// ui/src/app/login/page.tsx
import { Suspense } from 'react'
import { LoginForm } from './LoginForm'
import { getLastLoginMethod } from '@/lib/server/loginMethod'

export default async function LoginPage() {
    const lastUsedMethod = await getLastLoginMethod()

    return (
        <Suspense fallback={<div className="flex min-h-screen items-center justify-center text-sm text-muted-foreground">Loading…</div>}>
            <LoginForm lastUsedMethod={lastUsedMethod} />
        </Suspense>
    )
}
```

Note what's deliberately gone from the old `page.tsx`: the `mounted`/`atLimit`-style `'use client'` directive, and the inline `LoginForm` function — both moved to `LoginForm.tsx` unchanged in behavior. `page.tsx` itself has no `'use client'` at all now — it's a genuine Server Component, so `getLastLoginMethod()`'s `next/headers` read is valid here and resolves before the client bundle for `LoginForm` even ships, eliminating the hydration-flicker case entirely.

- [ ] **Step 3: Typecheck**

Run (from `ui/`): `npx tsc --noEmit -p .`
Expected: no new errors. (If `Badge`'s props don't accept plain children the way used above, this step will surface it — check `components/ui/badge.tsx`'s existing usage elsewhere in the codebase, e.g. `status-badge.tsx`, for the exact accepted prop shape and adjust the JSX above to match, without changing `badge.tsx` itself.)

- [ ] **Step 4: Manual verification — no cookie (first-time visitor)**

Start the dev server: `cd /Users/as-mac-0272/Desktop/sass/valori-ui/ui && npm run dev -- -p 3002` (or reuse the `valori-cloud-dashboard` launch config from `.claude/launch.json` in the `Valori-Kernel` repo, as used earlier this session).
Open `http://localhost:3002/login` in a browser with no `valori_last_login_method` cookie set.
Expected: neither OAuth button nor the email section shows a "Last used" badge; page looks identical to before this change.

- [ ] **Step 5: Manual verification — cookie present**

In the browser devtools console, run `document.cookie = 'valori_last_login_method=google; path=/'`, then reload `/login`.
Expected: the Google button shows a muted "Last used" badge on its right side; GitHub and the email section show none. Repeat with `github` and `email` values — confirm the badge always follows the cookie value and only one spot shows it at a time. Set the cookie to `facebook` and reload — expected: no badge anywhere, page renders normally (proves `parseLoginMethod`'s invalid-value handling reaches all the way through to the rendered page).

- [ ] **Step 6: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add ui/src/app/login/LoginForm.tsx ui/src/app/login/page.tsx
git commit -m "feat(auth): show a 'Last used' badge on the login page, server-rendered"
```

---

### Task 9: `user_preferences` migration

**Files:**
- Create: `supabase/migrations/20260920000000_user_login_preferences.sql`

**Interfaces:**
- Consumes: nothing (standalone DDL).
- Produces: the `public.user_preferences` table that `recordSuccessfulLogin` (Task 3) already upserts into — Task 3's code was written against this exact shape, so this migration must match it precisely (`user_id`, `last_login_method`, `updated_at` column names).

- [ ] **Step 1: Write the migration**

```sql
-- Backs the "last used login method" feature: which of google/github/email
-- a user most recently authenticated with successfully, written by
-- ui/src/lib/server/loginMethod.ts's recordSuccessfulLogin() on every
-- successful login. The signed-out login page itself never reads this
-- table directly (it doesn't know which account is about to sign in yet —
-- see that file's own comments); it's written for future cross-device use
-- if Valori ever moves to an identity-first login flow. The per-browser
-- `valori_last_login_method` cookie is what actually drives today's UI.

create table public.user_preferences (
  user_id           uuid primary key references auth.users(id) on delete cascade,
  last_login_method text check (last_login_method is null or last_login_method in ('google', 'github', 'email')),
  updated_at        timestamptz not null default now()
);

comment on table public.user_preferences is
  'Per-user settings that are neither org-scoped nor project-scoped. '
  'Started with last_login_method; add columns here rather than creating '
  'a new single-purpose table for the next per-user preference.';

alter table public.user_preferences enable row level security;

-- Strictly per-user, same reasoning as personal_access_tokens
-- (20260723050000): no org role concept applies, a preference row acts
-- on behalf of its own owner only.
create policy user_preferences_select on public.user_preferences
  for select to authenticated
  using (user_id = auth.uid());

create policy user_preferences_insert on public.user_preferences
  for insert to authenticated
  with check (user_id = auth.uid());

create policy user_preferences_update on public.user_preferences
  for update to authenticated
  using (user_id = auth.uid())
  with check (user_id = auth.uid());

revoke all on public.user_preferences from public, anon, authenticated;
grant select on public.user_preferences to authenticated;
grant insert (user_id, last_login_method, updated_at) on public.user_preferences to authenticated;
grant update (last_login_method, updated_at) on public.user_preferences to authenticated;
```

- [ ] **Step 2: Review for idempotency against this session's own recent lesson**

This session found, twice today, that a migration written correctly can still fail against production if a PRIOR migration attempt partially applied. Before running this: this is a brand-new table (`create table`, not `alter table` on something that might already exist), so there is no partial-application risk the way there was with `runtime_profiles`/`projects` earlier today — a single clean `create table` either fully succeeds or fully fails, nothing to reconcile. No `IF NOT EXISTS` guard is needed or added, matching this repo's other `create table` migrations (e.g. `20260723050000_personal_access_tokens.sql` has none either).

- [ ] **Step 3: Apply it**

This repo's migrations are applied manually via the Supabase SQL editor (confirmed this session — no `supabase` CLI is installed, and no auto-migrate step ran against production for the SH2 migration either). Paste this file's contents into the Supabase SQL editor for the project and run it. Do not apply it via any other path.

- [ ] **Step 4: Verify it applied cleanly**

Run in the SQL editor:

```sql
select count(*) as table_exists from information_schema.tables where table_schema='public' and table_name='user_preferences';
select count(*) as policy_count from pg_policies where schemaname='public' and tablename='user_preferences';
```

Expected: `table_exists = 1`, `policy_count = 3`.

- [ ] **Step 5: Commit**

```bash
cd /Users/as-mac-0272/Desktop/sass/valori-ui
git add supabase/migrations/20260920000000_user_login_preferences.sql
git commit -m "feat(db): add user_preferences table for last_login_method"
```

---

### Task 10: Full verification pass + final report

**Files:** none (verification only).

- [ ] **Step 1: Typecheck**

Run (from `ui/`): `npx tsc --noEmit -p .`
Record: pass/fail and any errors.

- [ ] **Step 2: Lint**

Run (from `ui/`): `npm run lint`
Record: pass/fail and any errors. Fix anything this feature's new files introduce; do not touch unrelated pre-existing lint state.

- [ ] **Step 3: Test**

Run (from `ui/`): `npm run test:demo-rag`
Record: total pass count, confirm all of `loginMethod.test.ts` (16), `actions.test.ts` (2), `route.test.ts` (4) — 22 new tests — are included and green, alongside the pre-existing suite with no regressions.

- [ ] **Step 4: Build**

Run (from `ui/`): `npm run build`
Record: pass/fail. This is the step that would catch a Server/Client Component boundary violation from Task 8's split (e.g. if `next/headers` were accidentally imported into a `'use client'` file) — Next.js's build fails loudly on that, it doesn't silently degrade.

- [ ] **Step 5: Confirm logout does not clear the preference cookie**

Run: `grep -rn "signOut\|valori_last_login_method" ui/src/components/Header.tsx ui/src/components/layout/AppSidebar.tsx`
Expected: both files call `supabase.auth.signOut()` and neither references `valori_last_login_method` (or `cookies().delete(...)`/`cookieStore.delete(...)` at all) anywhere near it. This confirms by inspection — not by a unit test, since the behavior being verified is Supabase's own `signOut()` implementation, not code in this repo — that logout only clears Supabase's own `sb-*` session cookies and never touches this feature's cookie. If either file DOES reference deleting cookies generically near the `signOut()` call, stop and re-examine before proceeding — that would be a pre-existing pattern this feature's cookie could get caught by.

- [ ] **Step 6: Confirm no existing auth semantics changed**

Run: `git diff main -- ui/src/app/login/actions.ts ui/src/app/auth/callback/route.ts ui/src/utils/supabase/middleware.ts ui/src/utils/supabase/cookieOptions.ts` (adjust the base ref if this feature branch was cut from something other than `main`).
Expected: `middleware.ts` and `cookieOptions.ts` show **zero diff** (this feature only reads `supabaseCookieOptions`, never modifies it); `actions.ts`'s diff is exactly the `data` destructure change (Task 6) plus the new `if (data.user) { await recordSuccessfulLogin(...) }` block — no changes to the rate-limiting, `signInWithPassword` call itself, MFA branch, or any `redirect()` target; `route.ts`'s diff is exactly the new import plus the provider-detection block from Task 7 — no changes to `exchangeCodeForSession`, the desktop-handoff branch, the MFA branch, or any redirect target. If any diff shows more than these additive blocks, stop and reduce it before reporting completion.

- [ ] **Step 7: Compile the final report**

Using the results above, answer each of these (this is the exact 15-point report format requested for this feature):

1. Files changed — list every file from the File Structure table above with created/modified.
2. Migration added — `supabase/migrations/20260920000000_user_login_preferences.sql`, and whether Task 9 Step 4's verification query confirmed it applied.
3. Where `LoginMethod` is defined — `ui/src/lib/server/loginMethod.ts`.
4. Where the cookie is written — `recordSuccessfulLogin()` in the same file; called from `login/actions.ts`'s `login()` and `auth/callback/route.ts`'s `GET()`, nowhere else.
5. How Google/GitHub provider detection works — `data.session.user.app_metadata?.provider ?? data.session.user.identities?.[0]?.provider`, read right after `exchangeCodeForSession()` succeeds, validated through `parseLoginMethod()` before ever being recorded; never a query parameter.
6. Where email login records success — `login/actions.ts`'s `login()`, immediately after the existing `signInWithPassword` error check confirms no error, before the final `redirect()`.
7. How the login page reads the cookie server-side — `page.tsx` (now a Server Component, no `'use client'`) calls `getLastLoginMethod()` before rendering `<LoginForm>`, so the badge is present in the initial server-rendered HTML.
8. How `user_preferences` is protected with RLS — three policies (`select`/`insert`/`update`), all gated on `user_id = auth.uid()`; table-level grants revoked from `public`/`anon`/`authenticated` first, then re-granted narrowly per Task 9's migration.
9. What happens if DB preference persistence fails — logged via `console.error` inside a `try/catch` in `recordSuccessfulLogin`; the function still returns normally, the cookie is already set by that point, and the caller's own `redirect()` proceeds unaffected (proven by Task 3's and Task 6/7's "never throws" tests).
10. Tests added — 22 total: 16 in `loginMethod.test.ts` (parser, cookie reader, writer including 2 DB-failure cases, badge-decision helper), 2 in `login/actions.test.ts`, 4 in `auth/callback/route.test.ts`.
11. Typecheck result — from Step 1.
12. Lint result — from Step 2.
13. Test result — from Step 3.
14. Production build result — from Step 4.
15. Security concerns / remaining limitations — no email-first lookup endpoint exists or was built (avoids account enumeration, per spec); `user_preferences` is written on every login but nothing reads it back yet, so cross-device "last used" isn't live — only the per-browser cookie drives today's UI; the migration has not been applied to any environment until Task 9 Step 3 is done by hand in the Supabase SQL editor, matching this repo's existing migration-application process.

- [ ] **Step 8: Report to the user**

Do not commit or push anything beyond what Tasks 1-9 already committed. Present the 15-point report from Step 7 directly in the conversation.

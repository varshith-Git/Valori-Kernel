import type { CookieOptionsWithName } from '@supabase/ssr'

const ROOT_DOMAIN = 'valori.systems'

/**
 * Shared by all three Supabase client constructors (browser/server/
 * middleware) so a session cookie set on one `*.valori.systems` host is
 * readable on another (valori.systems, app.valori.systems) — the default is
 * host-only (RFC 6265), which would otherwise make a user look logged out
 * the moment login redirects them to a different subdomain.
 *
 * Only rewrites the domain when `hostname` actually ends with
 * `valori.systems` — a Vercel preview URL or `localhost` can't have a
 * `Domain=.valori.systems` cookie set on it at all (the browser rejects a
 * Domain attribute that isn't the current host or a parent of it), so
 * forcing this unconditionally would silently break login there instead of
 * just misrouting a redirect. Checking the real request host is more
 * reliable than branching on `NODE_ENV`, since Vercel preview builds also
 * run with `NODE_ENV=production`.
 */
export function supabaseCookieOptions(hostname: string | null | undefined): CookieOptionsWithName | undefined {
    if (!hostname?.endsWith(ROOT_DOMAIN)) return undefined
    return { domain: `.${ROOT_DOMAIN}` }
}

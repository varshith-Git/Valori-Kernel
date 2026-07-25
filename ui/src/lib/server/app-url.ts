import { headers } from 'next/headers'

const APP_HOST = 'app.valori.systems'
const ROOT_DOMAIN = 'valori.systems'

/**
 * Every post-auth redirect that lands on `/dashboard` or `/admin` needs to
 * go to the app subdomain, not the marketing domain it may have started
 * on (login/signup/password-reset/MFA can all be entered from
 * `valori.systems`). Only rewrites when actually serving `*.valori.systems`
 * right now — a Vercel preview URL or `localhost` has no
 * `app.valori.systems` of its own to send its users to, so those stay
 * relative, same reasoning as `supabaseCookieOptions`.
 */
export async function appRedirectUrl(path: string): Promise<string> {
    if (!(path.startsWith('/dashboard') || path.startsWith('/admin'))) return path

    const hostname = (await headers()).get('host') ?? ''
    if (!hostname.endsWith(ROOT_DOMAIN)) return path

    return `https://${APP_HOST}${path}`
}

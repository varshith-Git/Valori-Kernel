import { NextResponse } from 'next/server'
// The client you created from the Server-Side Auth instructions
import { createClient } from '@/utils/supabase/server'
import { mfaChallengeRedirect } from '@/lib/server/mfa'
import { appRedirectUrl } from '@/lib/server/app-url'

export async function GET(request: Request) {
    const { searchParams, origin } = new URL(request.url)
    const code = searchParams.get('code')
    // if "next" is in param, use it as the redirect URL
    const next = searchParams.get('next') ?? '/dashboard'
    const desktop = searchParams.get('desktop') === '1'

    if (code) {
        const supabase = await createClient()
        const { data, error } = await supabase.auth.exchangeCodeForSession(code)
        if (!error) {
            const mfaRedirect = await mfaChallengeRedirect(supabase, next)

            // Desktop app handoff (see login/page.tsx's signInWithOAuth):
            // the desktop shell opened this whole flow in the system
            // browser, not its own embedded webview — hand the session back
            // via /desktop-handoff, which fires a valori://auth-callback
            // deep link the Tauri shell is registered to receive.
            if (desktop && !mfaRedirect && data.session) {
                const params = new URLSearchParams({
                    access_token: data.session.access_token,
                    refresh_token: data.session.refresh_token,
                })
                return NextResponse.redirect(`${origin}/desktop-handoff?${params.toString()}`)
            }

            // MFA still pending — carry `desktop` through to the challenge
            // page/action, which does this same handoff after verification
            // succeeds (see login/mfa-challenge/actions.ts).
            if (desktop && mfaRedirect) {
                return NextResponse.redirect(new URL(`${mfaRedirect}&desktop=1`, origin))
            }

            const target = mfaRedirect ?? next

            // appRedirectUrl only rewrites /dashboard and /admin targets, and
            // only when actually serving *.valori.systems — everything else
            // (e.g. /reset-password, an MFA challenge step, a preview
            // deployment) stays relative to wherever the callback landed.
            return NextResponse.redirect(new URL(await appRedirectUrl(target), origin))
        }

        return NextResponse.redirect(`${origin}/error?message=${encodeURIComponent(error.message)}`)
    }

    return NextResponse.redirect(`${origin}/error?message=${encodeURIComponent('Missing auth code in callback URL')}`)
}

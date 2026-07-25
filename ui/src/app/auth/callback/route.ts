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

    if (code) {
        const supabase = await createClient()
        const { error } = await supabase.auth.exchangeCodeForSession(code)
        if (!error) {
            const mfaRedirect = await mfaChallengeRedirect(supabase, next)
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

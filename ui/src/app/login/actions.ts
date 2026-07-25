'use server'

import { revalidatePath } from 'next/cache'
import { redirect } from 'next/navigation'
import { headers } from 'next/headers'
import { createClient } from '@/utils/supabase/server'
import { mfaChallengeRedirect } from '@/lib/server/mfa'
import { appRedirectUrl } from '@/lib/server/app-url'

export async function login(formData: FormData) {
    const supabase = await createClient()

    // type-casting here for convenience
    // in practice, you should validate your inputs
    const email = formData.get('email') as string
    const password = formData.get('password') as string

    const { error } = await supabase.auth.signInWithPassword({
        email,
        password,
    })

    // Layer 2.14: login history. Fire-and-forget via a SECURITY DEFINER
    // RPC (log_login_attempt), anon-callable since a failed attempt has no
    // session — logged regardless of outcome so a brute-force attempt is
    // actually visible, not just successful logins. Never blocks or fails
    // the real sign-in flow below.
    try {
        const headersList = await headers()
        const ip = headersList.get('x-forwarded-for')?.split(',')[0]?.trim() || headersList.get('x-real-ip') || null
        await supabase.rpc('log_login_attempt', {
            p_email: email,
            p_success: !error,
            p_ip: ip,
            p_user_agent: headersList.get('user-agent'),
        })
    } catch {
        // best-effort, see above
    }

    if (error) {
        redirect(`/error?message=${encodeURIComponent(error.message)}`)
    }

    revalidatePath('/', 'layout')
    const next = (formData.get('next') as string) || '/dashboard'
    const mfaRedirect = await mfaChallengeRedirect(supabase, next)
    redirect(await appRedirectUrl(mfaRedirect ?? next))
}

export async function signup(formData: FormData) {
    const supabase = await createClient()

    // Layer 2.2: signup reads system_settings (allow_signup/maintenance_mode)
    // through the Rust backend's public settings endpoint — no session
    // exists yet at signup time, so this has to be the unauthenticated
    // /v1/settings/public route, not the AdminAuth-gated one. Fails open
    // (allows signup) if the backend is unreachable — a settings-service
    // outage shouldn't also take down account creation. The redirect()
    // calls below must stay OUTSIDE this try/catch: Next.js implements
    // redirect() by throwing, and a catch block here would silently
    // swallow that and let signup proceed anyway.
    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'
    let blockReason: string | null = null
    try {
        const res = await fetch(`${apiUrl}/v1/settings/public`, { cache: 'no-store' })
        if (res.ok) {
            const settings = await res.json()
            if (settings.maintenance_mode) {
                blockReason = 'Signups are temporarily paused for maintenance. Please try again shortly.'
            } else if (!settings.allow_signup) {
                blockReason = 'New signups are currently closed.'
            }
        }
    } catch {
        // fail open, see comment above
    }
    if (blockReason) {
        redirect(`/error?message=${encodeURIComponent(blockReason)}`)
    }

    const email = formData.get('email') as string
    const password = formData.get('password') as string

    const { error } = await supabase.auth.signUp({
        email,
        password,
    })

    if (error) {
        redirect(`/error?message=${encodeURIComponent(error.message)}`)
    }

    revalidatePath('/', 'layout')
    const next = (formData.get('next') as string) || '/dashboard'
    redirect(await appRedirectUrl(next))
}

// Deliberately never reveals whether `email` belongs to an account —
// Supabase's own resetPasswordForEmail returns success either way, and we
// preserve that (redirect to the same "check your email" state regardless)
// to avoid leaking which addresses are registered.
export async function requestPasswordReset(formData: FormData) {
    const supabase = await createClient()
    const email = formData.get('email') as string
    const headersList = await headers()
    const host = headersList.get('host')
    const protocol = headersList.get('x-forwarded-proto') || 'http'
    const origin = `${protocol}://${host}`

    await supabase.auth.resetPasswordForEmail(email, {
        redirectTo: `${origin}/auth/callback?next=${encodeURIComponent('/reset-password')}`,
    })

    redirect('/forgot-password?sent=1')
}

export async function updatePassword(formData: FormData) {
    const supabase = await createClient()
    const password = formData.get('password') as string
    const confirm = formData.get('confirm') as string

    if (password !== confirm) {
        redirect('/reset-password?error=' + encodeURIComponent("Passwords don't match"))
    }
    if (password.length < 8) {
        redirect('/reset-password?error=' + encodeURIComponent('Password must be at least 8 characters'))
    }

    const { error } = await supabase.auth.updateUser({ password })

    if (error) {
        redirect(`/reset-password?error=${encodeURIComponent(error.message)}`)
    }

    redirect(await appRedirectUrl('/dashboard'))
}

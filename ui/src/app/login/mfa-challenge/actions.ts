'use server'

import { redirect } from 'next/navigation'
import { createClient } from '@/utils/supabase/server'
import { appRedirectUrl } from '@/lib/server/app-url'

export async function verifyMfaChallenge(formData: FormData) {
    const code = (formData.get('code') as string)?.trim()
    const next = (formData.get('next') as string) || '/dashboard'
    const desktop = formData.get('desktop') === '1'
    const supabase = await createClient()

    const { data: factors, error: factorsError } = await supabase.auth.mfa.listFactors()
    const totpFactor = factors?.totp.find((f) => f.status === 'verified')

    if (factorsError || !totpFactor) {
        redirect('/login/mfa-challenge?error=' + encodeURIComponent('No MFA factor found for this account') + `&next=${encodeURIComponent(next)}${desktop ? '&desktop=1' : ''}`)
        return
    }

    const { data: challenge, error: challengeError } = await supabase.auth.mfa.challenge({
        factorId: totpFactor.id,
    })

    if (challengeError || !challenge) {
        redirect('/login/mfa-challenge?error=' + encodeURIComponent(challengeError?.message ?? 'Challenge failed') + `&next=${encodeURIComponent(next)}${desktop ? '&desktop=1' : ''}`)
        return
    }

    const { error: verifyError } = await supabase.auth.mfa.verify({
        factorId: totpFactor.id,
        challengeId: challenge.id,
        code,
    })

    if (verifyError) {
        redirect('/login/mfa-challenge?error=' + encodeURIComponent(verifyError.message) + `&next=${encodeURIComponent(next)}${desktop ? '&desktop=1' : ''}`)
    }

    // Desktop handoff (see auth/callback/route.ts — this mirrors the same
    // logic for the MFA-pending path it defers to here). mfa.verify() above
    // already stepped this session up to aal2, so getSession() now returns
    // fresh tokens reflecting that.
    if (desktop) {
        const { data: { session } } = await supabase.auth.getSession()
        if (session) {
            const params = new URLSearchParams({
                access_token: session.access_token,
                refresh_token: session.refresh_token,
            })
            redirect(`/desktop-handoff?${params.toString()}`)
        }
    }

    redirect(await appRedirectUrl(next))
}

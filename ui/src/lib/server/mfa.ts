import { SupabaseClient } from '@supabase/supabase-js'

// After any successful first-factor sign-in (password or OAuth), check
// whether the account has MFA enrolled and the current session hasn't
// stepped up yet. Returns the path to redirect to instead of `next`, or
// null if the caller can proceed straight to `next`.
export async function mfaChallengeRedirect(
    supabase: SupabaseClient,
    next: string
): Promise<string | null> {
    const { data, error } = await supabase.auth.mfa.getAuthenticatorAssuranceLevel()
    if (error || !data) return null

    if (data.currentLevel === 'aal1' && data.nextLevel === 'aal2') {
        return `/login/mfa-challenge?next=${encodeURIComponent(next)}`
    }
    return null
}

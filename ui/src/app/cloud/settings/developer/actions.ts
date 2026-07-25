'use server'

import { revalidatePath } from 'next/cache'
import { createClient } from '@/utils/supabase/server'
import { AuditAction } from '@/lib/audit-actions'

export async function createPersonalAccessToken(name: string, scopes: string[]) {
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return { error: 'Not signed in.', token: null }
    }

    const { data, error } = await supabase
        .rpc('create_personal_access_token', {
            token_name: name,
            token_scopes: scopes,
        })
        .single()

    if (error || !data) {
        return { error: error?.message ?? 'Could not create token.', token: null }
    }

    const created = data as { id: string; plaintext_token: string; token_prefix: string; name: string }

    // Best-effort — the token already exists above regardless of this outcome.
    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.PersonalAccessTokenCreated,
        p_resource_type: 'personal_access_token',
        p_resource_id: created.id,
        p_metadata: { name, scopes },
    })
    if (auditError) console.error('audit log failed for personal_access_token.created:', auditError.message)

    revalidatePath('/cloud/settings/developer')
    // plaintext_token is returned exactly this once — the RPC never stores
    // it, only a hash. If the caller doesn't show it now, it's gone for good.
    return { error: null, token: created }
}

export async function rotatePersonalAccessToken(tokenId: string) {
    const supabase = await createClient()

    const { data, error } = await supabase.rpc('rotate_personal_access_token', { token_id: tokenId }).single()

    if (error || !data) {
        return { error: error?.message ?? 'Could not rotate token.', token: null }
    }

    revalidatePath('/cloud/settings/developer')
    // Same reveal-once contract as createPersonalAccessToken — the old
    // secret stops working the instant this call succeeds.
    return { error: null, token: data as { plaintext_token: string; token_prefix: string; name: string } }
}

export async function revokePersonalAccessToken(tokenId: string) {
    const supabase = await createClient()

    const { error } = await supabase
        .from('personal_access_tokens_public')
        .update({ revoked_at: new Date().toISOString() })
        .eq('id', tokenId)

    if (error) {
        return { error: error.message }
    }

    // Best-effort — the revoke already succeeded above.
    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.PersonalAccessTokenRevoked,
        p_resource_type: 'personal_access_token',
        p_resource_id: tokenId,
    })
    if (auditError) console.error('audit log failed for personal_access_token.revoked:', auditError.message)

    revalidatePath('/cloud/settings/developer')
    return { error: null }
}

'use server'

import { revalidatePath } from 'next/cache'
import { createClient } from '@/utils/supabase/server'
import { AuditAction } from '@/lib/audit-actions'

// P2.1: fixes the stale 3-arg create_api_key() call this copy had drifted
// to — the RPC now requires p_project_id (see
// valori-ui/supabase/migrations/20260810000000_project_scoped_api_keys.sql,
// and this repo's docs/phases/phase-project-api-key-P2.1.md). Port of the
// same fix already applied to valori-ui/ui's copy of this file.
export async function createApiKey(
    orgId: string,
    projectId: string,
    name: string,
    serviceAccountId?: string | null,
    expiresInDays?: number | null,
) {
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return { error: 'Not signed in.', key: null }
    }

    const expiresAt = expiresInDays
        ? new Date(Date.now() + expiresInDays * 24 * 60 * 60 * 1000).toISOString()
        : null

    const { data, error } = await supabase
        .rpc('create_api_key', {
            target_org_id: orgId,
            key_name: name,
            p_project_id: projectId,
            p_service_account_id: serviceAccountId || null,
            p_expires_at: expiresAt,
        })
        .single()

    if (error || !data) {
        return { error: error?.message ?? 'Could not create key.', key: null }
    }

    // Best-effort — the key already exists above regardless of this outcome.
    const created = data as {
        id: string
        plaintext_key: string
        key_prefix: string
        name: string
        project_id: string
        expires_at: string | null
    }
    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.ApiKeyCreated,
        p_resource_type: 'api_key',
        p_resource_id: created.id,
        p_organization_id: orgId,
        p_metadata: { name, project_id: projectId, expires_at: expiresAt },
    })
    if (auditError) console.error('audit log failed for api_key.created:', auditError.message)

    revalidatePath('/cloud/settings/api-keys')
    // plaintext_key is returned exactly this once — the RPC never stores it,
    // only a hash. If the caller doesn't show it now, it's gone for good.
    return { error: null, key: created }
}

export async function rotateApiKey(keyId: string) {
    const supabase = await createClient()

    const { data, error } = await supabase.rpc('rotate_api_key', { key_id: keyId }).single()

    if (error || !data) {
        return { error: error?.message ?? 'Could not rotate key.', key: null }
    }

    revalidatePath('/cloud/settings/api-keys')
    // Same reveal-once contract as createApiKey — the old secret stops
    // working the instant this call succeeds (rotate_api_key overwrites
    // key_hash in place), so this really is the only chance to see it.
    return { error: null, key: data as { plaintext_key: string; key_prefix: string; name: string } }
}

export async function revokeApiKey(keyId: string) {
    const supabase = await createClient()

    const { data: updated, error } = await supabase
        .from('api_keys_public')
        .update({ revoked_at: new Date().toISOString() })
        .eq('id', keyId)
        .select('org_id')
        .single()

    if (error) {
        return { error: error.message }
    }

    // Best-effort — the revoke already succeeded above.
    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.ApiKeyRevoked,
        p_resource_type: 'api_key',
        p_resource_id: keyId,
        p_organization_id: updated?.org_id,
    })
    if (auditError) console.error('audit log failed for api_key.revoked:', auditError.message)

    revalidatePath('/cloud/settings/api-keys')
    return { error: null }
}

// Layer 2.14: service accounts — a named grouping over api_keys, not a
// parallel credential system (see supabase/migrations/20260723060000's
// comment). RLS on service_accounts already enforces owner/admin only;
// these actions just surface a friendly error and log the audit trail.
export async function createServiceAccount(orgId: string, name: string, description: string) {
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()
    if (!user) {
        return { error: 'Not signed in.' }
    }

    const { data: created, error } = await supabase
        .from('service_accounts')
        .insert({ org_id: orgId, name, description: description || null, created_by: user.id })
        .select('id')
        .single()

    if (error) {
        return { error: error.message }
    }

    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.ServiceAccountCreated,
        p_resource_type: 'service_account',
        p_resource_id: created?.id,
        p_organization_id: orgId,
        p_metadata: { name },
    })
    if (auditError) console.error('audit log failed for service_account.created:', auditError.message)

    revalidatePath('/cloud/settings/api-keys')
    return { error: null }
}

export async function disableServiceAccount(accountId: string) {
    const supabase = await createClient()

    const { data: updated, error } = await supabase
        .from('service_accounts')
        .update({ disabled_at: new Date().toISOString() })
        .eq('id', accountId)
        .select('org_id')
        .single()

    if (error) {
        return { error: error.message }
    }

    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.ServiceAccountDisabled,
        p_resource_type: 'service_account',
        p_resource_id: accountId,
        p_organization_id: updated?.org_id,
    })
    if (auditError) console.error('audit log failed for service_account.disabled:', auditError.message)

    revalidatePath('/cloud/settings/api-keys')
    return { error: null }
}

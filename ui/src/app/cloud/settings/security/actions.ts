'use server'

import { revalidatePath } from 'next/cache'
import { createClient } from '@/utils/supabase/server'
import { AuditAction } from '@/lib/audit-actions'

export async function enrollTotp() {
    const supabase = await createClient()
    const { data, error } = await supabase.auth.mfa.enroll({ factorType: 'totp' })

    if (error || !data) {
        return { error: error?.message ?? 'Could not start enrollment' }
    }

    return {
        factorId: data.id,
        // qr_code comes back as raw SVG markup — Supabase's own docs say to
        // prepend this prefix to turn it into a renderable data URI.
        qrCode: `data:image/svg+xml;utf-8,${encodeURIComponent(data.totp.qr_code)}`,
        secret: data.totp.secret,
    }
}

export async function verifyEnrollment(factorId: string, code: string) {
    const supabase = await createClient()

    const { data: challenge, error: challengeError } = await supabase.auth.mfa.challenge({ factorId })
    if (challengeError || !challenge) {
        return { error: challengeError?.message ?? 'Challenge failed' }
    }

    const { error: verifyError } = await supabase.auth.mfa.verify({
        factorId,
        challengeId: challenge.id,
        code,
    })

    if (verifyError) {
        return { error: verifyError.message }
    }

    revalidatePath('/cloud/settings/security')
    return { ok: true }
}

export async function unenrollFactor(factorId: string) {
    const supabase = await createClient()
    const { error } = await supabase.auth.mfa.unenroll({ factorId })

    if (error) {
        return { error: error.message }
    }

    revalidatePath('/cloud/settings/security')
    return { ok: true }
}

// Layer 2.14: IP allowlist — org-scoped, owner/admin only (enforced by
// ip_allowlist_rules_insert/_delete RLS policies; this action just
// surfaces the RLS rejection as a friendly error). Enforced inside
// verify_api_key() (see supabase/migrations/20260723060000) — an org with
// zero rules is unrestricted.
export async function addIpAllowlistRule(orgId: string, cidr: string, description: string) {
    const supabase = await createClient()

    const { data: rule, error } = await supabase
        .from('ip_allowlist_rules')
        .insert({ org_id: orgId, cidr, description: description || null })
        .select('id')
        .single()

    if (error) {
        return { error: error.message }
    }

    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.IpAllowlistRuleAdded,
        p_resource_type: 'ip_allowlist_rule',
        p_resource_id: rule?.id,
        p_organization_id: orgId,
        p_metadata: { cidr },
    })
    if (auditError) console.error('audit log failed for ip_allowlist_rule.added:', auditError.message)

    revalidatePath('/cloud/settings/security')
    return { error: null }
}

export async function removeIpAllowlistRule(ruleId: string) {
    const supabase = await createClient()

    const { data: removed, error } = await supabase
        .from('ip_allowlist_rules')
        .delete()
        .eq('id', ruleId)
        .select('org_id')
        .single()

    if (error) {
        return { error: error.message }
    }

    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.IpAllowlistRuleRemoved,
        p_resource_type: 'ip_allowlist_rule',
        p_resource_id: ruleId,
        p_organization_id: removed?.org_id,
    })
    if (auditError) console.error('audit log failed for ip_allowlist_rule.removed:', auditError.message)

    revalidatePath('/cloud/settings/security')
    return { error: null }
}

// GoTrue's own "sign out other sessions" — client and server SDKs both
// support scope: 'others' using the caller's own access token, no
// service-role or per-session admin call needed.
export async function signOutOtherSessions() {
    const supabase = await createClient()
    const { error } = await supabase.auth.signOut({ scope: 'others' })

    if (error) {
        return { error: error.message }
    }

    revalidatePath('/cloud/settings/security')
    return { ok: true }
}

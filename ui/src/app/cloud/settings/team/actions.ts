'use server'

import { revalidatePath } from 'next/cache'
import { headers } from 'next/headers'
import { createClient } from '@/utils/supabase/server'
import { AuditAction } from '@/lib/audit-actions'

export async function inviteMember(orgId: string, orgName: string, email: string, role: string) {
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return { error: 'Not signed in.' }
    }

    const { data: invitation, error: insertError } = await supabase
        .from('org_invitations')
        .insert({ org_id: orgId, email, role, invited_by: session.user.id })
        .select('token')
        .single()

    if (insertError || !invitation) {
        return { error: insertError?.message ?? 'Could not create invitation.' }
    }

    const headersList = await headers()
    const host = headersList.get('host')
    const protocol = headersList.get('x-forwarded-proto') || 'http'
    const origin = `${protocol}://${host}`
    const inviteUrl = `${origin}/invite/${invitation.token}`

    // Notification email — best-effort. The invitation row already exists
    // regardless of whether this succeeds, so a delivery failure here is a
    // soft warning, not a reason to roll back the invite itself.
    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'
    try {
        await fetch(`${apiUrl}/v1/notify/org-invite`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                Authorization: `Bearer ${session.access_token}`,
            },
            body: JSON.stringify({ org_name: orgName, to: email, role, invite_url: inviteUrl }),
        })
    } catch {
        revalidatePath('/cloud/settings/team')
        return { error: null, warning: 'Invitation created, but the notification email may not have been sent.' }
    }

    revalidatePath('/cloud/settings/team')
    return { error: null, warning: null }
}

export async function revokeInvitation(invitationId: string) {
    const supabase = await createClient()
    const { error } = await supabase.from('org_invitations').delete().eq('id', invitationId)
    if (error) {
        return { error: error.message }
    }
    revalidatePath('/cloud/settings/team')
    return { error: null }
}

export async function changeMemberRole(orgId: string, userId: string, role: string) {
    const supabase = await createClient()
    const { error } = await supabase
        .from('org_members')
        .update({ role })
        .eq('org_id', orgId)
        .eq('user_id', userId)
    if (error) {
        return { error: error.message }
    }
    revalidatePath('/cloud/settings/team')
    return { error: null }
}

export async function transferOwnership(orgId: string, newOwnerUserId: string) {
    const supabase = await createClient()
    const { error } = await supabase.rpc('transfer_ownership', {
        target_org_id: orgId,
        new_owner_user_id: newOwnerUserId,
    })
    if (error) {
        return { error: error.message }
    }

    // Best-effort — the transfer already succeeded above.
    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.OwnershipTransferred,
        p_resource_type: 'organization',
        p_resource_id: orgId,
        p_organization_id: orgId,
        p_metadata: { new_owner_user_id: newOwnerUserId },
    })
    if (auditError) console.error('audit log failed for org.ownership_transferred:', auditError.message)

    revalidatePath('/cloud/settings/team')
    return { error: null }
}

export async function removeMember(orgId: string, userId: string) {
    const supabase = await createClient()
    const { error } = await supabase
        .from('org_members')
        .delete()
        .eq('org_id', orgId)
        .eq('user_id', userId)
    if (error) {
        return { error: error.message }
    }
    revalidatePath('/cloud/settings/team')
    return { error: null }
}

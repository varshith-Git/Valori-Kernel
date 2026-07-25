import { NextResponse } from 'next/server'
import { createClient } from '@/utils/supabase/server'
import { AuditAction } from '@/lib/audit-actions'

// Un-archives a project back to 'stopped' — brings it back into the main
// dashboard list, but doesn't restart compute automatically; the user hits
// Start separately, same as any other stopped project.
export async function POST(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return NextResponse.json({ error: 'not signed in' }, { status: 401 })
    }

    const { data: updated, error } = await supabase
        .from('projects')
        .update({ status: 'stopped' })
        .eq('id', id)
        .eq('status', 'archived')
        .select('org_id')
        .single()

    if (error) {
        return NextResponse.json({ error: error.message }, { status: 400 })
    }

    // Best-effort — see archive/route.ts for the same pattern.
    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.ProjectRestored,
        p_resource_type: 'project',
        p_resource_id: id,
        p_organization_id: updated.org_id,
    })
    if (auditError) console.error('audit log failed for project.restored:', auditError.message)

    return NextResponse.json({ ok: true })
}

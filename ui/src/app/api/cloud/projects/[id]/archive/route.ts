import { NextResponse } from 'next/server'
import { createClient } from '@/utils/supabase/server'
import { AuditAction } from '@/lib/audit-actions'

// Archive = stopped AND hidden from the main dashboard list, unlike a plain
// Stop (still shown, still "yours"). If the project is currently running,
// stop it first via the Rust backend so we don't archive a project while
// leaving its compute silently running.
export async function POST(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return NextResponse.json({ error: 'not signed in' }, { status: 401 })
    }

    const { data: project } = await supabase.from('projects').select('status, org_id').eq('id', id).single()

    if (!project) {
        return NextResponse.json({ error: 'not found' }, { status: 404 })
    }

    if (project.status === 'active') {
        const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'
        const stopRes = await fetch(`${apiUrl}/v1/projects/${id}/stop`, {
            method: 'POST',
            headers: { Authorization: `Bearer ${session.access_token}` },
        })
        if (!stopRes.ok) {
            return NextResponse.json(
                { error: `Could not stop project before archiving: ${await stopRes.text()}` },
                { status: 502 }
            )
        }
    }

    const { error } = await supabase.from('projects').update({ status: 'archived' }).eq('id', id)

    if (error) {
        return NextResponse.json({ error: error.message }, { status: 400 })
    }

    // Best-effort — a failed audit write must never block the archive
    // itself, which already succeeded above.
    const { error: auditError } = await supabase.rpc('log_audit_event', {
        p_action: AuditAction.ProjectArchived,
        p_resource_type: 'project',
        p_resource_id: id,
        p_organization_id: project.org_id,
    })
    if (auditError) console.error('audit log failed for project.archived:', auditError.message)

    return NextResponse.json({ ok: true })
}

import { NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

// 400/404 from the node = object store not configured on that project's
// node — normalized to a 200 with `disabled: true` so the UI can show a
// real "not configured" state instead of an error banner.
export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)
        const res = await fetch(`${nodeUrl}/v1/storage/snapshots`, { cache: 'no-store', signal: AbortSignal.timeout(10000) })
        if (res.status === 400 || res.status === 404) {
            return NextResponse.json({ snapshots: [], count: 0, disabled: true })
        }
        const data = await res.json().catch(() => ({ snapshots: [], count: 0 }))
        if (!Array.isArray(data.snapshots)) data.snapshots = []
        return NextResponse.json(data, { status: res.status })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) return NextResponse.json({ error: 'not found' }, { status: 404 })
        if (e instanceof ProjectNotReadyError) return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        return NextResponse.json({ snapshots: [], count: 0, error: 'node unreachable' }, { status: 503 })
    }
}

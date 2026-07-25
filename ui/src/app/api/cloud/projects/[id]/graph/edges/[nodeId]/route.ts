import { NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

export async function GET(
    _req: Request,
    { params }: { params: Promise<{ id: string; nodeId: string }> }
) {
    const { id, nodeId } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)
        const res = await fetch(`${nodeUrl}/graph/edges/${nodeId}`, {
            cache: 'no-store',
            signal: AbortSignal.timeout(10000),
        })
        const data = await res.json().catch(() => ({ edges: [] }))
        return NextResponse.json(data, { status: res.status })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) {
            return NextResponse.json({ error: 'not found' }, { status: 404 })
        }
        if (e instanceof ProjectNotReadyError) {
            return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        }
        return NextResponse.json({ edges: [] }, { status: 503 })
    }
}

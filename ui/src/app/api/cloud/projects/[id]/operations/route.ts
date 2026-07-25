import { NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)
        const res = await fetch(`${nodeUrl}/v1/operations`, { cache: 'no-store', signal: AbortSignal.timeout(10000) })
        const body = await res.json().catch(() => ({ operations: [], total: 0 }))
        return NextResponse.json(body, { status: res.status })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) return NextResponse.json({ error: 'not found' }, { status: 404 })
        if (e instanceof ProjectNotReadyError) return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        return NextResponse.json({ operations: [], total: 0, error: 'backend unreachable' }, { status: 503 })
    }
}

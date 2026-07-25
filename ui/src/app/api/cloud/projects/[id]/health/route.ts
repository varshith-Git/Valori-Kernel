import { NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)
        const res = await fetch(`${nodeUrl}/health`, {
            cache: 'no-store',
            signal: AbortSignal.timeout(5000),
        })
        const data = await res.json()
        return NextResponse.json(data, { status: res.status })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) {
            return NextResponse.json({ error: 'not found' }, { status: 404 })
        }
        if (e instanceof ProjectNotReadyError) {
            return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        }
        return NextResponse.json({ error: 'node unreachable' }, { status: 503 })
    }
}

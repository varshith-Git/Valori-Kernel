import { NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)
        const res = await fetch(`${nodeUrl}/v1/cluster/status`, { cache: 'no-store', signal: AbortSignal.timeout(10000) })
        // 404 = standalone mode (no cluster router mounted) — a replication=1
        // project's node never mounts one, so this is the expected case, not
        // an error.
        if (res.status === 404) {
            return NextResponse.json({ standalone: true })
        }
        const data = await res.json().catch(() => ({}))
        return NextResponse.json(data, { status: res.status })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) return NextResponse.json({ error: 'not found' }, { status: 404 })
        if (e instanceof ProjectNotReadyError) return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        return NextResponse.json({ error: 'node unreachable' }, { status: 503 })
    }
}

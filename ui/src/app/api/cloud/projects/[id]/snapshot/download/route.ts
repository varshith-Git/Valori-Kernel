import { NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

// Streams the raw .snap binary — proxyToNode always parses JSON, which
// would corrupt binary content, so this one talks to the node directly.
export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)
        const res = await fetch(`${nodeUrl}/v1/snapshot/download`, { signal: AbortSignal.timeout(60000) })
        if (!res.ok) {
            return NextResponse.json({ error: `snapshot download failed: HTTP ${res.status}` }, { status: res.status })
        }
        const bytes = await res.arrayBuffer()
        const now = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19)
        return new NextResponse(bytes, {
            status: 200,
            headers: {
                'Content-Type': 'application/octet-stream',
                'Content-Disposition': `attachment; filename="valori-${now}.snap"`,
            },
        })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) return NextResponse.json({ error: 'not found' }, { status: 404 })
        if (e instanceof ProjectNotReadyError) return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        return NextResponse.json({ error: 'node unreachable' }, { status: 503 })
    }
}

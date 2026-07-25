import { NextRequest, NextResponse } from 'next/server'
import { resolveProjectNodeUrl, ProjectNotFoundError, ProjectNotReadyError } from '@/lib/server/project'

export async function GET(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectNodeUrl(id)
        const searchParams = req.nextUrl.searchParams
        const url = new URL(`${nodeUrl}/graph/nodes`)
        for (const key of ['collection', 'kind', 'limit', 'offset']) {
            const v = searchParams.get(key)
            if (v !== null) url.searchParams.set(key, v)
        }
        const res = await fetch(url.toString(), { cache: 'no-store', signal: AbortSignal.timeout(10000) })
        const data = await res.json().catch(() => ({ nodes: [], count: 0 }))
        return NextResponse.json(data, { status: res.status })
    } catch (e) {
        if (e instanceof ProjectNotFoundError) {
            return NextResponse.json({ error: 'not found' }, { status: 404 })
        }
        if (e instanceof ProjectNotReadyError) {
            return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        }
        return NextResponse.json({ nodes: [], count: 0 }, { status: 503 })
    }
}

import { NextRequest, NextResponse } from 'next/server'
import { resolveProjectAccess, ProjectNotFoundError, ProjectNotReadyError, ApiRateLimitedError } from '@/lib/server/project'

// Serves both the dashboard (session cookie) and external API clients
// (Authorization: Bearer vlk_...) — search only needs 'read' scope.
export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    try {
        const nodeUrl = await resolveProjectAccess(req, id, 'read')
        const body = await req.json()
        const res = await fetch(`${nodeUrl}/search`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(body),
            signal: AbortSignal.timeout(30000),
        })
        const data = await res.json()
        return NextResponse.json(
            { ...data, queried_at: new Date().toISOString() },
            { status: res.status }
        )
    } catch (e) {
        if (e instanceof ProjectNotFoundError) {
            return NextResponse.json({ error: 'not found' }, { status: 404 })
        }
        if (e instanceof ProjectNotReadyError) {
            return NextResponse.json({ error: 'project not active yet' }, { status: 409 })
        }
        if (e instanceof ApiRateLimitedError) {
            return NextResponse.json({ error: "rate limit exceeded — see your plan's requests/minute limit" }, { status: 429 })
        }
        return NextResponse.json({ error: 'node unreachable' }, { status: 503 })
    }
}

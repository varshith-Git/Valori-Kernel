import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

// GET — list collections (namespaces) for this project's node.
export async function GET(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    return proxyToNode(id, '/v1/namespaces', {}, { req, scope: 'read', fallbackBody: { error: 'backend unreachable' } })
}

// POST — create a collection. Body: { name: string }
export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const body = await req.text()
    return proxyToNode(id, '/v1/namespaces', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
    }, { req, scope: 'write', fallbackBody: { error: 'backend unreachable' } })
}

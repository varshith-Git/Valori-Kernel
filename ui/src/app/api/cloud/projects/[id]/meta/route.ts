import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

export async function GET(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const targetId = req.nextUrl.searchParams.get('target_id')
    if (!targetId) return Response.json({ error: 'target_id required' }, { status: 400 })
    return proxyToNode(id, `/v1/memory/meta/get?target_id=${encodeURIComponent(targetId)}`)
}

export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const body = await req.text()
    return proxyToNode(id, '/v1/memory/meta/set', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
    })
}

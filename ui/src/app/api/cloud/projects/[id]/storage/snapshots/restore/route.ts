import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const body = await req.text()
    return proxyToNode(id, '/v1/storage/snapshots/restore', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
    })
}

import { proxyToNode } from '@/lib/server/nodeProxy'

export async function POST(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    return proxyToNode(id, '/v1/storage/snapshots/upload', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: '{}',
    })
}

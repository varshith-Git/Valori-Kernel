import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

export async function PATCH(
    req: NextRequest,
    { params }: { params: Promise<{ id: string; recordId: string }> }
) {
    const { id, recordId } = await params
    const collection = req.nextUrl.searchParams.get('collection') ?? ''
    const qs = collection ? `?collection=${encodeURIComponent(collection)}` : ''
    const body = await req.text()
    return proxyToNode(id, `/v1/records/${recordId}/metadata${qs}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body,
    })
}

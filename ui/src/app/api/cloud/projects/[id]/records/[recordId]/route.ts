import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

export async function GET(
    req: NextRequest,
    { params }: { params: Promise<{ id: string; recordId: string }> }
) {
    const { id, recordId } = await params
    const collection = req.nextUrl.searchParams.get('collection') ?? ''
    const qs = collection ? `?collection=${encodeURIComponent(collection)}` : ''
    return proxyToNode(id, `/v1/records/${recordId}${qs}`)
}

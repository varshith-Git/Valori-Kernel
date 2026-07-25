import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

/** POST /api/cloud/projects/[id]/community?action=detect|search */
export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const action = req.nextUrl.searchParams.get('action') ?? 'detect'
    const endpoint = action === 'search' ? '/v1/community/search' : '/v1/community/detect'
    const body = await req.text()
    return proxyToNode(id, endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
    })
}

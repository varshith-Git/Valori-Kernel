import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

// Accepts: { batch: number[][], collection?: string }
// Forwards to Valori POST /v1/vectors/batch_insert. Serves both the
// dashboard (session cookie) and external API clients (Authorization:
// Bearer vlk_...) — writes require 'write' scope on the key.
export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const body = await req.text()
    return proxyToNode(id, '/v1/vectors/batch_insert', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
    }, { req, scope: 'write' })
}

import { NextRequest } from 'next/server'
import { proxyToNode } from '@/lib/server/nodeProxy'

// The request body carries the LLM provider config (see useLLMConfig.ts) —
// this route just forwards text + that config to the node's own extraction
// endpoint, which calls the LLM itself. No project-independent relay here
// (unlike embed-query), since entity extraction writes graph nodes/edges
// back into this project's node.
export async function POST(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const body = await req.text()
    return proxyToNode(id, '/v1/ingest/extract-entities', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body,
    })
}

import { NextResponse } from 'next/server'
import { createClient } from '@/utils/supabase/server'

// Layer 3 (Runtime Management) — stop+start sequenced as one action, see
// backend's instance_lifecycle.rs. Same auth/proxy shape as stop/start.
export async function POST(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return NextResponse.json({ error: 'not signed in' }, { status: 401 })
    }

    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'
    const res = await fetch(`${apiUrl}/v1/projects/${id}/restart`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${session.access_token}` },
    })

    if (!res.ok) {
        return NextResponse.json({ error: await res.text() }, { status: res.status })
    }
    return new NextResponse(null, { status: 204 })
}

import { NextResponse } from 'next/server'
import { createClient } from '@/utils/supabase/server'

export async function GET(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return NextResponse.json({ error: 'not signed in' }, { status: 401 })
    }

    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'
    const res = await fetch(`${apiUrl}/v1/projects/${id}/status`, {
        headers: { Authorization: `Bearer ${session.access_token}` },
        cache: 'no-store',
    })
    const data = await res.json()
    return NextResponse.json(data, { status: res.status })
}

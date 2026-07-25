import { NextRequest, NextResponse } from 'next/server'
import { createClient } from '@/utils/supabase/server'

// Rename only — doesn't touch the running node at all (just Supabase
// metadata), so unlike DELETE this never talks to the Rust backend.
export async function PATCH(req: NextRequest, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const supabase = await createClient()

    const {
        data: { user },
    } = await supabase.auth.getUser()

    if (!user) {
        return NextResponse.json({ error: 'not signed in' }, { status: 401 })
    }

    const body = await req.json().catch(() => ({}))
    const name = typeof body.name === 'string' ? body.name.trim() : ''

    if (!name) {
        return NextResponse.json({ error: 'name is required' }, { status: 400 })
    }

    const { error } = await supabase.from('projects').update({ name }).eq('id', id)

    if (error) {
        return NextResponse.json({ error: error.message }, { status: 400 })
    }
    return NextResponse.json({ ok: true })
}

export async function DELETE(_req: Request, { params }: { params: Promise<{ id: string }> }) {
    const { id } = await params
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return NextResponse.json({ error: 'not signed in' }, { status: 401 })
    }

    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'
    const res = await fetch(`${apiUrl}/v1/projects/${id}`, {
        method: 'DELETE',
        headers: { Authorization: `Bearer ${session.access_token}` },
    })

    if (!res.ok) {
        return NextResponse.json({ error: await res.text() }, { status: res.status })
    }
    return new NextResponse(null, { status: 204 })
}

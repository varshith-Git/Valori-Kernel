'use server'

import { revalidatePath } from 'next/cache'
import { createClient } from '@/utils/supabase/server'

function slugify(name: string): string {
    return name
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/(^-|-$)/g, '')
}

async function provisionNewProject(
    orgId: string,
    name: string,
    region: string,
    replication: number,
    dim: number = 768,
    index: string = 'brute',
) {
    const supabase = await createClient()

    const {
        data: { session },
    } = await supabase.auth.getSession()

    if (!session) {
        return { error: 'Not signed in.' }
    }

    const { data: project, error: insertError } = await supabase
        .from('projects')
        .insert({
            org_id: orgId,
            name,
            slug: slugify(name),
            region,
            replication,
            dim,
            index_type: index,
            created_by: session.user.id,
        })
        .select()
        .single()

    if (insertError || !project) {
        return { error: insertError?.message ?? 'Could not create project.' }
    }

    const apiUrl = process.env.VALORI_CLOUD_API_URL ?? 'http://localhost:8787'

    let provisionRes: Response
    try {
        provisionRes = await fetch(`${apiUrl}/v1/projects/${project.id}/provision`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
                Authorization: `Bearer ${session.access_token}`,
            },
            body: JSON.stringify({ region, replication, dim, index }),
        })
    } catch {
        await supabase.from('projects').update({ status: 'error' }).eq('id', project.id)
        revalidatePath('/cloud')
        return { error: 'Provisioning service is unreachable. The project was created but not deployed — try again shortly.' }
    }

    if (!provisionRes.ok) {
        await supabase.from('projects').update({ status: 'error' }).eq('id', project.id)
        revalidatePath('/cloud')
        return { error: `Provisioning failed: ${await provisionRes.text()}` }
    }

    revalidatePath('/cloud')
    return { error: null }
}

export async function createProject(
    orgId: string,
    name: string,
    region: string,
    replication: number,
    dim?: number,
    index?: string,
) {
    return provisionNewProject(orgId, name, region, replication, dim, index)
}

// Duplicates project CONFIG only (name/region/replication/dim/index) — spins
// up a fresh, empty node via the normal provisioning path. It does not clone
// the source project's stored vectors/graph data; that would need a real
// snapshot-based clone, a bigger feature than the Layer-1 "Duplicate"
// checkbox implies. UI copy should say "duplicates settings" explicitly.
export async function duplicateProject(orgId: string, sourceProjectId: string) {
    const supabase = await createClient()

    const { data: source, error } = await supabase
        .from('projects')
        .select('name, region, replication, dim, index_type')
        .eq('id', sourceProjectId)
        .single()

    if (error || !source) {
        return { error: error?.message ?? 'Source project not found.' }
    }

    return provisionNewProject(orgId, `${source.name} (copy)`, source.region, source.replication, source.dim, source.index_type)
}

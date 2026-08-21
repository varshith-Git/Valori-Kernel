import { cache } from 'react'
import { createClient } from './server'

// Every /cloud layout and page independently called `supabase.auth.getUser()`
// — each call is a network round-trip to Supabase Auth to revalidate the
// JWT, so a single navigation (layout + page, sometimes + nested layout)
// was paying for it 2-3x. `cache()` memoizes this per-request: whichever
// Server Component calls it first pays the round-trip, every other caller
// in the same render tree gets the memoized result for free.
export const getAuthedUser = cache(async () => {
    const supabase = await createClient()
    const {
        data: { user },
    } = await supabase.auth.getUser()
    return user
})

export type CurrentMembership = {
    role: string
    organizations: { id: string; name: string; is_personal: boolean }
}

// The "first org" lookup (`org_members` joined to `organizations`) was
// duplicated — with three slightly different `select()` shapes — across
// /cloud/layout.tsx, /cloud/page.tsx, and four settings/archived pages.
// This is the superset shape (adds `role` + `is_personal` where a caller
// didn't previously select them, which costs nothing extra on a
// single-row PK-joined query) so every call site can share one cached
// query instead of each re-selecting the same rows with a slightly
// different column list. No org switcher exists yet, so "first org" is
// still correct everywhere this replaces a call.
export const getCurrentMembership = cache(async (): Promise<CurrentMembership | undefined> => {
    const user = await getAuthedUser()
    if (!user) return undefined

    const supabase = await createClient()
    const { data: memberships } = await supabase
        .from('org_members')
        .select('role, organizations(id, name, is_personal)')
        .eq('user_id', user.id)
        .limit(1)

    return memberships?.[0] as CurrentMembership | undefined
})

// The audit found ~9 pages under /cloud/projects/[id]/* independently
// running this exact query. Moved to /cloud/projects/[id]/layout.tsx
// (which Next.js does not re-render on sibling-page navigation, so
// metrics -> cluster -> tools no longer refetches at all) plus `cache()`
// so a hard load (layout + page rendering in the same request) still
// only hits Supabase once.
export const getCloudProject = cache(async (id: string) => {
    const supabase = await createClient()
    const { data: project } = await supabase.from('projects').select('*').eq('id', id).single()
    return project
})

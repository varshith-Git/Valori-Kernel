import { getAuthedUser, getCloudProject } from '@/utils/supabase/dal'
import { redirect, notFound } from 'next/navigation'

// Establishes the auth + project-exists boundary once for the whole
// /cloud/projects/[id]/* segment. Next.js does not re-render a layout on
// sibling-page navigation within the same dynamic segment, so navigating
// metrics -> cluster -> tools no longer re-runs this at all. Each child
// page still calls `getCloudProject(id)` itself (rather than receiving it
// as a prop — layouts can't inject extra props into `page.tsx`, only
// `children`/`params`) for its own render, but that call is the same
// `cache()`-memoized function with the same id, so on a hard load (layout
// + page rendering in one request) it's one Supabase query, not two.
export default async function ProjectLayout({
    children,
    params,
}: {
    children: React.ReactNode
    params: Promise<{ id: string }>
}) {
    const { id } = await params
    const user = await getAuthedUser()

    if (!user) {
        redirect('/login')
    }

    const project = await getCloudProject(id)

    if (!project) {
        notFound()
    }

    return <>{children}</>
}

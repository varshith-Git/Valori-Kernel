import { createClient } from '@/utils/supabase/server'
import { createClient as createPlainClient } from '@supabase/supabase-js'

export class ProjectNotFoundError extends Error {}
export class ProjectNotReadyError extends Error {}
export class ApiRateLimitedError extends Error {}

/**
 * Resolves a project's live node_url for the CURRENT signed-in user.
 * Uses the user's own Supabase session (not service_role) — RLS's
 * `projects_select` policy (is_org_member) does the ownership check for
 * free. An empty result is ambiguous by design: either the project doesn't
 * exist, or the caller isn't a member of the org that owns it — both cases
 * should read as "not found" to the caller, never leaking which.
 */
export async function resolveProjectNodeUrl(projectId: string): Promise<string> {
  const supabase = await createClient()

  const {
    data: { user },
  } = await supabase.auth.getUser()

  if (!user) {
    throw new ProjectNotFoundError(projectId)
  }

  const { data: project } = await supabase
    .from('projects')
    .select('node_url, status')
    .eq('id', projectId)
    .single()

  if (!project) {
    throw new ProjectNotFoundError(projectId)
  }
  if (!project.node_url || project.status !== 'active') {
    throw new ProjectNotReadyError(projectId)
  }

  // Every per-project API route resolves through here, so this is the one
  // real "this project is being used" signal — feeds the 30-day free-tier
  // inactivity suspension sweep (see supabase/migrations/20260722170000).
  // Best-effort: an org 'viewer' can read/search but the `projects_update`
  // RLS policy only allows owner/admin/developer to write, so this can fail
  // silently for viewer-only callers — swallowed either way, must never
  // block the real request this helper was called for. Awaited (not
  // fire-and-forget) since a serverless function can be frozen the moment
  // its response is sent, which would otherwise drop the update entirely.
  try {
    await supabase.from('projects').update({ last_active_at: new Date().toISOString() }).eq('id', projectId)
  } catch {
    // best-effort, see above
  }

  return project.node_url as string
}

/**
 * Resolves a project's live node_url for an EXTERNAL API caller presenting
 * `Authorization: Bearer vlk_...` — no Supabase session exists for these
 * requests, so ownership is decided entirely by the `verify_api_key()`
 * Postgres function (anon-callable, hashes the presented key and checks it
 * server-side; see supabase/migrations/20260722200000 and, for usage
 * counting + rate limiting, 20260723040000). That same call also bumps
 * the key's `last_used_at`/`request_count` — usage tracking piggybacks on
 * the auth check itself rather than needing a second write.
 *
 * `callerIp`: Layer 2.14's opt-in per-org IP allowlist, enforced inside
 * verify_api_key() itself — `x-forwarded-for`'s first entry is the
 * original client behind Vercel's proxy chain (see resolveProjectAccess).
 * `undefined` (no header present) is passed through as `null` to the RPC,
 * which fails CLOSED if the org has any allowlist rules configured.
 */
async function resolveProjectNodeUrlByApiKey(
  apiKey: string,
  projectId: string,
  requiredScope: 'read' | 'write',
  callerIp: string | null
): Promise<string> {
  const supabase = createPlainClient(process.env.NEXT_PUBLIC_SUPABASE_URL!, process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!)

  const { data, error } = await supabase
    .rpc('verify_api_key', {
        full_key: apiKey,
        target_project_id: projectId,
        required_scope: requiredScope,
        p_ip: callerIp,
    })
    .single()

  const result = data as { valid: boolean; node_url: string | null; status: string; rate_limited: boolean } | null

  if (error || !result?.valid) {
    if (result?.rate_limited) {
      throw new ApiRateLimitedError(projectId)
    }
    throw new ProjectNotFoundError(projectId)
  }
  if (!result.node_url || result.status !== 'active') {
    throw new ProjectNotReadyError(projectId)
  }
  return result.node_url
}

/**
 * Entry point for routes that should serve BOTH the browser dashboard
 * (Supabase session cookie) and external API clients (a `vlk_` bearer
 * key) — tries the API key first if the Authorization header looks like
 * one, otherwise falls back to the existing session-based resolution.
 * `requiredScope` only applies to the API-key path; a signed-in dashboard
 * user already has full access via RLS org membership, so there's no
 * equivalent "read vs write" distinction to enforce for sessions.
 */
export async function resolveProjectAccess(
  req: Request,
  projectId: string,
  requiredScope: 'read' | 'write' = 'read'
): Promise<string> {
  const authHeader = req.headers.get('authorization')
  const bearer = authHeader?.match(/^Bearer\s+(.+)$/i)?.[1]

  if (bearer?.startsWith('vlk_')) {
    const callerIp = req.headers.get('x-forwarded-for')?.split(',')[0]?.trim() || req.headers.get('x-real-ip') || null
    return resolveProjectNodeUrlByApiKey(bearer, projectId, requiredScope, callerIp)
  }
  return resolveProjectNodeUrl(projectId)
}

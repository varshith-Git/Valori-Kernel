/**
 * Opaque project identity. Shared Studio never inspects, parses, or assumes
 * a shape for this value — it is whatever the host already resolved a
 * project to before mounting a Studio feature:
 *   - Desktop Local: the project's registry/display name.
 *   - Desktop Cloud / Cloud Web: the Cloud project's id.
 * It is passed straight through to `Transport.path()` and used as an opaque
 * cache key by SWR — nothing in this package ever compares it to a UUID
 * pattern, a filesystem path, or anything else host-specific.
 */
export type ProjectRef = string;

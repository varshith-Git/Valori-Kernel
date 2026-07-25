// One central definition for actions that originate in ui/ (direct-Supabase
// writes this backend never observes over HTTP) — mirrors the Rust
// AuditAction enum in backend/apps/api/src/audit/mod.rs, which covers the
// separate set of actions the control plane performs itself. Neither list
// reaches across languages, so both exist; every log_audit_event() call
// site should import from here instead of writing a literal string.
//
// These values are stored on disk in audit_logs.action — treat them as
// append-only, same as the Rust side.
export const AuditAction = {
    ProjectArchived: 'project.archived',
    ProjectRestored: 'project.restored',
    OwnershipTransferred: 'org.ownership_transferred',
    ApiKeyCreated: 'api_key.created',
    ApiKeyRevoked: 'api_key.revoked',
    PersonalAccessTokenCreated: 'personal_access_token.created',
    PersonalAccessTokenRevoked: 'personal_access_token.revoked',
    ServiceAccountCreated: 'service_account.created',
    ServiceAccountDisabled: 'service_account.disabled',
    IpAllowlistRuleAdded: 'ip_allowlist_rule.added',
    IpAllowlistRuleRemoved: 'ip_allowlist_rule.removed',
} as const

export type AuditAction = (typeof AuditAction)[keyof typeof AuditAction]

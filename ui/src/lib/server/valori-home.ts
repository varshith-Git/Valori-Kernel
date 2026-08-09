import path from "path";
import os from "os";

/**
 * The ONE TypeScript-side resolver for `$VALORI_HOME` — every ui-server
 * module that needs the Studio/daemon home directory must import this
 * instead of computing its own copy. Before Studio S7, this rule was
 * independently duplicated in `api-client.ts`, `connection.ts`, and
 * `cluster-config.ts` (all three agreed, but there were three copies to
 * keep in sync); `projects.ts`/`project-adapter.ts` had their own
 * hardcoded, `VALORI_HOME`-blind fallback. See
 * `docs/phases/phase-studio-S7-persistence-boundary.md`.
 *
 * Mirrors `valori_daemon::default_home()` and
 * `valori_studio_storage::path::default_home_dir()` (Rust) exactly:
 * `$VALORI_HOME`, else `$HOME`/`os.homedir()` + `.valori`. This remains a
 * **deliberate duplicate**, not a shared dependency — the ui-server is a
 * separate Node.js process from the Tauri/Rust desktop app (see
 * `docs/architecture/control-plane.md`), so there is no cross-language
 * import to share instead. If this rule ever changes, all three
 * copies (this one + the two Rust ones) must change together — each
 * names the other two in its own doc comment.
 *
 * # This is a bootstrap-time / fallback value, not always authoritative
 *
 * Once the daemon is actually running, its own live-reported paths are
 * the source of truth — see `project-adapter.ts`'s `resolveProjectsDir()`,
 * which queries the daemon first and only falls back to
 * `getValoriHome()` when the daemon isn't reachable yet (e.g. still
 * starting up), never overriding a real answer with this default. Rust
 * (the Tauri desktop app / the daemon it spawns) remains the actual
 * authority for what `$VALORI_HOME` resolves to on desktop — this
 * function exists because the ui-server sometimes needs an answer
 * *before* it can ask, or when running standalone with no daemon at all
 * (self-hosted web deployment).
 */
export function getValoriHome(): string {
  return process.env.VALORI_HOME || path.join(os.homedir(), ".valori");
}

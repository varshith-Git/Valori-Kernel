use std::path::PathBuf;
use valori_studio_storage::StudioDatabase;

fn main() {
    let home = std::env::var("HOME").expect("HOME environment variable must be set");
    let db_path = PathBuf::from(&home).join(".valori").join("studio.redb");

    if !db_path.exists() {
        eprintln!("No studio.redb found at: {}", db_path.display());
        return;
    }

    println!("============================================================");
    println!("             VALORI STUDIO (redb) DATABASE DUMP             ");
    println!("============================================================");
    println!("Database path: {}\n", db_path.display());

    // If the desktop app is running, redb holds an exclusive file lock.
    // We copy the database file to ~/.valori/studio_dump.redb to inspect.
    let snapshot_path = PathBuf::from(&home)
        .join(".valori")
        .join("studio_dump.redb");
    let _ = std::fs::copy(&db_path, &snapshot_path);
    let db = StudioDatabase::open(&snapshot_path).expect("Failed to open studio.redb snapshot");
    let mode = "Live Snapshot of ~/.valori/studio.redb";

    println!("Open Mode: {}\n", mode);

    // 1. Schema Version
    println!("── [1] Database Metadata ───────────────────────────────────");
    println!(
        "  Schema Version : {}",
        valori_studio_storage::CURRENT_SCHEMA_VERSION
    );
    println!();

    // 2. Preferences Table
    println!("── [2] Preferences (preferences table) ─────────────────────");
    match db.preferences().get() {
        Ok(prefs) => {
            println!(
                "  Theme              : {:?}",
                prefs.theme.as_deref().unwrap_or("default")
            );
            println!("  Onboarding Version : {:?}", prefs.onboarding_version);
            println!(
                "  Workspace Directory: {:?}",
                prefs.workspace_dir.as_deref().unwrap_or("<none>")
            );
            println!(
                "  Model Directory    : {:?}",
                prefs.model_dir.as_deref().unwrap_or("<none>")
            );
            println!(
                "  Installation ID    : {:?}",
                prefs
                    .installation_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "<none>".into())
            );
            println!("  Telemetry Consent  : {:?}", prefs.telemetry_consent);
            println!("  Terms Accepted     : {:?}", prefs.terms_accepted);
        }
        Err(e) => println!("  <error reading preferences: {e}>"),
    }
    println!();

    // 3. Project Registry Table
    println!("── [3] Registered Projects (projects table) ────────────────");
    match db.projects().list() {
        Ok(projects) => {
            if projects.is_empty() {
                println!("  (No projects registered in redb yet)");
            } else {
                println!("  Total Projects: {}", projects.len());
                for (i, p) in projects.iter().enumerate() {
                    println!("  [{}] Display Name: {}", i + 1, p.display_name);
                    println!("      Project ID   : {}", p.id);
                    println!("      Kind / Path  : {:?}", p.kind);
                    println!("      Favorite     : {}", p.favorite);
                    println!("      Registered At: {}", p.registered_at);
                    if let Some(opened) = p.last_opened_at {
                        println!("      Last Opened  : {}", opened);
                    }
                }
            }
        }
        Err(e) => println!("  <error reading projects: {e}>"),
    }
    println!();

    // 4. Session History Table
    println!("── [4] Application Sessions (sessions table) ───────────────");
    match db.sessions().recent(10) {
        Ok(sessions) => {
            if sessions.is_empty() {
                println!("  (No session records found)");
            } else {
                println!("  Recent Sessions (showing up to 10):");
                for s in sessions {
                    let status = if s.crashed {
                        "CRASHED"
                    } else if s.ended_at.is_some() {
                        "CLEAN EXIT"
                    } else {
                        "ACTIVE / IN PROGRESS"
                    };
                    println!(
                        "  - Session [{}] Version: {} | Status: {} | Started: {}",
                        s.id, s.app_version, status, s.started_at
                    );
                }
            }
        }
        Err(e) => println!("  <error reading sessions: {e}>"),
    }
    println!();

    // 5. Telemetry Queue Table
    println!("── [5] Telemetry Queue (telemetry_queue table) ─────────────");
    match db.telemetry().count() {
        Ok(count) => println!("  Queued Events: {}", count),
        Err(e) => println!("  <error reading telemetry queue: {e}>"),
    }

    println!("\n============================================================");
    println!("                        END OF DUMP                         ");
    println!("============================================================");
}

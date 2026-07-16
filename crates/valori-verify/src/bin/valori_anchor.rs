// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! `valori-anchor` — create and verify Ed25519-signed chain-head anchors.
//!
//! ## Commands
//!
//! ```text
//! valori-anchor keygen [--out-dir <dir>]
//!     Generate a fresh Ed25519 keypair.
//!     signing.key → keep secret (signs anchors)
//!     verify.pub  → share with auditors (verifies anchors without the private key)
//!
//! valori-anchor create <log> --key <signing.key> [--note <text>]
//!     Replay the log, compute the chain head and state hash, sign them,
//!     and write <log>.anchor.
//!
//! valori-anchor verify <log> --anchor <log.anchor>
//!     Replay the log, check that the computed chain head and state hash
//!     match what the anchor asserts, and verify the Ed25519 signature.
//!     Exits 0 on success, 1 on any mismatch or invalid signature.
//! ```
//!
//! ## Trust model
//! The anchor file is self-describing: it contains the public key, so anyone
//! can verify signatures without a separate key distribution step.  The
//! *security* comes from distributing the public key out-of-band (give
//! `verify.pub` to your auditors) so they can confirm the anchor wasn't
//! re-signed with a different key after a rewrite.
//!
//! ## Future: external anchoring
//! For full protection against a private-key-holding operator, publish anchor
//! JSON to an immutable external channel (a transparency log, blockchain,
//! or the Valori hosted anchor registry at valori.ai/anchors).  The
//! `--publish <url>` flag is reserved for that integration.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};

use valori_wire::{format_utc, hex};

#[allow(dead_code)]
#[path = "../anchor.rs"]
mod anchor;
use anchor::{generate_keypair, load_signing_key, load_verifying_key, AnchorPayload};

use valori_verify::replay_log;

#[derive(Parser)]
#[command(
    name = "valori-anchor",
    version,
    about = "Create and verify Ed25519-signed chain-head anchors for Valori event logs"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a fresh Ed25519 signing keypair.
    Keygen {
        /// Directory to write signing.key and verify.pub into (default: current dir)
        #[arg(long, default_value = ".")]
        out_dir: PathBuf,
    },

    /// Replay the log, compute state + chain, sign and write <log>.anchor.
    Create {
        /// Event log to anchor
        log: PathBuf,
        /// Path to the Ed25519 signing key (generated with `keygen`)
        #[arg(long, value_name = "FILE")]
        key: PathBuf,
        /// Optional human note embedded in the anchor (auditor name, purpose, etc.)
        #[arg(long, value_name = "TEXT")]
        note: Option<String>,
    },

    /// Verify a log against an existing anchor file.
    Verify {
        /// Event log to verify
        log: PathBuf,
        /// Anchor file to check against
        #[arg(long, value_name = "FILE")]
        anchor: PathBuf,
        /// H-2: Path to the TRUSTED verify.pub to compare against the anchor's embedded
        /// public key. Required in production — without this flag the anchor is
        /// self-validating (an attacker can re-sign with their own key). Use --trust-embedded
        /// to explicitly opt in to the self-validating mode for convenience/dev.
        #[arg(long, value_name = "FILE", conflicts_with = "trust_embedded")]
        trusted_key: Option<PathBuf>,
        /// Allow the anchor's embedded public key to be used as its own trust root.
        /// Convenient for local development but provides no protection if the operator
        /// controls both the log and the anchor file. Prefer --trusted-key in production.
        #[arg(long, default_value_t = false)]
        trust_embedded: bool,
    },
}

// ── subcommand handlers ───────────────────────────────────────────────────────

fn cmd_keygen(out_dir: &Path) -> ExitCode {
    match generate_keypair(out_dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn cmd_create(log: &Path, key_path: &Path, note: Option<&str>) -> ExitCode {
    let signing_key = match load_signing_key(key_path) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };

    println!("replaying log…");
    let summary = match replay_log(log) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    // M-1: warn about trailing bytes — likely a partial write at the tail.
    if summary.trailing_bytes > 0 {
        eprintln!(
            "warning: {} trailing byte(s) could not be decoded (partial write?).",
            summary.trailing_bytes
        );
        eprintln!(
            "         anchor covers events #1..#{} only.",
            summary.event_count
        );
        eprintln!("         A subsequent verify after node repair may show a DIFFERENT hash.");
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let payload = AnchorPayload {
        chain_head: summary.chain_head,
        event_count: summary.event_count,
        state_hash: summary.state_hash,
        anchored_at_unix: now,
    };

    let anchor_json = payload.sign_to_json(&signing_key, note);
    let out_path = log.with_extension("anchor");

    match serde_json::to_string_pretty(&anchor_json) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&out_path, text) {
                eprintln!("error: cannot write anchor file: {e}");
                return ExitCode::from(1);
            }
        }
        Err(e) => {
            eprintln!("error: serialisation failed: {e}");
            return ExitCode::from(1);
        }
    }

    println!("✅  anchor written → {}", out_path.display());
    println!();
    println!("  events:     {}", summary.event_count);
    println!("  chain head: {}", hex(&summary.chain_head));
    println!("  state hash: {}", hex(&summary.state_hash));
    println!("  signed at:  {} (unix {})", format_utc(now), now);
    println!(
        "  public key: {}",
        hex(signing_key.verifying_key().as_bytes())
    );
    println!();
    println!("Share verify.pub with auditors so they can run:");
    println!(
        "  valori-anchor verify {} --anchor {}",
        log.display(),
        out_path.display()
    );

    ExitCode::SUCCESS
}

fn cmd_verify(
    log: &Path,
    anchor_path: &Path,
    trusted_key_path: Option<&Path>,
    trust_embedded: bool,
) -> ExitCode {
    // H-2: require either --trusted-key or explicit --trust-embedded.
    if trusted_key_path.is_none() && !trust_embedded {
        eprintln!("error: --trusted-key <verify.pub> is required.");
        eprintln!("       Without it the anchor is self-validating — an attacker who rewrites the");
        eprintln!("       log can re-sign with their own key and this command would still say OK.");
        eprintln!("       Pass --trusted-key <path> to your out-of-band verify.pub, or use");
        eprintln!(
            "       --trust-embedded if you understand the trust-model implication (dev only)."
        );
        return ExitCode::from(2);
    }

    // Load and verify the anchor signature.
    let anchor_text = match std::fs::read_to_string(anchor_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read anchor file: {e}");
            return ExitCode::from(1);
        }
    };
    let anchor_json: serde_json::Value = match serde_json::from_str(&anchor_text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: anchor file is not valid JSON: {e}");
            return ExitCode::from(1);
        }
    };

    let (anchor_payload, verifying_key) = match AnchorPayload::verify_json(&anchor_json) {
        Ok(pair) => pair,
        Err(e) => {
            println!();
            println!("❌  ANCHOR INVALID");
            println!("    {e}");
            return ExitCode::from(1);
        }
    };

    // H-2: if a trusted key was provided, compare it against the anchor's embedded key.
    if let Some(tk_path) = trusted_key_path {
        let trusted = match load_verifying_key(tk_path) {
            Ok(k) => k,
            Err(e) => {
                eprintln!("error: cannot load trusted key: {e}");
                return ExitCode::from(1);
            }
        };
        if trusted.as_bytes() != verifying_key.as_bytes() {
            println!();
            println!("❌  ANCHOR SIGNED BY UNKNOWN KEY");
            println!("    trusted key:  {}", hex(trusted.as_bytes()));
            println!("    anchor's key: {}", hex(verifying_key.as_bytes()));
            println!("    The anchor was not signed by the key you trust. Either the anchor was");
            println!(
                "    re-signed after a log rewrite, or the wrong trusted-key file was supplied."
            );
            return ExitCode::from(1);
        }
        println!("trusted key match: ✓ anchor signed by the expected key");
    } else {
        // --trust-embedded mode — warn the operator.
        eprintln!("warning: --trust-embedded: verifying against the anchor's own key (dev/convenience mode).");
        eprintln!("         This provides no protection if the operator controls both the log and anchor.");
    }

    println!("anchor signature: ✓ valid");
    println!("  signed by:  {}", hex(verifying_key.as_bytes()));
    println!(
        "  anchored:   {}",
        format_utc(anchor_payload.anchored_at_unix)
    );
    println!();

    // Replay the log and compare.
    println!("replaying log…");
    let summary = match replay_log(log) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };

    let mut ok = true;

    if summary.chain_head != anchor_payload.chain_head {
        println!("❌  CHAIN HEAD MISMATCH");
        println!("    anchor says:   {}", hex(&anchor_payload.chain_head));
        println!("    log computes:  {}", hex(&summary.chain_head));
        ok = false;
    } else {
        println!("  chain head: ✓ matches anchor");
    }

    if summary.state_hash != anchor_payload.state_hash {
        println!("❌  STATE HASH MISMATCH");
        println!("    anchor says:   {}", hex(&anchor_payload.state_hash));
        println!("    log computes:  {}", hex(&summary.state_hash));
        ok = false;
    } else {
        println!("  state hash: ✓ matches anchor");
    }

    if summary.event_count != anchor_payload.event_count {
        println!("❌  EVENT COUNT MISMATCH");
        println!("    anchor says:   {}", anchor_payload.event_count);
        println!("    log has:       {}", summary.event_count);
        ok = false;
    } else {
        println!("  events:     ✓ {} (matches anchor)", summary.event_count);
    }

    println!();
    if ok {
        println!("✅  ANCHOR VERIFIED");
        println!(
            "    The log is identical to what was anchored at {}.",
            format_utc(anchor_payload.anchored_at_unix)
        );
        println!("    Any alteration after that time would be detected here.");
        ExitCode::SUCCESS
    } else {
        println!("❌  LOG HAS CHANGED SINCE ANCHORING");
        println!(
            "    Run valori-verify {} for a detailed tamper report.",
            log.display()
        );
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Keygen { out_dir } => cmd_keygen(&out_dir),
        Cmd::Create { log, key, note } => cmd_create(&log, &key, note.as_deref()),
        Cmd::Verify {
            log,
            anchor,
            trusted_key,
            trust_embedded,
        } => cmd_verify(&log, &anchor, trusted_key.as_deref(), trust_embedded),
    }
}

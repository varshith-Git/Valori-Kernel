// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Emit the utoipa-generated OpenAPI document to stdout or an atomic output file.
//!
//!     cargo run -p valori-node --features utoipa --bin valori-openapi [-- --output api/openapi/valori-v1.yaml]
//!

use std::fs;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut output_path: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--output" || args[i] == "-o" {
            if i + 1 < args.len() {
                output_path = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            } else {
                eprintln!("Error: --output requires a file path argument");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let yaml = match valori_node::openapi::to_yaml() {
        Ok(y) => y,
        Err(e) => {
            eprintln!("failed to render OpenAPI: {e}");
            std::process::exit(1);
        }
    };

    if let Some(target) = output_path {
        let parent = target.parent().unwrap_or_else(|| std::path::Path::new("."));
        let temp_file = parent.join(format!(".valori-openapi-{}.tmp", std::process::id()));

        if let Err(e) = fs::write(&temp_file, &yaml) {
            eprintln!(
                "failed to write temp OpenAPI file {}: {e}",
                temp_file.display()
            );
            std::process::exit(1);
        }

        if let Err(e) = fs::rename(&temp_file, &target) {
            eprintln!(
                "failed to atomic rename {} -> {}: {e}",
                temp_file.display(),
                target.display()
            );
            let _ = fs::remove_file(&temp_file);
            std::process::exit(1);
        }
        println!(
            "Successfully wrote atomic OpenAPI contract to {}",
            target.display()
        );
    } else {
        println!("{yaml}");
    }
}

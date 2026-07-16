// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Resource monitor — one job: sample CPU / RAM / threads for a PID.
//!
//! Knows nothing about processes' lifecycle or restarting. Uses `ps` (present
//! on macOS and Linux) so there is no platform crate dependency. Threads come
//! from `/proc/<pid>/status` on Linux; `None` elsewhere.

use std::process::Command;

use serde::Serialize;

/// A point-in-time resource sample for one node.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceStats {
    /// Percent of a single CPU (as `ps` reports it).
    pub cpu_percent: f64,
    /// Resident memory in MB.
    pub memory_mb: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<u32>,
    pub uptime_secs: u64,
}

pub struct ResourceMonitor;

impl ResourceMonitor {
    /// Sample `pid`. Returns `None` if the process is gone or `ps` is unavailable.
    pub fn sample(pid: u32, uptime_secs: u64) -> Option<ResourceStats> {
        // `ps -o %cpu=,rss= -p PID` → e.g. "  1.3 123456" (cpu%, rss in KB).
        let out = Command::new("ps")
            .args(["-o", "%cpu=,rss=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut fields = text.split_whitespace();
        let cpu_percent: f64 = fields.next()?.parse().ok()?;
        let rss_kb: f64 = fields.next()?.parse().ok()?;

        Some(ResourceStats {
            cpu_percent,
            memory_mb: rss_kb / 1024.0,
            threads: read_threads(pid),
            uptime_secs,
        })
    }
}

/// Linux: `/proc/<pid>/status` → `Threads:` line. Other OSes: `None`.
fn read_threads(pid: u32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Threads:") {
            return rest.trim().parse().ok();
        }
    }
    None
}

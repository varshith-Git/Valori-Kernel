//! Thin wrapper that suppresses the macOS Dock icon before exec-ing into Node.js.
//!
//! Node.js/libuv calls CFRunLoopGetMain() → _RegisterApplication() on startup,
//! which connects the process to the window server and shows a Dock icon.
//!
//! The fix: call TransformProcessType(kProcessTransformToUIElementApplication)
//! BEFORE exec()-ing into node. TransformProcessType patches the LaunchServices
//! PSN record for this process. The PSN is PID-based and persists across exec().
//! When libuv's _RegisterApplication() fires in the exec'd node process,
//! LaunchServices already has a UIElement record for this PSN → no Dock icon.
//!
//! Why not CGSSetConnectionProperty("LSUIElement", true)?  That approach sets
//! a window-server connection property, but _RegisterApplication() re-initialises
//! the app type from LaunchServices (not CGS), overwriting the flag we set.
//! TransformProcessType works at the LaunchServices level and is not overwritten.
//!
//! Usage: node-launcher <real-node-binary> [node-args...]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("node-launcher: expected <node-binary> [args...]");
        std::process::exit(1);
    }

    #[cfg(target_os = "macos")]
    mark_background_agent();

    exec_into_node(&args[1], &args[2..]);
}

/// Mark this process as a UIElement (no Dock icon) via TransformProcessType.
///
/// TransformProcessType patches the LaunchServices PSN record for the current
/// process. The PSN is PID-based and survives exec(), so when libuv fires
/// _RegisterApplication() in the exec'd node image, LS already knows this
/// PSN is a UIElement and does not create a Dock entry.
#[cfg(target_os = "macos")]
fn mark_background_agent() {
    #[repr(C)]
    struct ProcessSerialNumber {
        high: u32,
        low: u32,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn GetCurrentProcess(psn: *mut ProcessSerialNumber) -> i32;
        fn TransformProcessType(psn: *const ProcessSerialNumber, transform_type: u32) -> i32;
    }

    // kProcessTransformToUIElementApplication = 4 (HIServices/Processes.h)
    const K_PROCESS_TRANSFORM_TO_UI_ELEMENT: u32 = 4;

    unsafe {
        let mut psn = ProcessSerialNumber { high: 0, low: 0 };
        let err = GetCurrentProcess(&mut psn);
        if err != 0 {
            eprintln!("node-launcher: GetCurrentProcess failed: {err}");
            return;
        }
        let err = TransformProcessType(&psn, K_PROCESS_TRANSFORM_TO_UI_ELEMENT);
        if err != 0 {
            eprintln!("node-launcher: TransformProcessType failed: {err}");
        }
    }
}

/// Replace the current process image with the real node binary.
fn exec_into_node(node_bin: &str, node_args: &[String]) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(node_bin)
            .args(node_args)
            .exec();
        eprintln!("node-launcher: exec failed: {err}");
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let status = std::process::Command::new(node_bin)
            .args(node_args)
            .status()
            .unwrap_or_else(|e| {
                eprintln!("node-launcher: failed to spawn node: {e}");
                std::process::exit(1);
            });
        std::process::exit(status.code().unwrap_or(1));
    }
}

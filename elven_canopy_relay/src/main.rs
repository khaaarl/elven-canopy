// CLI entry point for the Elven Canopy multiplayer relay.
//
// Starts a standalone (dedicated) relay server that game clients connect to.
// Clients create and join sessions dynamically via the `CreateSession` /
// `ListSessions` protocol messages. The relay orders commands into turns and
// broadcasts them — it never runs the sim.
//
// See `server.rs` for the networking architecture and `session.rs` for the
// session state.
//
// Usage:
//   relay [OPTIONS]
//     --bind <ADDR>           Bind address (default: 0.0.0.0)
//     --port <PORT>           Listen port (default: 7878)
//     --turn-cadence <MS>     Event loop turn cadence in ms (default: 50)

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use elven_canopy_relay::server::{RelayConfig, start_relay};

fn main() {
    let config = parse_args();

    let (handle, addr) = match start_relay(config) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Failed to start relay: {e}");
            std::process::exit(1);
        }
    };

    println!("Relay listening on {addr}");
    println!("Press Ctrl+C to stop.");

    // Wait for Ctrl+C.
    let running = Arc::new(AtomicBool::new(true));
    install_signal_handler(running.clone());

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    println!("\nShutting down...");
    handle.stop();
    println!("Relay stopped.");
}

/// Parse command-line arguments into a `RelayConfig`. Uses simple
/// `std::env::args()` matching — no clap dependency.
fn parse_args() -> RelayConfig {
    let mut config = RelayConfig::default();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--bind" => {
                i += 1;
                config.bind_address = args.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("--bind requires an address (e.g. 0.0.0.0 or 127.0.0.1)");
                    std::process::exit(1);
                });
            }
            "--port" => {
                i += 1;
                config.port = args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--port requires a valid port number");
                    std::process::exit(1);
                });
            }
            "--turn-cadence" => {
                i += 1;
                config.turn_cadence_ms =
                    args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                        eprintln!("--turn-cadence requires a valid number");
                        std::process::exit(1);
                    });
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    config
}

fn print_usage() {
    println!("Usage: relay [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --bind <ADDR>           Bind address (default: 0.0.0.0)");
    println!("  --port <PORT>           Listen port (default: 7878)");
    println!("  --turn-cadence <MS>     Event loop cadence in ms (default: 50)");
    println!("  --help, -h              Show this help");
}

/// Install a signal handler that sets `running` to false on SIGINT/SIGTERM.
/// This allows graceful shutdown: the main loop detects the flag change,
/// calls `handle.stop()`, and the relay threads wind down cleanly.
#[cfg(unix)]
fn install_signal_handler(running: Arc<AtomicBool>) {
    // Global flag set by the C signal handler (must be a static AtomicBool
    // since signal handlers can't capture state).
    static SIGNAL_RECEIVED: AtomicBool = AtomicBool::new(false);

    extern "C" fn handler(_sig: libc::c_int) {
        SIGNAL_RECEIVED.store(true, Ordering::SeqCst);
    }

    unsafe {
        libc::signal(libc::SIGINT, handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handler as *const () as libc::sighandler_t);
    }

    // Spawn a thread that polls the global flag and propagates to `running`.
    std::thread::spawn(move || {
        while !SIGNAL_RECEIVED.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        running.store(false, Ordering::SeqCst);
    });
}

#[cfg(not(unix))]
fn install_signal_handler(running: Arc<AtomicBool>) {
    // On non-Unix platforms, the default SIGINT behavior (process exit)
    // is acceptable — the OS cleans up TCP sockets on process termination.
    let _ = running;
}

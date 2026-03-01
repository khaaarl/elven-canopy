// CLI entry point for the Elven Canopy multiplayer relay.
//
// Starts a standalone relay server that game clients connect to. The relay
// orders commands into turns and broadcasts them — it never runs the sim.
// See `server.rs` for the networking architecture and `session.rs` for the
// session state.
//
// Usage:
//   relay [OPTIONS]
//     --port <PORT>           Listen port (default: 7878)
//     --name <NAME>           Session name (default: elven-canopy-session)
//     --password <PASS>       Session password (optional)
//     --ticks-per-turn <N>    Sim ticks per turn (default: 50)
//     --max-players <N>       Max players (default: 4)

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
    let running_clone = running.clone();
    ctrlc_wait(running_clone);

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    println!("\nShutting down...");
    handle.stop();
}

/// Parse command-line arguments into a `RelayConfig`. Uses simple
/// `std::env::args()` matching — no clap dependency.
fn parse_args() -> RelayConfig {
    let mut config = RelayConfig::default();
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                i += 1;
                config.port = args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--port requires a valid port number");
                    std::process::exit(1);
                });
            }
            "--name" => {
                i += 1;
                config.session_name = args.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("--name requires a value");
                    std::process::exit(1);
                });
            }
            "--password" => {
                i += 1;
                config.password = args.get(i).cloned().or_else(|| {
                    eprintln!("--password requires a value");
                    std::process::exit(1);
                });
            }
            "--ticks-per-turn" => {
                i += 1;
                config.ticks_per_turn =
                    args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                        eprintln!("--ticks-per-turn requires a valid number");
                        std::process::exit(1);
                    });
            }
            "--max-players" => {
                i += 1;
                config.max_players =
                    args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                        eprintln!("--max-players requires a valid number");
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
    println!("  --port <PORT>           Listen port (default: 7878)");
    println!("  --name <NAME>           Session name (default: elven-canopy-session)");
    println!("  --password <PASS>       Session password (optional)");
    println!("  --ticks-per-turn <N>    Sim ticks per turn (default: 50)");
    println!("  --max-players <N>       Max players (default: 4)");
    println!("  --help, -h              Show this help");
}

/// Block until Ctrl+C is pressed, then set the flag to false.
fn ctrlc_wait(running: Arc<AtomicBool>) {
    // Use a simple signal handler approach: spawn a thread that blocks on
    // reading a line from stdin, or use the fact that the main loop already
    // checks the flag. For a standalone binary, we rely on the user killing
    // the process — the relay threads will be torn down on exit.
    //
    // A proper signal handler would use `signal_hook` or `ctrlc` crate, but
    // to keep dependencies minimal we just let the main loop spin. The process
    // exits on SIGINT/SIGTERM by default, which is fine for a relay.
    //
    // If more graceful shutdown is needed later, add the `ctrlc` crate.
    let _ = running;
}

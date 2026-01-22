// ptree-driver: Windows service for real-time file system change tracking
// Provides incremental cache updates via NTFS USN Journal monitoring

use ptree_driver::{PtreeService, ServiceConfig, DRIVER_VERSION};
use std::env;

#[cfg(windows)]
use ptree_driver::registration;

fn main() {
    // Initialize logging
    env_logger::Builder::from_default_env()
        .format_timestamp_millis()
        .init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        match args[1].as_str() {
            "run" => run_service(),
            "register" => register_service(),
            "unregister" => unregister_service(),
            "start" => start_service(),
            "stop" => stop_service(),
            "status" => print_status(),
            "version" => print_version(),
            "help" => print_help(),
            _ => {
                eprintln!("Unknown command: {}", args[1]);
                print_help();
                std::process::exit(1);
            }
        }
    } else {
        // No args - show help
        print_help();
    }
}

/// Run the service in foreground
fn run_service() {
    println!("ptree-driver v{} - Starting", DRIVER_VERSION);
    
    // Create service with default config
    let config = ServiceConfig::default();
    let mut service = PtreeService::new(config);

    // Setup signal handlers (Ctrl+C)
    let should_exit = service.should_exit.clone();
    ctrlc::set_handler(move || {
        println!("\nShutting down...");
        should_exit.store(true, std::sync::atomic::Ordering::Relaxed);
    })
    .expect("Error setting Ctrl-C handler");

    // Run the service
    match service.run() {
        Ok(_) => {
            println!("Service stopped successfully");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Service error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Register service with Windows
#[cfg(windows)]
fn register_service() {
    println!("ptree-driver v{} - Registering as Windows service", DRIVER_VERSION);
    
    // Get current executable path
    match env::current_exe() {
        Ok(exe_path) => {
            match registration::register_service(&exe_path) {
                Ok(_) => {
                    println!("✓ Service registered successfully");
                    println!("  Service Name: {}", registration::SERVICE_NAME);
                    println!("  Display Name: {}", registration::SERVICE_DISPLAY_NAME);
                    println!("  Executable: {}", exe_path.display());
                    println!("\nThe service will start automatically on next boot.");
                    println!("To start it now, run: ptree-driver start");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("✗ Failed to register service: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("✗ Failed to get executable path: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn register_service() {
    eprintln!("Service registration is only supported on Windows");
    std::process::exit(1);
}

/// Unregister service from Windows
#[cfg(windows)]
fn unregister_service() {
    println!("ptree-driver v{} - Unregistering Windows service", DRIVER_VERSION);
    
    match registration::unregister_service() {
        Ok(_) => {
            println!("✓ Service unregistered successfully");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("✗ Failed to unregister service: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn unregister_service() {
    eprintln!("Service unregistration is only supported on Windows");
    std::process::exit(1);
}

/// Start the Windows service
#[cfg(windows)]
fn start_service() {
    println!("ptree-driver v{} - Starting service", DRIVER_VERSION);
    
    match registration::start_service() {
        Ok(_) => {
            println!("✓ Service started successfully");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("✗ Failed to start service: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn start_service() {
    eprintln!("Service start is only supported on Windows");
    std::process::exit(1);
}

/// Stop the Windows service
#[cfg(windows)]
fn stop_service() {
    println!("ptree-driver v{} - Stopping service", DRIVER_VERSION);
    
    match registration::stop_service() {
        Ok(_) => {
            println!("✓ Service stopped successfully");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("✗ Failed to stop service: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(not(windows))]
fn stop_service() {
    eprintln!("Service stop is only supported on Windows");
    std::process::exit(1);
}

/// Print service status
fn print_status() {
    println!("ptree-driver v{}", DRIVER_VERSION);
    println!("Status: Service status command");
    println!("Note: Full status monitoring requires Windows service integration");
}

/// Print version information
fn print_version() {
    println!("ptree-driver v{}", DRIVER_VERSION);
    println!("Windows NTFS USN Journal monitoring service");
}

/// Print help information
fn print_help() {
    println!("ptree-driver v{}", DRIVER_VERSION);
    println!("Windows NTFS USN Journal monitoring service for incremental cache updates\n");
    println!("USAGE:");
    println!("    ptree-driver run          - Run service in foreground (for testing)");
    println!("    ptree-driver register    - Register as Windows service (admin required)");
    println!("    ptree-driver unregister  - Unregister from Windows (admin required)");
    println!("    ptree-driver start       - Start the Windows service");
    println!("    ptree-driver stop        - Stop the Windows service");
    println!("    ptree-driver status      - Show service status");
    println!("    ptree-driver version     - Show version");
    println!("    ptree-driver help        - Show this help\n");
    println!("SETUP (one-time):");
    println!("    1. Run as Administrator");
    println!("    2. ptree-driver register");
    println!("    3. Service will start on next boot, or use: ptree-driver start\n");
    println!("ENVIRONMENT:");
    println!("    RUST_LOG - Set log level (debug, info, warn, error)");
    println!("    APPDATA  - Cache directory (default: %APPDATA%/ptree/cache)");
}

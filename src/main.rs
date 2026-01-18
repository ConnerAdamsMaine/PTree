mod cache;
mod cache_mmap;
mod traversal;
mod error;
mod cli;

#[cfg(windows)]
mod usn_journal;

use anyhow::Result;
use cli::{OutputFormat, ColorMode};
use std::time::Instant;

fn main() -> Result<()> {
    let program_start = Instant::now();

    // ========================================================================
    // Parse Command-Line Arguments
    // ========================================================================

    let args = cli::parse_args();

    // ========================================================================
    // Determine Color Output Settings
    // ========================================================================

    let use_colors = match args.color {
        ColorMode::Auto => atty::is(atty::Stream::Stdout),
        ColorMode::Always => true,
        ColorMode::Never => false,
    };

    // ========================================================================
    // Load or Create Cache
    // ========================================================================

    let cache_path = cache::get_cache_path()?;
    let cache_load_start = Instant::now();
    let mut cache = cache::DiskCache::open(&cache_path)?;
    let cache_load_elapsed = cache_load_start.elapsed();

    // ========================================================================
    // Traverse Disk & Update Cache
    // ========================================================================

    let debug_info = traversal::traverse_disk(&args.drive, &mut cache, &args)?;

    // ========================================================================
    // Output Results
    // ========================================================================

    cache.show_hidden = args.hidden;

    let formatting_start = Instant::now();
    let output = if !args.quiet {
        Some(match args.format {
            OutputFormat::Tree => {
                if use_colors {
                    cache.build_colored_tree_output_with_depth(args.max_depth)?
                } else {
                    cache.build_tree_output_with_depth(args.max_depth)?
                }
            }
            OutputFormat::Json => cache.build_json_output_with_depth(args.max_depth)?,
        })
    } else {
        None
    };
    let formatting_elapsed = formatting_start.elapsed();
    
    let output_start = Instant::now();
    if let Some(output) = output {
        println!("{}", output);
    }
    let output_elapsed = output_start.elapsed();

    // ========================================================================
    // Skip Statistics (if requested)
    // ========================================================================

    if args.skip_stats {
        eprintln!("{}", cache.get_skip_report());
    }

    // ========================================================================
    // Debug Output (Final Summary)
    // ========================================================================

    if args.debug {
        let total_elapsed = program_start.elapsed();
        print_debug_summary(&debug_info, cache_load_elapsed, formatting_elapsed, output_elapsed, &cache_path, total_elapsed);
    }

    Ok(())
}

/// Format duration in both milliseconds and microseconds
fn format_duration(duration: std::time::Duration) -> String {
    let ms = duration.as_secs_f64() * 1000.0;
    let us = duration.as_secs_f64() * 1_000_000.0;
    format!("{:.3} MS | {:.3} US", ms, us)
}

/// Print formatted debug summary
fn print_debug_summary(
    debug_info: &traversal::DebugInfo,
    cache_load_time: std::time::Duration,
    formatting_time: std::time::Duration,
    output_time: std::time::Duration,
    cache_path: &std::path::Path,
    total_time: std::time::Duration,
) {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("{:^70}", "PERFORMANCE DEBUG INFO");
    eprintln!("{}", "=".repeat(70));

    eprintln!("\n{:<40} {}", "Execution Mode:", if debug_info.is_first_run { "FULL DISK SCAN (First Run)" } else if debug_info.cache_used { "CACHED (< 1 hour)" } else { "PARTIAL SCAN (Current Dir)" });
    eprintln!("{:<40} {}", "Scan Root:", debug_info.scan_root.display());

    eprintln!("\n{:<40} {}", "Directories Scanned:", format_number(debug_info.total_dirs));
    eprintln!("{:<40} {}", "Threads Used:", debug_info.threads_used);

    eprintln!("\n{:<40} {}", "Cache Load Time:", format_duration(cache_load_time));
    if !debug_info.cache_used {
        eprintln!("{:<40} {}", "Traversal Time:", format_duration(debug_info.traversal_time));
        eprintln!("{:<40} {}", "Cache Index Time:", format_duration(debug_info.cache_index_time));
        eprintln!("{:<40} {}", "Cache Save Time:", format_duration(debug_info.save_time));
    }
    eprintln!("{:<40} {}", "Formatting Time:", format_duration(formatting_time));
    eprintln!("{:<40} {}", "Output Time:", format_duration(output_time));
    eprintln!("{:<40} {}", "Total Time:", format_duration(total_time));

    eprintln!("\n{:<40} {}", "Cache Location:", cache_path.display());
    eprintln!("{}", "=".repeat(70));
    eprintln!();
}

/// Format large numbers with thousands separator
fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    result
}

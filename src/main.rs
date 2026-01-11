mod cache;
mod traversal;
mod error;
mod cli;

#[cfg(windows)]
mod usn_journal;

use anyhow::Result;
use cli::{OutputFormat, ColorMode};

fn main() -> Result<()> {
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
    let mut cache = cache::DiskCache::open(&cache_path)?;

    // ========================================================================
    // Traverse Disk & Update Cache
    // ========================================================================

    traversal::traverse_disk(&args.drive, &mut cache, &args)?;

    // ========================================================================
    // Output Results
    // ========================================================================

    if !args.quiet {
        let output = match args.format {
            OutputFormat::Tree => {
                if use_colors {
                    cache.build_colored_tree_output()?
                } else {
                    cache.build_tree_output()?
                }
            }
            OutputFormat::Json => cache.build_json_output()?,
        };
        println!("{}", output);
    }

    Ok(())
}

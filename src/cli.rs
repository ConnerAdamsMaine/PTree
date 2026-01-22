use clap::Parser;
use std::collections::HashSet;

// ============================================================================
// Output Format Options
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Tree,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tree" | "ascii" => Ok(OutputFormat::Tree),
            "json" => Ok(OutputFormat::Json),
            other => Err(format!("Unknown format: {}", other)),
        }
    }
}

// ============================================================================
// Color Mode Options
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl std::str::FromStr for ColorMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(ColorMode::Auto),
            "always" => Ok(ColorMode::Always),
            "never" => Ok(ColorMode::Never),
            other => Err(format!("Unknown color mode: {}", other)),
        }
    }
}

/// ptree - A cache-first disk tree traversal tool for Windows
///
/// Scans disk directories with multi-threaded parallelism and caches results
/// for near-instant subsequent runs.
#[derive(Parser, Debug)]
#[command(name = "ptree")]
#[command(about = "Fast disk tree visualization with incremental caching")]
pub struct Args {
    // ========================================================================
    // Drive & Scanning Options
    // ========================================================================

    /// Drive letter (e.g., C, D)
    #[arg(short, long, default_value = "C")]
    pub drive: char,

    /// Enable admin mode to scan system directories
    #[arg(short, long)]
    pub admin: bool,

    /// Force full rescan (ignore cache)
    #[arg(short, long)]
    pub force: bool,

    // ========================================================================
    // Cache Options
    // ========================================================================

    /// Cache time-to-live in seconds (default: 3600)
    #[arg(long)]
    pub cache_ttl: Option<u64>,

    /// Override cache directory location
    #[arg(long)]
    pub cache_dir: Option<String>,

    /// Disable cache entirely (scan fresh every time)
    #[arg(long)]
    pub no_cache: bool,

    // ========================================================================
    // Output & Display Options
    // ========================================================================

    /// Suppress tree output (useful when just updating cache)
    #[arg(short, long)]
    pub quiet: bool,

    /// Output format: tree or json
    #[arg(long, default_value = "tree")]
    pub format: OutputFormat,

    /// Color output: auto, always, never
    #[arg(long, default_value = "auto")]
    pub color: ColorMode,

    /// Include directory sizes in output
    #[arg(long)]
    pub size: bool,

    /// Include file count per directory
    #[arg(long)]
    pub file_count: bool,

    /// Display summary statistics (total dirs, scan time, cache info)
    #[arg(long)]
    pub stats: bool,

    // ========================================================================
    // Filtering & Traversal Options
    // ========================================================================

    /// Maximum depth to display
    #[arg(short, long)]
    pub max_depth: Option<usize>,

    /// Directories to skip (comma-separated)
    #[arg(short, long)]
    pub skip: Option<String>,

    /// Show hidden files
    #[arg(long)]
    pub hidden: bool,

    // ========================================================================
    // Performance Options
    // ========================================================================

    /// Maximum threads (default: physical cores * 2)
    #[arg(short = 'j', long)]
    pub threads: Option<usize>,

    /// Enable incremental updates via USN Journal (Windows only)
    #[arg(long)]
    pub incremental: bool,

    // ========================================================================
    // Debugging & Diagnostics
    // ========================================================================

    /// Enable debug output with timing and performance metrics
    #[arg(long)]
    pub debug: bool,

    /// Show skip statistics (directories skipped during traversal)
     #[arg(long)]
     pub skip_stats: bool,
    
     // ========================================================================
     // Scheduler Options
     // ========================================================================
    
     /// Setup automatic cache refresh every 30 minutes (Windows Task Scheduler / cron)
     #[arg(long)]
     pub scheduler: bool,
    
     /// Remove scheduled cache updates
     #[arg(long)]
     pub scheduler_uninstall: bool,
    
     /// Show scheduler status
     #[arg(long)]
     pub scheduler_status: bool,
    }
    
    pub fn parse_args() -> Args {
     Args::parse()
    }

impl Args {
    /// Build skip directory set based on arguments
    pub fn skip_dirs(&self) -> HashSet<String> {
        let mut skip = Self::default_skip_dirs();

        // Add system directories unless in admin mode
        if !self.admin {
            skip.insert("System32".to_string());
            skip.insert("WinSxS".to_string());
            skip.insert("Temp".to_string());
            skip.insert("Temporary Internet Files".to_string());
        }

        // Add user-provided skip directories
        if let Some(skip_str) = &self.skip {
            for dir in skip_str.split(',') {
                skip.insert(dir.trim().to_string());
            }
        }

        skip
    }

    /// Default directories to always skip
    fn default_skip_dirs() -> HashSet<String> {
        vec![
            "System Volume Information".to_string(),
            "$Recycle.Bin".to_string(),
            ".git".to_string(),
        ]
        .into_iter()
        .collect()
    }
}

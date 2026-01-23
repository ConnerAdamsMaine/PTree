# PTree - High-Performance Disk Tree Visualization

A fast, cache-first disk tree traversal tool for Windows and Unix systems with multi-threaded parallelism and incremental caching.

## Features

- **Cache-first design**: Near-instant subsequent runs using persistent cache
- **Parallel traversal**: Multi-threaded DFS with configurable thread count
- **Incremental updates**: USN Journal support for change detection (Windows)
- **Scheduled refreshes**: Automatic cache updates via Windows Task Scheduler or cron
- **Flexible output**: Tree view or JSON output with configurable depth limiting
- **Memory-bounded**: Strict O(n) memory usage guarantees (200 bytes per directory)
- **Cross-platform**: Windows and Unix/Linux support

## Architecture

PTree uses a modular crate-based architecture:

```
ptree (binary)
├── ptree-core       (CLI, types, error handling)
├── ptree-cache      (disk cache, serialization)
├── ptree-traversal  (parallel DFS traversal)
├── ptree-scheduler  (scheduled refresh)
└── ptree-incremental (USN Journal updates)
```

### Key Components

- **ptree-core**: Command-line argument parsing and core types
- **ptree-cache**: In-memory cache with rkyv-based persistence
- **ptree-traversal**: Multi-threaded iterative DFS with batching and lock-free optimization
- **ptree-scheduler**: Task scheduling for automatic cache refresh (30-minute intervals)
- **ptree-incremental**: USN Journal tracking for incremental updates (Windows only)

## Building

### Requirements

- Rust 1.70+
- Windows 10+ or Linux/macOS

### Build

```bash
cargo build --release
```

The release binary will be at `target/release/ptree`.

## Usage

```bash
# Basic usage - show current directory tree
ptree

# Show full drive C:
ptree --force

# JSON output with depth limit
ptree --format json --max-depth 3

# Show hidden files
ptree --hidden

# Display statistics
ptree --stats

# Setup automatic cache refresh (every 30 minutes)
ptree --scheduler

# Custom cache location
ptree --cache-dir "C:\Custom\Path"
```

### Command-Line Options

```
USAGE:
    ptree [OPTIONS]

OPTIONS:
    -d, --drive <DRIVE>              Drive letter (default: C)
    -f, --force                      Force full rescan (ignore cache)
    -a, --admin                      Admin mode (scan system directories)
    --cache-ttl <SECONDS>            Cache time-to-live (default: 3600)
    --cache-dir <DIR>                Custom cache directory
    --no-cache                       Disable cache entirely
    -q, --quiet                      Suppress output
    --format <FORMAT>                Output format: tree or json (default: tree)
    --color <MODE>                   Color output: auto, always, never (default: auto)
    -m, --max-depth <DEPTH>          Maximum display depth
    -j, --threads <COUNT>            Thread count (default: CPU cores * 2)
    --stats                          Show timing statistics
    --skip-stats                     Show skipped directory statistics
    --incremental                    Enable incremental updates (USN Journal)
    --scheduler                      Install scheduled cache refresh
    --scheduler-uninstall            Remove scheduled refresh
    --scheduler-status               Check scheduler status
```

## Cache Behavior

The cache operates on a time-to-live model:

- **First run**: Full disk scan stored in cache
- **Subsequent runs**: Cache returned if age < TTL (default 1 hour)
- **Cache location**: `%APPDATA%\ptree\cache\ptree.dat` (Windows)
- **Cache format**: Rkyv binary with lazy-loading index for O(1) cold start
- **Force rescan**: Use `--force` flag to bypass cache

## Performance

### Benchmarks

Performance varies by system configuration. Expected results on typical systems:

| Operation | Cold Start | Warm Cache | Notes |
|-----------|-----------|-----------|-------|
| First scan (1M dirs) | TBD | - | Full traversal |
| Cached read | - | TBD | ~1ms cold-start |
| Formatting output | TBD | - | Parallel sort |
| Scheduler overhead | - | TBD | 30-min refresh |

*Benchmarks to be filled in after performance testing.*

## Development

### Project Structure

```
PerfTree/
├── src/                 # Binary entrypoint
├── crates/
│   ├── ptree-core/
│   ├── ptree-cache/
│   ├── ptree-traversal/
│   ├── ptree-scheduler/
│   ├── ptree-incremental/
│   ├── ptree-NTFS/      # (placeholder)
│   ├── ptree-USN/       # (placeholder)
│   └── ptree-MFT/       # (placeholder)
├── benches/             # Benchmarks
└── docs/                # Documentation
```

### Running Tests

```bash
cargo test
```

### Running Benchmarks

```bash
cargo bench --bench traversal_benchmarks
```

## Features (Compile-time)

```bash
# Default (with scheduler and incremental)
cargo build --release

# Minimal (cache + traversal only)
cargo build --release --no-default-features

# Custom feature selection
cargo build --release --features scheduler
```

## Platform-Specific Notes

### Windows
- Full USN Journal support for incremental updates
- Windows Task Scheduler integration for scheduled refresh
- System directory skipping (without `--admin` flag)

### Unix/Linux
- Basic traversal and caching
- Cron-based scheduling (future)
- No incremental update support

## Future Work

- [ ] Fill in performance benchmarks
- [ ] Implement NTFS/USN/MFT crates
- [ ] Add Unix cron scheduling
- [ ] Web UI for visualization
- [ ] Database export support

## License

[Add your license here]

## Contributing

[Add contribution guidelines here]

# ptree - Fast Windows Disk Tree Visualization

A high-performance Rust-based Windows disk tree traversal tool with cache-first architecture. Scans large disks (2TB+) and caches results for near-instant subsequent runs.

## Features

- **Cache-first design**: First scan takes minutes, subsequent runs take < 1 second (if cache fresh)
- **Parallel traversal**: Multi-threaded DFS with 2×cores threads by default
- **Memory-bounded**: O(200 bytes) per directory, scales to millions of directories
- **Atomic writes**: Safe cache updates with no corruption risk
- **Skip filtering**: Default system directories or custom skip lists
- **Admin mode**: Optional system directory scanning when run as Administrator
- **Thread control**: Fine-tune thread count for different hardware

## Quick Start

### Installation

1. Download `ptree.exe` from releases (or build from source)
2. Add to PATH or run directly

### Basic Usage

```bash
# Scan C: drive
ptree.exe

# Scan D: drive
ptree.exe --drive D

# Force rescan (ignore cache)
ptree.exe --force

# Suppress output (just update cache)
ptree.exe --quiet
```

## Command Reference

```bash
ptree.exe [OPTIONS]

OPTIONS:
  -d, --drive <DRIVE>          Drive letter (default: C)
  -a, --admin                  Include system directories (requires elevation)
  -q, --quiet                  No output (cache update only)
  -f, --force                  Ignore cache, full rescan
  -m, --max-depth <DEPTH>      Limit tree depth (future)
  -s, --skip <DIRS>            Skip directories (comma-separated)
      --hidden                 Show hidden files
      --threads <NUM>          Override thread count
  -h, --help                   Show help
```

## Examples

### View disk structure
```bash
ptree.exe
```

### Force refresh cache
```bash
ptree.exe --force
```

### Skip development directories
```bash
ptree.exe --skip "node_modules,target,.git,.venv"
```

### Silent cache update (scheduled)
```bash
ptree.exe --force --quiet
```

### Include system directories (admin)
```bash
ptree.exe --admin
```

## Performance

### Typical Scan Times

| Disk Size | Directories | First Run | Cached Run |
|-----------|-------------|-----------|-----------|
| 100GB | 100K | 1-2 min | <100ms |
| 500GB | 500K | 3-5 min | <100ms |
| 1TB | 1M | 5-10 min | <100ms |
| 2TB | 2M | 10-20 min | <100ms |

**Note**: Times vary by disk speed (HDD slower, NVMe faster). Cache is reused if < 1 hour old.

## Architecture

### Module Structure

```
src/
  main.rs        - Application orchestration
  cli.rs         - Command-line argument parsing
  cache.rs       - Binary serialization & HashMap storage
  traversal.rs   - Parallel DFS with thread pool
  error.rs       - Error types
```

### Key Design Decisions

1. **HashMap for entries**: O(1) lookup by path
2. **Batched writes**: 10K-entry flush threshold reduces memory contention
3. **Atomic saves**: Temp file + rename prevents corruption
4. **1-hour cache TTL**: Balances freshness vs. performance
5. **2×cores threads**: Optimal for I/O-bound operations

## Documentation

- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Complete design document
- **[IMPLEMENTATION_GUIDE.md](IMPLEMENTATION_GUIDE.md)** - Code walkthrough & internals
- **[USAGE_EXAMPLES.md](USAGE_EXAMPLES.md)** - Real-world examples & best practices

## Building from Source

### Requirements
- Rust 1.70+ (2021 edition)
- Windows 10+ with MSVC toolchain

### Build
```bash
cargo build --release
```

Binary: `target/release/ptree.exe` (855KB)

## Cache Location

```
%APPDATA%\ptree\cache\ptree.dat
```

Typical path:
```
C:\Users\<YourName>\AppData\Roaming\ptree\cache\ptree.dat
```

## Scheduled Updates (Windows Task Scheduler)

Create batch file `update_ptree.bat`:
```batch
@echo off
ptree.exe --drive C --force --quiet
ptree.exe --drive D --force --quiet
```

Schedule daily execution:
1. Open Task Scheduler
2. Create basic task
3. Trigger: Daily at 2 AM
4. Action: Run script
5. Run with highest privileges

## Performance Tips

### Skip Heavy Directories
```bash
ptree.exe --skip "Windows,Program Files,node_modules"
```
Reduces scan time by 50%+ on typical systems.

### Tune Thread Count
```bash
ptree.exe --threads 16    # High-core systems
ptree.exe --threads 2     # Slow I/O (USB, network)
```

### Pre-cache on Deployment
```bash
ptree.exe --force --quiet
```
On new machines, cache immediately for instant subsequent access.

## Comparison with Windows `tree` Command

| Feature | ptree | tree |
|---------|-------|------|
| Caching | ✓ | ✗ |
| Parallel | ✓ | ✗ |
| Speed (repeat) | <1 sec | 10+ min |
| Admin filtering | ✓ | ✗ |
| Custom skip | ✓ | Limited |
| Binary format | ✓ | ✗ |
| Atomic writes | ✓ | ✗ |

## Known Limitations

- **Symlinks**: Detected and skipped (prevents cycles)
- **Permissions**: Unreadable directories silently skipped
- **Max depth**: Not yet implemented (future)
- **Size calculation**: Not included (future)
- **Export formats**: ASCII tree only (JSON/CSV future)

## Future Enhancements

- **Incremental updates**: NTFS USN Journal tracking
- **Directory sizes**: Calculate per-directory totals
- **Max depth**: Limit output depth
- **Export formats**: JSON, CSV, XML output
- **Diff mode**: Show what changed since last scan
- **Colored output**: Visual distinction for directory types

## Troubleshooting

### "Cache not updating"
- Check file: `%APPDATA%\ptree\cache\ptree.dat`
- Use `--force` to rescan: `ptree.exe --force`
- Verify file is not read-only

### "Permission denied" (silent)
- Some directories require admin access
- Use `--admin` flag: `ptree.exe --admin`
- Run as Administrator for full scan

### "Slow first scan"
- Expected: Full disk scan required
- First run unavoidably slow on large disks
- Subsequent runs instant (use cache)
- Consider scheduling pre-cache: `ptree.exe --force --quiet`

### "High memory usage"
- Expected for large disks: 200 bytes × 10M dirs = 2GB
- Normal and acceptable
- Memory usage proportional to directory count

## Contributing

Contributions welcome. Areas for help:

- [ ] Incremental cache updates
- [ ] Export formats (JSON, CSV)
- [ ] Windows-specific optimizations
- [ ] Performance benchmarks
- [ ] Integration tests

## License

MIT

## Acknowledgments

Built with:
- [Rust](https://www.rust-lang.org/) - Safe systems programming
- [clap](https://github.com/clap-rs/clap) - CLI argument parsing
- [serde](https://github.com/serde-rs/serde) + [bincode](https://github.com/bincode-org/bincode) - Fast serialization
- [rayon](https://github.com/rayon-rs/rayon) - Data parallelism
- [parking_lot](https://github.com/Amanieu/parking_lot) - Faster locks

## Support

For issues, feature requests, or questions:
1. Check [USAGE_EXAMPLES.md](USAGE_EXAMPLES.md) for common scenarios
2. Review [ARCHITECTURE.md](ARCHITECTURE.md) for technical details
3. Run `ptree.exe --help` for command reference

---

**Made with Rust** 🦀 for Windows systems.

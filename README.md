# ptree - Fast Windows Disk Tree Visualization

A high-performance Rust-based Windows disk tree traversal tool with incremental cache architecture. Scans large disks (2TB+) and maintains cache correctness via NTFS USN Journal tracking.

## Features

- **Incremental cache**: First scan captures filesystem snapshot. USN Journal refresh every 1 hour synchronizes cache to current state.
- **Parallel traversal**: Multi-threaded DFS with 2×CPU cores threads by default (iterative, stack-bounded)
- **Memory-bounded**: Hard-capped at 200 bytes per directory entry. Scales linearly to millions of directories.
- **Atomic cache writes**: Temp file + atomic rename prevents corruption on write failure
- **Skip filtering**: Default system directories or custom skip lists
- **Admin mode**: Full traversal of protected system directories when run as Administrator
- **Direct hardware enumeration**: Nanosecond-scale directory listing via Win32 hardware bindings

## Design Invariants

### Memory Model (Hard-Bounded)

- Each directory entry is capped at 200 bytes (directory name only)
- Memory usage is strictly: `memory ≤ directory_count × 200 bytes`
- Example: 2M directories = 400MB maximum memory footprint
- No unbounded string growth; paths are traversed, not accumulated

### Cache Correctness Model

- **First scan**: Produces accurate snapshot of filesystem at scan time
- **After scan, before USN refresh**: Cache may lag live filesystem changes (eventual consistency)
- **After 1-hour interval**: USN Journal refresh synchronizes cache to current filesystem state via incremental updates
- **Cache invariant**: Always correct given sufficient time (1-hour refresh window)

### Symlink Handling

- All symlinks are detected and skipped (not traversed)
- Symlinks appear in output with notation: `symlink_name (→ target)` for clarity
- Symlinks are counted in total directory count but never recursively followed
- Cycle prevention: Guaranteed by symlink skipping (no inode tracking needed)

### Path Resolution

- Parent-child relationships use separator (`\`) boundaries, preventing prefix collisions (e.g., `Dir1` vs `Dir10`)
- Paths are traversed via hardware enumeration, not string manipulation
- Iterative DFS prevents recursive stack growth

### USN Journal Management

- USN Journal max size: 500MB (hardcoded)
- On USN Journal wrap-around: Cache size is increased up to the 500MB limit
- If 500MB capacity is reached and wrap occurs: Automatic fallback to full rescan on next refresh cycle
- USN entries are cached; refresh interval is 1 hour from last cache write

## Quick Start

### Installation

1. Download `ptree.exe` from releases (or build from source)
1. Add to PATH or run directly

### Basic Usage

```bash
# Scan C: drive
ptree.exe

# Scan D: drive
ptree.exe --drive D

# Force rescan (ignore cache, update USN state)
ptree.exe --force

# Suppress output (cache update only)
ptree.exe --quiet
```

## Command Reference

```bash
ptree.exe [OPTIONS]

OPTIONS:
  -d, --drive <DRIVE>          Drive letter (default: C)
  -a, --admin                  Include all protected system directories (requires elevation)
  -q, --quiet                  No output (cache update only)
  -f, --force                  Ignore cache, full rescan
  -s, --skip <DIRS>            Skip directories (comma-separated)
      --threads <NUM>          Override thread count (default: 2×CPU cores)
  -h, --help                   Show help

PLANNED (not yet implemented):
  -m, --max-depth <DEPTH>      Limit tree depth
      --hidden                 Include hidden file attributes
```

## Examples

### View disk structure

```bash
ptree.exe
```

### Force refresh cache and update USN Journal state

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

### Include all system directories (admin)

```bash
ptree.exe --admin
```

### Tune threads for slow I/O (e.g., USB, HDD)

```bash
ptree.exe --threads 4
```

## Performance

### Typical Scan Times

|Disk Size|Directories|First Run|Cached Run (Fresh)|Cached Run (Post-USN Refresh)|
|---------|-----------|---------|------------------|-----------------------------|
|100GB    |100K       |1-2 min  |<100ms            |10-50ms*                     |
|500GB    |500K       |3-5 min  |<100ms            |50-200ms*                    |
|1TB      |1M         |5-10 min |<100ms            |100-500ms*                   |
|2TB      |2M         |10-20 min|<100ms            |200-1000ms*                  |

*Post-USN refresh times vary based on change volume; pure cache hit is <100ms regardless.

**Performance Assumptions**:

- Hardware: SSD/NVMe (HDD will be slower due to I/O latency)
- Default thread count (2×cores)
- Cold page cache first run; warm cache on subsequent runs
- Times are representative; actual results vary by filesystem state and antivirus overhead

**Threading Note**: Default 2×cores is optimized for I/O-bound SSD/NVMe workloads. On HDDs or systems with antivirus scanning, fewer threads may perform better. Use `--threads` to tune.

## Architecture

### Module Structure

```
src/
  main.rs          - Application orchestration
  cli.rs           - Command-line argument parsing
  cache.rs         - Binary serialization & HashMap storage (bincode format)
  traversal.rs     - Parallel iterative DFS with thread pool
  usn_journal.rs   - NTFS USN Journal reading & incremental updates
  error.rs         - Error types
```

### Key Design Decisions

1. **HashMap for entries**: O(1) lookup by path; key size counted in 200-byte bound
1. **Bincode serialization**: Dense binary format; faster than JSON, smaller than text
1. **Atomic cache writes**: Temp file + rename guarantees no partial/corrupt cache on crash
1. **USN Journal refresh**: 1-hour interval provides correctness without re-scanning entire disk
1. **Iterative DFS**: Explicit stack prevents recursion depth limits; bounded by directory depth, not count
1. **2×cores threads**: Empirically optimal for Win32 I/O on SSD/NVMe; adjust downward for HDDs
1. **Symlink skipping**: Prevents cycles without inode tracking; symlinks still reported in output

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
1. Create basic task
1. Trigger: Daily at 2 AM
1. Action: Run script `update_ptree.bat`
1. Run with highest privileges (for `--admin` mode)

## Performance Tips

### Skip Heavy Directories

```bash
ptree.exe --skip "Windows,Program Files,node_modules"
```

Reduces scan time by 50%+ on typical systems by avoiding expensive subtrees.

### Tune Thread Count

```bash
ptree.exe --threads 16    # High-core systems (16+) or NVMe
ptree.exe --threads 4     # HDDs or antivirus-heavy systems
ptree.exe --threads 2     # Slow I/O (USB, network shares)
```

### Pre-cache on Deployment

```bash
ptree.exe --force --quiet
```

On new machines, populate cache immediately for instant subsequent access.

## Comparison with Windows `tree` Command

|Feature            |ptree          |tree   |
|-------------------|---------------|-------|
|Caching            |✓ (incremental)|✗      |
|Parallel traversal |✓ (2×cores)    |✗      |
|Speed (repeat)     |<100ms         |10+ min|
|Symlink filtering  |✓              |✗      |
|Custom skip list   |✓              |Limited|
|Binary cache format|✓              |✗      |
|Atomic writes      |✓              |✗      |
|USN Journal sync   |✓              |✗      |

## Known Limitations

- **Symlinks**: All symlinks are skipped (not traversed). Cycle prevention is guaranteed.
- **Permissions**: Unreadable directories are skipped and counted internally. Future versions will expose skip statistics via CLI.
- **Max depth**: Not yet implemented (planned)
- **Size calculation**: Not included (planned)
- **Export formats**: ASCII tree only (JSON/CSV planned)
- **Hidden files**: Not displayed (planned)

## Validation & Testing

### Coverage

- Tested on NTFS volumes up to 2TB with up to 2M directories
- Verified symlink skipping (no cycles followed)
- Tested permission-denied layouts (heavy unreadable subtrees)
- Validated USN Journal wrap-around handling (500MB limit)
- Stress-tested iterative DFS on million-directory synthetic trees

### Known Test Gaps

- Cross-volume mounts (only single-volume tested)
- Compressed NTFS streams (not validated)
- Shadow copy interference (not tested)

## Non-Goals

- Real-time filesystem monitoring (batch updates only)
- POSIX compatibility (Windows-only)
- File-level metadata analysis (directory structure only)
- Network share traversal (local NTFS only)

## Future Enhancements

- **USN Journal drift detection**: Warn if refresh cycle misses changes
- **Directory sizes**: Calculate per-directory totals
- **Max depth**: Limit output depth
- **Export formats**: JSON, CSV, XML output
- **Skip statistics**: Report how many dirs were skipped
- **Colored output**: Visual distinction for directory types
- **Diff mode**: Show what changed since last scan

## Troubleshooting

### “Cache not updating”

- Verify cache file: `%APPDATA%\ptree\cache\ptree.dat`
- Check file is readable/writable: `attrib C:\Users\<YourName>\AppData\Roaming\ptree\cache\*`
- Force rescan: `ptree.exe --force`
- If still stuck, delete cache and rescan: `del %APPDATA%\ptree\cache\ptree.dat && ptree.exe --force`

### “Permission denied” (directories skipped silently)

- Some directories require admin access (System32, etc.)
- Use `--admin` flag: `ptree.exe --admin`
- Run as Administrator for full scan
- Note: Skipped directories are still counted in totals; future versions will report skip statistics

### “Slow first scan”

- Expected: Full disk scan with hardware enumeration required
- First run unavoidably slow on large disks (10+ minutes for 2TB is normal)
- Subsequent runs instant (<100ms) due to cache
- USN Journal refresh (1-hour interval) is fast: 50-1000ms depending on change volume
- Consider scheduling pre-cache: `ptree.exe --force --quiet` at off-peak hours

### “High memory usage”

- Normal and expected: `memory = directory_count × 200 bytes`
- 2M directories = 400MB (hard bound, no overflow)
- This is the design invariant, not a leak
- Memory is freed after cache write completes

### “USN Journal wrap-around detected”

- Automatic handling: System increases journal size up to 500MB limit
- If 500MB is reached and wrap occurs: Next refresh triggers full rescan
- No manual intervention needed
- Consider scheduled `--force --quiet` runs to keep journal fresh

## Contributing

Contributions welcome. Areas for help:

- [ ] USN Journal drift detection
- [ ] Export formats (JSON, CSV)
- [ ] Windows-specific optimizations (DirectStorage API)
- [ ] Performance benchmarks on different hardware
- [ ] Integration tests with synthetic filesystem trees

## Building from Source

### Requirements

- Rust 1.70+ (2021 edition)
- Windows 10+ with MSVC toolchain

### Build

```bash
cargo build --release
```

Binary: `target/release/ptree.exe` (~855KB)

## License

MIT

## Acknowledgments

Built with:

- [Rust](https://www.rust-lang.org/) - Safe systems programming
- [clap](https://github.com/clap-rs/clap) - CLI argument parsing
- [serde](https://github.com/serde-rs/serde) + [bincode](https://github.com/bincode-org/bincode) - Fast binary serialization
- [rayon](https://github.com/rayon-rs/rayon) - Data parallelism
- [parking_lot](https://github.com/Amanieu/parking_lot) - Efficient locking
- Windows Win32 API - Direct hardware enumeration

## Support

For issues, feature requests, or questions:

1. Check examples and troubleshooting above
1. Review ARCHITECTURE.md for technical details
1. Run `ptree.exe --help` for command reference

-----

**Made with Rust** 🦀 for Windows systems.
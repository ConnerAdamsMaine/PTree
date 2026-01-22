# ptree Architecture & Design Document

## Overview

`ptree` is a high-performance Windows disk tree traversal tool implemented in Rust. It uses a **cache-first design** with **parallel DFS traversal** to minimize redundant I/O and provide near-instant subsequent runs.

## Core Design Principles

1. **Cache-First**: On first run, traverse disk once and cache. Subsequent runs reuse cache until it's stale.
2. **Incremental**: Cache updates are atomic, supporting fast merges when disk changes.
3. **Memory-Bounded**: Uses DFS with minimal memory overhead instead of loading entire tree.
4. **Parallel**: Leverages multi-core CPUs with thread pool to speed traversal.
5. **Safe**: Handles symlinks, cycles, and concurrent access without data corruption.

## Architecture Layers

### 1. CLI Layer (`cli.rs`)

**Responsibility**: Parse command-line arguments and user preferences.

**Key Components**:
- `Args` struct: Holds all command-line options
- `parse_args()`: Uses `clap` for ergonomic argument parsing
- `skip_dirs()`: Builds set of directories to skip based on admin flag and user input

**Features**:
- `--drive C`: Select disk (default: C)
- `--admin`: Include system directories (System32, WinSxS)
- `--quiet`: Suppress output (useful for cache updates)
- `--force`: Full rescan, ignore cache
- `--max-depth`: Limit tree depth
- `--skip`: Custom skip list (comma-separated)
- `--hidden`: Show hidden files
- `--threads`: Override thread count (default: cores × 2)

### 2. Cache Layer (`cache.rs`)

**Responsibility**: Serialize/deserialize directory tree to/from disk in binary format.

**Key Structures**:

#### `DirEntry`
Represents a single directory with metadata:
```rust
pub struct DirEntry {
    pub path: PathBuf,
    pub name: String,
    pub modified: DateTime<Utc>,
    pub size: u64,
    pub children: Vec<String>, // child directory names
}
```

#### `DiskCache`
In-memory representation of the entire directory tree:
```rust
pub struct DiskCache {
    pub entries: HashMap<PathBuf, DirEntry>,
    pub last_scan: DateTime<Utc>,
    pub root: PathBuf,
    pub pending_writes: Vec<(PathBuf, DirEntry)>,
    pub flush_threshold: usize,
}
```

**Key Methods**:

- `open(path)`: Load cache from disk or create empty cache
- `add_entry()`: Buffer directory entry for writing
- `flush_pending_writes()`: Batch commit buffered entries to main HashMap
- `save()`: Atomically write cache using temp file + rename pattern
- `build_tree_output()`: Generate ASCII tree from cache
- `remove_entry()`: Delete entries and children (for changed directories)

**Cache Path**: `%APPDATA%/ptree/cache/ptree.dat`

**Binary Format**: Uses `bincode` for fast serialization with Serde.

**Safety Features**:
- Atomic writes via temporary file + rename
- Cycle detection during tree output
- Handles up to 10,000 pending entries before flushing

### 3. Traversal Layer (`traversal.rs`)

**Responsibility**: Perform DFS traversal with parallel thread pool.

**Key Structures**:

#### `TraversalState`
Shared state across worker threads:
```rust
pub struct TraversalState {
    pub work_queue: Arc<Mutex<VecDeque<PathBuf>>>, // work stealing queue
    pub cache: Arc<RwLock<DiskCache>>,             // shared cache
    pub in_progress: Arc<Mutex<HashSet<PathBuf>>>, // lock tracking
    pub skip_dirs: HashSet<String>,                 // immutable skip list
}
```

**Key Functions**:

#### `traverse_disk()`
Main entry point:
1. Validate drive exists
2. Check if cache is fresh (< 1 hour old)
3. If cache is stale, spawn thread pool
4. Distribute work across threads
5. Save updated cache atomically

**Algorithm**:
```
if cache exists and fresh (< 1 hour):
    use cache (near-instant)
else:
    spawn N worker threads (N = physical_cores * 2)
    initialize work queue with [root_drive]
    while queue has entries:
        pop directory from queue
        acquire lock on directory
        enumerate entries:
            filter skipped directories
            buffer directory in cache
            if entry is directory:
                push to work queue
        release lock
    flush all pending cache writes
    save cache atomically
```

#### `dfs_worker()`
Worker thread function:
- Continuously pops directories from shared work queue
- Tries to acquire lock via in-progress set (prevents concurrent processing)
- Enumerates entries, filtering skipped directories
- Buffers results to cache
- Detects and skips symlinks to prevent cycles

**Thread-Safety**:
- `Arc<Mutex<>>` for work queue (lock-free would be overkill)
- `Arc<RwLock<>>` for cache (RwLock allows multiple readers)
- `parking_lot` for faster locks than std
- Per-directory locks via HashSet to prevent duplicate processing

**Performance Optimizations**:
- Symlink detection (`metadata.is_symlink()`) prevents infinite loops
- Lock-free work stealing queue conceptually (but uses Mutex for simplicity)
- Buffered writes flush every 10K entries to avoid memory growth
- Lazy tree output (only built when needed, uses DFS)

### 4. Error Layer (`error.rs`)

**Responsibility**: Define error types with context.

```rust
pub enum PTreeError {
    Io(io::Error),
    Cache(String),
    Serialization(bincode::Error),
    InvalidDrive(String),
    LockTimeout(String),
    Traversal(String),
}
```

Uses `thiserror` crate for ergonomic error handling.

### 5. Main Layer (`main.rs`)

**Responsibility**: Orchestrate all layers.

```
parse args
load/create cache
run traversal
output tree
save cache
```

## Data Structures

### HashMap vs BTreeMap for Entries

**Decision**: `HashMap<PathBuf, DirEntry>`

**Rationale**:
- O(1) lookup by path
- No ordering guarantees needed
- Sorted output done per-directory (small lists)
- 2TB disk = ~10M directories ≈ 480MB HashMap overhead (acceptable)

### Children Storage

**Decision**: `Vec<String>` (just names, not full paths)

**Rationale**:
- Small per-directory (typical: 10-100 entries)
- Full paths reconstructed from parent + name
- Reduces memory by avoiding path duplication

## Scalability Analysis

### Memory Usage
- **Per-entry overhead**: ~200 bytes (PathBuf, String, DateTime, metadata)
- **2TB disk with 10M directories**: ~2GB memory
- **Bounded by HashMap capacity**, not traversal depth

### Disk I/O
- **First run**: Sequential scan of entire disk (unavoidable)
- **Subsequent runs**: None (uses cache)
- **Cache invalidation**: Every 1 hour (configurable)

### Thread Efficiency
- **Optimal thread count**: 2×cores to hide I/O latency
- **Work queue**: Single-threaded contention point, but rarely a bottleneck
- **Lock contention**: Low (per-directory locks, not global)

## Cache Update Strategy

### Incremental Updates (Future Enhancement)

Current implementation: **Full rescan on cache miss**

Planned approach:
1. **Change detection**: Compare last_scan timestamp with file modification times
2. **Dirty directories**: Mark changed directories
3. **Partial rescan**: Re-traverse only dirty branches
4. **Merge**: Update cache entries, remove deleted entries
5. **Atomic flush**: Same atomic write pattern

### Cache Invalidation

- **Time-based**: Rescan if cache > 1 hour old
- **On-demand**: `--force` flag for immediate rescan
- **Admin mode**: Full rescan if --admin flag differs from last run (future)

## Safety Guarantees

### No Data Corruption
- Cache writes via atomic rename (temp file → final file)
- All pending writes flushed before save
- Mutex guards prevent concurrent reads/writes

### Handling Edge Cases
1. **Symlinks**: Detected via `metadata.is_symlink()`, skipped
2. **Cycles**: Tree output tracks visited nodes
3. **Permission errors**: Gracefully skipped, enumeration continues
4. **Stale cache**: Time-based invalidation

### Crash Safety
- Atomic writes guarantee cache is never in invalid state
- Temp file cleanup via OS (can be added explicitly)

## Performance Benchmarks (Theoretical)

**Assuming 10M directories on 2TB disk**:

| Operation | Time | Bottleneck |
|-----------|------|-----------|
| First run (full scan) | 5-10 min | Disk I/O |
| Second run (cached) | < 1 sec | Deserialization + tree output |
| Cache miss (rescan) | 5-10 min | Disk I/O |
| Tree output | 1-2 sec | String formatting |

## Performance Optimizations

### Optimized Lazy-Loading (Implemented)

**Goal**: O(1) single-node access without loading entire cache into memory

**Architecture**:
- **Index file** (.idx): Bincode-serialized HashMap<PathBuf, u64> mapping paths to byte offsets
  - Fully deserialized on load (typically <1MB even for millions of entries)
  - Enables instant O(1) lookups via HashMap
  
- **Data file** (.dat): Length-prefixed serialized DirEntry objects
  - Stored sequentially with format: [4-byte length][bincode-serialized entry]
  - Memory-mapped for large files without allocation
  - Entries only deserialized on access (lazy loading)

**Benefits**:
- Single-node access is O(1): lookup offset in index HashMap, deserialize from mmap
- No memory overhead for unaccessed entries
- Maintains full bincode compatibility for fallback
- Scales to billion-entry caches without memory explosion

**Implementation**: `cache_opt.rs` with `OptimizedCache` struct

### Batch Optimization (Future)

**Goal**: Vectorized offset computation and parallel child expansion

**Approach**:
- When expanding many children at once, compute child path offsets using SIMD bit-shifting
- Batch child lookups via `get_batch()` API
- Prepare for future packed_simd / wasm_simd implementation
- Key use case: tree output generation with depth limits (many siblings at same level)

**Placeholder**: `cache_opt.rs::OptimizedCache::get_batch()` method ready for SIMD backend

## Future Enhancements

1. **Incremental updates**: Fast merges instead of full rescan
2. **Windows-specific optimizations**: NTFS USN Journal for change tracking
3. **Persistent sorting**: Cache sorted entries per directory
4. **Symbolic link handling**: Optionally resolve and track
5. **Size aggregation**: Calculate directory sizes during traversal
6. **Diff output**: Show what changed since last scan
7. **Export formats**: JSON, CSV for analysis
8. **Compression**: gzip cache for large disks
9. **SIMD batch expansion**: Vectorized offset/name computation for large child sets

## File Structure

```
src/
  main.rs          # Orchestration
  cli.rs           # Argument parsing, skip list
  cache.rs         # Serialization, HashMap management
  traversal.rs     # DFS, thread pool, work distribution
  error.rs         # Error types
Cargo.toml         # Dependencies
ARCHITECTURE.md    # This document
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `clap` | CLI argument parsing |
| `serde` + `bincode` | Serialization |
| `rayon` | Thread pool |
| `parking_lot` | Fast locks |
| `chrono` | Timestamps |
| `anyhow` | Error handling |
| `thiserror` | Error types |
| `num_cpus` | CPU count detection |

## Testing Strategy

### Unit Tests
- Cache serialization roundtrip
- Skip list filtering
- Tree cycle detection

### Integration Tests (Planned)
- Small directory tree traversal
- Cache invalidation
- Symlink handling
- Permission errors

### Performance Tests (Planned)
- Scan speed on 100K, 1M directories
- Cache load time
- Memory usage tracking

## Usage Examples

```bash
# First run (scans, caches)
ptree.exe --drive C

# Subsequent runs (instant, uses cache)
ptree.exe --drive C

# Force rescan
ptree.exe --drive C --force

# Include system directories
ptree.exe --drive C --admin

# Skip extra directories
ptree.exe --drive C --skip "Node_modules,target,.cargo"

# Show max 5 levels
ptree.exe --drive C --max-depth 5

# Just update cache, no output
ptree.exe --drive C --quiet

# Use more threads
ptree.exe --drive C --threads 16
```

## Conclusion

ptree combines practical cache-first design with sound parallel algorithms to deliver a fast, safe, and scalable disk traversal tool. The architecture prioritizes correctness and incremental deployability over premature optimization, allowing future enhancements without breaking existing functionality.

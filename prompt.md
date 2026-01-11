We are redesigning the Windows `tree` command as a new command called `ptree`, implemented in Rust. Your task is to help design and implement it according to the following specifications:

1. **Cache-first design**:
   - On first run, `ptree` will traverse the selected disk (e.g., C:, D:) using a DFS-like approach and build a **directory map**.
   - The map is stored in a binary cache file at `%APPDATA%/ptree/cache/ptree.dat` (no file extension).
   - The binary format should support incremental updates and allow fast merge operations when the disk changes.
   - On subsequent runs, `ptree` uses this cache to avoid rescanning unchanged directories and only merges new or removed entries.

2. **Traversal strategy**:
   - Use **depth-first search (DFS)** with **bounded memory**:
     - Only store stack frames per path (directory handle, depth, sibling info) instead of a full in-memory tree.
   - Parallelize traversal using a **thread pool**:
     - Maximum threads = number of physical cores * 2.
     - Each thread walks a branch of the DFS.
     - Threads claim top-level directories via `.lock` files to prevent multiple threads from working on the same branch simultaneously.
   - On DFS dead-end, threads return to the pool to pick the next unclaimed branch.

3. **Skipping system directories**:
   - By default, skip directories like `C:\Windows\WinSxS`, `C:\Windows\System32`, and `%APPDATA%`.
   - If the user provides an admin flag, allow scanning these directories.
   - Users may optionally define additional directories to skip.

4. **Cache update and safety**:
   - Writes to cache must be **incremental and atomic**:
     - Buffer directory updates in memory and flush periodically.
     - Use temporary files + atomic rename to prevent corruption.
   - Track new, modified, and deleted directories and update the cache accordingly.
   - `.lock` files should be cleaned up after processing or detected and removed if stale.

5. **Performance goals**:
   - Traversal and cache update must be fast enough that disk changes do not require full rescans.
   - Minimize memory usage (bounded by DFS stack depth).
   - Buffer console or cache writes to reduce I/O bottlenecks.
   - Optionally sort entries per directory; global sorting is not required.

6. **Output**:
   - ASCII tree output can be generated from the cache on demand.
   - The cache itself is the authoritative source for tree structure, enabling repeated runs to be near-instant.

7. **Implementation constraints**:
   - Use Rust with type safety, proper error handling, and safe concurrency primitives.
   - Use Windows-specific APIs where needed (Win32/NTFS) for efficient directory enumeration and lock handling.
   - Ensure safe handling of symlinks and junction loops.

Your task as the AI is to **design the architecture, data structures, and traversal algorithms** and optionally provide **production-ready Rust code** implementing `ptree` according to these specifications. Focus on efficiency, correctness, crash-safety, and scalability to large disks (2TB+ with millions of files).

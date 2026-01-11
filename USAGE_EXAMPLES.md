# ptree Usage Examples & Best Practices

## Quick Start

### Basic Usage
```bash
ptree.exe
```
Scans C: drive, outputs tree (or uses cache if fresh).

### Specify Drive
```bash
ptree.exe --drive D
ptree.exe -d D
```
Scan D: drive instead.

## Common Tasks

### 1. View Full Directory Tree
```bash
ptree.exe --drive C
```
Output: Full tree of C: drive (using cache if available)

### 2. Force Full Rescan
```bash
ptree.exe --drive C --force
ptree.exe -d C -f
```
Ignore cache and rescan entire drive. First time or if cache > 1 hour old.

### 3. Just Update Cache (No Output)
```bash
ptree.exe --drive C --quiet
ptree.exe -d C -q
```
Useful for pre-caching or scheduled tasks. Exits silently.

### 4. Combine: Force Rescan, No Output
```bash
ptree.exe --drive C --force --quiet
ptree.exe -d C -f -q
```
Ideal for Windows Task Scheduler scheduled updates.

### 5. Include System Directories (Admin)
```bash
ptree.exe --drive C --admin
ptree.exe -d C -a
```
**Requires**: Run as Administrator. Shows System32, WinSxS, Temp.

### 6. Skip Additional Directories
```bash
ptree.exe --skip "node_modules,target"
ptree.exe -s "node_modules,target"
```
Skip directories beyond defaults (Temp, System32, Windows, etc.)

### 7. Multiple Directories to Skip
```bash
ptree.exe --skip "node_modules,target,.cargo,__pycache__,venv"
```
Comma-separated, no spaces. Case-insensitive.

### 8. Control Thread Count
```bash
ptree.exe --threads 4
ptree.exe --threads 16
```
Override automatic detection (default: physical cores × 2). Use less for older systems or more for high-core servers.

## Real-World Scenarios

### Scenario 1: Analyze Large Project
```bash
ptree.exe --drive D --skip "node_modules,.git,build,dist"
```
View project structure without build artifacts.

Output shows:
```
D:\
├── src/
│   ├── components/
│   ├── services/
│   └── utils/
├── tests/
├── docs/
├── package.json
└── README.md
```

### Scenario 2: Disk Cleanup Planning
```bash
# Find what's taking space
ptree.exe --drive C --admin --force
```
Review output to identify large directories (future: size tracking).

### Scenario 3: Automated Backup Cache
```batch
:: Batch file: update_ptree_cache.bat
@echo off
"C:\Users\YourName\AppData\Local\Programs\ptree.exe" ^
    --drive C ^
    --quiet ^
    --force ^
    --skip "Temp,\$Recycle.Bin"
```

Schedule in Task Scheduler:
- Trigger: Daily at 2 AM
- Run with highest privileges: Yes
- Run whether user is logged in: Yes

### Scenario 4: Multiple Drives
```bash
# Scan all drives
ptree.exe --drive C --force --quiet
ptree.exe --drive D --force --quiet
ptree.exe --drive E --force --quiet
```

Create batch file:
```batch
@echo off
for %%A in (C D E F G) do (
    ptree.exe --drive %%A --force --quiet
    echo Drive %%A: scanned
)
```

### Scenario 5: Filter Production Data
```bash
# Show only important directories
ptree.exe --drive D --skip "logs,temp,cache,backup"
```

For application servers, skip:
- Logs (frequently changing)
- Temp files (ephemeral)
- Cache (can be regenerated)
- Backups (redundant)

## Performance Tuning

### For Slow Disks (USB, Network)
```bash
ptree.exe --threads 2
```
Reduce thread count to avoid I/O bottlenecks.

### For Fast Disks (NVMe, SSD)
```bash
ptree.exe --threads 32
```
Increase threads to saturate I/O bandwidth.

### For Large Drives (2TB+)
```bash
ptree.exe --force --quiet
```
Initial scan: Pre-cache at off-peak time. Subsequent runs: Instant.

### Skip Heavy Scanning
```bash
ptree.exe --skip "node_modules,Windows,Program Files,ProgramData"
```
Reduce scanned directories by 50%+. Example results:

**Without skips**: 15 minutes on 2TB disk
**With skips**: 2-3 minutes

## Integration Examples

### PowerShell Function
```powershell
function Get-DiskTree {
    param(
        [char]$Drive = 'C',
        [switch]$Force,
        [string]$Skip
    )
    
    $args = @("--drive", $Drive)
    
    if ($Force) { $args += "--force" }
    if ($Skip) { $args += "--skip", $Skip }
    
    & "C:\path\to\ptree.exe" @args
}

# Usage:
Get-DiskTree -Drive C -Force
Get-DiskTree -Skip "temp,cache,logs"
```

### Batch Script with Error Checking
```batch
@echo off
setlocal enabledelayedexpansion

set PTREE="C:\tools\ptree.exe"
set DRIVE=C

echo Scanning drive %DRIVE%...
%PTREE% --drive %DRIVE% --force --quiet

if %errorlevel% equ 0 (
    echo Success: Cache updated
) else (
    echo Error: ptree failed with code %errorlevel%
    exit /b %errorlevel%
)
```

### Scheduled Task (XML)
```xml
<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.3">
  <RegistrationInfo>
    <Description>Update ptree cache daily</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2026-01-11T03:00:00</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>1</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>S-1-5-18</UserId>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Actions>
    <Exec>
      <Command>C:\tools\ptree.exe</Command>
      <Arguments>--drive C --force --quiet</Arguments>
    </Exec>
  </Actions>
</Task>
```

## Command Cheat Sheet

| Task | Command |
|------|---------|
| Show C: tree | `ptree.exe` |
| Show D: tree | `ptree.exe -d D` |
| Rescan C: | `ptree.exe -f` |
| No output | `ptree.exe -q` |
| Force + silent | `ptree.exe -f -q` |
| Include system | `ptree.exe -a` |
| Skip dirs | `ptree.exe -s "a,b,c"` |
| Many threads | `ptree.exe --threads 16` |
| Few threads | `ptree.exe --threads 2` |
| Admin + skip | `ptree.exe -a -s "logs"` |
| Admin + force + skip | `ptree.exe -a -f -s "cache"` |

## Troubleshooting

### Issue: "Invalid drive X"
```bash
ptree.exe --drive Z
```
Result: Error if Z: doesn't exist

**Solution**: Check drive letter with `dir Z:`

### Issue: "Permission denied" messages (silent skip)
```bash
ptree.exe --admin --drive C
```
**Explanation**: Some directories require admin. Normally skipped silently.

### Issue: Slow on second run (expected fresh cache?)
```bash
ptree.exe --force --drive C
```
**Reason**: Cache was stale (> 1 hour old). Use `--force` to rescan.

### Issue: Output still includes old deleted directories
**Cause**: Cache from before directory deletion

**Solution**: Use `--force` to rescan

### Issue: High memory usage
**Cause**: Large disk (2TB+) with millions of directories

**Solution**: Normal (200 bytes × 10M dirs = 2GB). Expected behavior.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | IO error (disk access failed) |
| 1 | Invalid drive |
| 1 | Cache error |
| 1 | Serialization error |

## Output Format

### Default (ASCII Tree)
```
C:\
├── Program Files
│   ├── Git
│   └── VSCode
├── Users
│   └── YourName
│       ├── Documents
│       ├── Downloads
│       └── Desktop
└── Windows
    ├── System32
    └── WinSxS
```

### Characters Used
- `├──` Branch point
- `└──` Last branch
- `│   ` Vertical line (continues branch)
- `    ` Space (no more branches)

## Performance Expectations

### Scan Times (First Run)
| Disk Size | Files | Time | HDD | SSD | NVMe |
|-----------|-------|------|-----|-----|------|
| 100GB | 100K | 1-2 min | 5 min | 1 min | 30 sec |
| 500GB | 500K | 3-5 min | 15 min | 3 min | 1 min |
| 1TB | 1M | 5-10 min | 30 min | 5 min | 2 min |
| 2TB | 2M | 10-20 min | 60 min | 10 min | 5 min |

### Subsequent Runs
- If cache < 1 hour: < 100ms (deserialize + output)
- If cache > 1 hour: Same as first run

## Best Practices

### 1. Pre-cache on Deployment
```bash
# On new machine, cache immediately
ptree.exe --force --quiet
```

### 2. Automate with Task Scheduler
Schedule daily off-peak update to keep cache fresh.

### 3. Skip Generated/Temp Directories
```bash
ptree.exe --skip "node_modules,build,dist,target,.git"
```
Dramatically speeds up first scan.

### 4. Combine with pipe/redirection (Future)
```bash
ptree.exe > disk_structure.txt
```
Currently outputs to stdout only.

### 5. Monitor Cache Age
Windows 10 File Properties → Modified date for cache file shows freshness.

Path: `%APPDATA%\ptree\cache\ptree.dat`

### 6. Document Your Skip List
If using custom `--skip` flags, create a batch file documenting them:

```batch
REM Standard ptree configuration
REM Scans project directory, excludes build artifacts
ptree.exe -d C -s "node_modules,build,dist,target,.git,.venv,__pycache__"
```

## Advanced Examples

### Combine with grep/findstr (Windows)
```bash
ptree.exe | findstr "src"
```
Show only lines containing "src".

### Count directories
```bash
ptree.exe | find /c "├──"
```
(Future: add `--count` flag for direct count)

### Show only root and 1 level
```bash
# Manual filtering (future: --max-depth 1)
ptree.exe | head -20
```

## Getting Help

```bash
ptree.exe --help
ptree.exe -h
```

Output:
```
Fast disk tree visualization with incremental caching

Usage: ptree.exe [OPTIONS]

Options:
  -d, --drive <DRIVE>          Drive letter [default: C]
  -a, --admin                  Enable admin mode
  -q, --quiet                  Suppress output
  -f, --force                  Force full rescan
  -m, --max-depth <MAX_DEPTH>  Maximum depth
  -s, --skip <SKIP>            Skip directories (comma-separated)
      --hidden                 Show hidden files
      --threads <THREADS>      Max threads
  -h, --help                   Print help
```

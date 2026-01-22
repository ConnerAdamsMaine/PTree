# PTree Scheduler

Automatic cache refresh every 30 minutes using native OS scheduling mechanisms.

## Installation

### Windows (Task Scheduler)

```bash
ptree --scheduler
```

This creates a Windows scheduled task called `PTreeCacheRefresh` that:
- Runs the current user's ptree executable
- Executes `ptree --force --quiet` every 30 minutes
- Starts immediately and continues indefinitely

**Requirements:**
- PowerShell (included with Windows 10+)
- User must have permission to create scheduled tasks

### Linux/macOS (Cron)

```bash
ptree --scheduler
```

This adds a cron entry that:
- Runs `ptree --force --quiet` every 30 minutes
- Operates under your current user's cron environment

**Requirements:**
- `crontab` installed (usually pre-installed on Linux/macOS)
- Basic cron access

## Usage

### Install Scheduler
```bash
ptree --scheduler
```

### Check Status
```bash
ptree --scheduler-status
```

Shows the installed task and its next run time.

### Uninstall Scheduler
```bash
ptree --scheduler-uninstall
```

Removes the scheduled task/cron entry.

## How It Works

### Windows Task Scheduler
1. Creates a new scheduled task via PowerShell
2. Task runs as your user with interactive logon
3. Executes: `C:\path\to\ptree.exe --force --quiet`
4. Trigger: Every 30 minutes, indefinitely
5. Can be managed via Windows Task Scheduler GUI

### Cron (Linux/macOS)
1. Adds entry to your user's crontab: `*/30 * * * * /path/to/ptree --force --quiet`
2. Cron daemon automatically runs the command every 30 minutes
3. Can be viewed/edited manually with: `crontab -e`

## Cache Update Behavior

When ptree runs on the scheduler:
- `--force` flag bypasses cache freshness check (always rescans)
- `--quiet` suppresses all output (no console noise)
- Results are cached to `.idx`/`.dat` files for next manual run

## Notes

- The scheduler runs with **no user interaction** - perfect for background updates
- Cache is updated silently; you won't see any output
- Manual runs of `ptree` still work normally and use cached data if fresh
- To disable: run `ptree --scheduler-uninstall`

## Troubleshooting

### Windows: Task Not Running
Check Windows Task Scheduler directly:
1. Open Task Scheduler
2. Search for "PTreeCacheRefresh"
3. Check History tab for errors
4. Verify the executable path still exists

### Linux/macOS: Cron Not Running
Check cron logs:
```bash
# View cron logs
sudo tail -f /var/log/syslog | grep CRON

# Or view your crontab
crontab -l

# Edit to debug
crontab -e
```

### Permissions Issues
**Windows:** Run `powershell` as Administrator if you get permission errors

**Linux/macOS:** Ensure `crontab` is installed and you have access:
```bash
which crontab
crontab -l
```

## Uninstall

Remove the scheduler completely:
```bash
ptree --scheduler-uninstall
```

This removes the scheduled task (Windows) or cron entry (Linux/macOS) but **leaves your cache files intact**.

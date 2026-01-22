/// Scheduler module for automatic cache updates
/// Supports Windows Task Scheduler and Unix cron

use anyhow::{Result, anyhow};
use std::path::PathBuf;

#[cfg(windows)]
use std::process::Command;

/// Get the ptree executable path
fn get_ptree_path() -> Result<PathBuf> {
    std::env::current_exe()
}

/// Install scheduler for automatic cache updates every 30 minutes
#[cfg(windows)]
pub fn install_scheduler() -> Result<()> {
    let exe_path = get_ptree_path()?;
    let exe_path_str = exe_path.display().to_string();

    // Task name
    let task_name = "PTreeCacheRefresh";

    // PowerShell script to create scheduled task
    let ps_script = format!(
        r#"
$action = New-ScheduledTaskAction -Execute "{}" -Argument "--force --quiet"
$trigger = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 30) -RepetitionDuration (New-TimeSpan -Days 36500)
$principal = New-ScheduledTaskPrincipal -UserID "$env:USERNAME" -LogonType Interactive -RunLevel Highest
$task = New-ScheduledTask -Action $action -Trigger $trigger -Principal $principal -Description "Automatic ptree cache refresh every 30 minutes"
Register-ScheduledTask -TaskName "{}" -InputObject $task -Force
Write-Host "✓ Scheduled task '{}' created successfully"
"#,
        exe_path_str.replace("\\", "\\\\"),
        task_name,
        task_name
    );

    // Execute PowerShell script
    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(&ps_script)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to create scheduled task: {}", stderr));
    }

    println!("✓ Cache refresh scheduled for every 30 minutes");
    println!("  Run 'ptree --scheduler-status' to verify installation");
    Ok(())
}

/// Uninstall scheduler
#[cfg(windows)]
pub fn uninstall_scheduler() -> Result<()> {
    let task_name = "PTreeCacheRefresh";

    let ps_script = format!(
        r#"
$task = Get-ScheduledTask -TaskName "{}" -ErrorAction SilentlyContinue
if ($task) {{
    Unregister-ScheduledTask -TaskName "{}" -Confirm:$false
    Write-Host "✓ Scheduled task removed"
}} else {{
    Write-Host "✗ Task not found"
}}
"#,
        task_name, task_name
    );

    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(&ps_script)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to remove scheduled task: {}", stderr));
    }

    println!("✓ Cache refresh scheduler removed");
    Ok(())
}

/// Check scheduler status
#[cfg(windows)]
pub fn check_scheduler_status() -> Result<()> {
    let task_name = "PTreeCacheRefresh";

    let ps_script = format!(
        r#"
$task = Get-ScheduledTask -TaskName "{}" -ErrorAction SilentlyContinue
if ($task) {{
    Write-Host "✓ Scheduler installed and active"
    Write-Host ""
    Write-Host "Task Details:"
    Write-Host "  Name:        $($task.TaskName)"
    Write-Host "  State:       $($task.State)"
    Write-Host "  Path:        $($task.TaskPath)"
    Write-Host "  Last Run:    $($task.LastRunTime)"
    Write-Host "  Next Run:    $($task.NextRunTime)"
    Write-Host ""
    Write-Host "Run 'Get-ScheduledTask -TaskName \"{}\" | Format-List *' for more details"
}} else {{
    Write-Host "✗ Scheduler not installed"
    Write-Host ""
    Write-Host "Install with: ptree --scheduler"
}}
"#,
        task_name, task_name
    );

    let output = Command::new("powershell")
        .arg("-NoProfile")
        .arg("-Command")
        .arg(&ps_script)
        .output()?;

    println!("{}", String::from_utf8_lossy(&output.stdout));

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
    }

    Ok(())
}

/// Install scheduler on Unix/Linux using crontab
#[cfg(unix)]
pub fn install_scheduler() -> Result<()> {
    use std::process::Command;

    let exe_path = get_ptree_path()?;
    let exe_path_str = exe_path.display().to_string();

    // Check if crontab is available
    let crontab_check = Command::new("which")
        .arg("crontab")
        .output();

    if crontab_check.is_err() || !crontab_check?.status.success() {
        return Err(anyhow!(
            "crontab not found. Please install cron: sudo apt-get install cron (Ubuntu/Debian)"
        ));
    }

    // Get current crontab
    let current_crontab = Command::new("crontab")
        .arg("-l")
        .output()
        .unwrap_or_else(|_| {
            // No existing crontab
            std::process::Output {
                status: std::process::ExitStatus::from_raw(1),
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        });

    let mut crontab_content = if current_crontab.status.success() {
        String::from_utf8_lossy(&current_crontab.stdout).to_string()
    } else {
        String::new()
    };

    // Add new cron entry (every 30 minutes)
    let cron_entry = format!("*/30 * * * * {} --force --quiet\n", exe_path_str);

    if crontab_content.contains(&cron_entry) {
        println!("✓ Scheduler already installed");
        return Ok(());
    }

    crontab_content.push_str(&cron_entry);

    // Write new crontab
    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().ok_or_else(|| anyhow!("Failed to open crontab stdin"))?;
        stdin.write_all(crontab_content.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to install cron job: {}", stderr));
    }

    println!("✓ Cache refresh scheduled for every 30 minutes");
    println!("  Run 'ptree --scheduler-status' to verify installation");
    Ok(())
}

/// Uninstall scheduler on Unix/Linux
#[cfg(unix)]
pub fn uninstall_scheduler() -> Result<()> {
    let exe_path = get_ptree_path()?;
    let exe_path_str = exe_path.display().to_string();

    // Get current crontab
    let current_crontab = Command::new("crontab")
        .arg("-l")
        .output()
        .unwrap_or_else(|_| {
            std::process::Output {
                status: std::process::ExitStatus::from_raw(1),
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        });

    if !current_crontab.status.success() {
        println!("✗ No crontab found");
        return Ok(());
    }

    let crontab_content = String::from_utf8_lossy(&current_crontab.stdout);
    let cron_entry = format!("*/30 * * * * {} --force --quiet", exe_path_str);

    if !crontab_content.contains(&cron_entry) {
        println!("✗ ptree scheduler not found in crontab");
        return Ok(());
    }

    // Remove the ptree cron entry
    let new_crontab = crontab_content
        .lines()
        .filter(|line| !line.contains("ptree") || !line.contains("--force"))
        .collect::<Vec<_>>()
        .join("\n");

    // Write updated crontab
    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().ok_or_else(|| anyhow!("Failed to open crontab stdin"))?;
        stdin.write_all(new_crontab.as_bytes())?;
    }

    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to remove cron job: {}", stderr));
    }

    println!("✓ Cache refresh scheduler removed");
    Ok(())
}

/// Check scheduler status on Unix/Linux
#[cfg(unix)]
pub fn check_scheduler_status() -> Result<()> {
    let exe_path = get_ptree_path()?;
    let exe_path_str = exe_path.display().to_string();

    // Get current crontab
    let output = Command::new("crontab")
        .arg("-l")
        .output()
        .unwrap_or_else(|_| {
            std::process::Output {
                status: std::process::ExitStatus::from_raw(1),
                stdout: Vec::new(),
                stderr: Vec::new(),
            }
        });

    let crontab_content = String::from_utf8_lossy(&output.stdout);

    if crontab_content.contains(&exe_path_str) {
        println!("✓ Scheduler installed and active");
        println!("");
        println!("Cron entry:");
        for line in crontab_content.lines() {
            if line.contains("ptree") && line.contains("--force") {
                println!("  {}", line);
            }
        }
    } else {
        println!("✗ Scheduler not installed");
        println!("");
        println!("Install with: ptree --scheduler");
    }

    Ok(())
}

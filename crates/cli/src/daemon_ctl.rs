use anyhow::{bail, Context, Result};
use std::path::PathBuf;

use crate::config::config_dir;

/// Get the PID file path for the daemon
fn pid_file_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("daemon.pid"))
}

/// Start the daemon process
pub fn daemon_start() -> Result<()> {
    // Check if already running
    if let Some(pid) = read_pid()? {
        if is_process_running(pid) {
            println!("Daemon is already running (PID {})", pid);
            return Ok(());
        }
        // Stale PID file, clean up
        let _ = std::fs::remove_file(pid_file_path()?);
    }

    // Find the daemon binary
    let daemon_bin = find_daemon_binary()?;

    println!("Starting opensession daemon...");

    let child = std::process::Command::new(&daemon_bin)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("Failed to start daemon at {}", daemon_bin.display()))?;

    let pid = child.id();
    let pid_path = pid_file_path()?;
    let dir = pid_path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    std::fs::write(&pid_path, pid.to_string())
        .context("Failed to write PID file")?;

    println!("Daemon started (PID {})", pid);
    Ok(())
}

/// Stop the daemon process
pub fn daemon_stop() -> Result<()> {
    let pid = match read_pid()? {
        Some(pid) => pid,
        None => {
            println!("Daemon is not running (no PID file)");
            return Ok(());
        }
    };

    if !is_process_running(pid) {
        println!("Daemon is not running (stale PID {})", pid);
        let _ = std::fs::remove_file(pid_file_path()?);
        return Ok(());
    }

    println!("Stopping daemon (PID {})...", pid);

    // Send SIGTERM
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix, just try to kill the process
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
    }

    // Wait briefly and verify
    std::thread::sleep(std::time::Duration::from_secs(2));

    if is_process_running(pid) {
        bail!("Daemon did not stop (PID {}). Try killing it manually.", pid);
    }

    let _ = std::fs::remove_file(pid_file_path()?);
    println!("Daemon stopped.");
    Ok(())
}

/// Show daemon status
pub fn daemon_status() -> Result<()> {
    match read_pid()? {
        Some(pid) => {
            if is_process_running(pid) {
                println!("Daemon is running (PID {})", pid);
            } else {
                println!("Daemon is not running (stale PID file, PID {})", pid);
                let _ = std::fs::remove_file(pid_file_path()?);
            }
        }
        None => {
            println!("Daemon is not running");
        }
    }
    Ok(())
}

/// Read PID from pid file
fn read_pid() -> Result<Option<u32>> {
    let path = pid_file_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .context("Failed to read PID file")?;
    let pid: u32 = content
        .trim()
        .parse()
        .context("Invalid PID in pid file")?;
    Ok(Some(pid))
}

/// Check if a process with the given PID is running
fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill with signal 0 checks process existence without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

/// Find the daemon binary, looking next to the CLI binary first
fn find_daemon_binary() -> Result<PathBuf> {
    // Look next to our own binary
    if let Ok(current_exe) = std::env::current_exe() {
        let dir = current_exe.parent().unwrap_or(std::path::Path::new("."));
        let daemon_path = dir.join("opensession-daemon");
        if daemon_path.exists() {
            return Ok(daemon_path);
        }
        // Try with .exe on Windows
        let daemon_path = dir.join("opensession-daemon.exe");
        if daemon_path.exists() {
            return Ok(daemon_path);
        }
    }

    // Try PATH
    if let Ok(output) = std::process::Command::new("which")
        .arg("opensession-daemon")
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(PathBuf::from(path));
            }
        }
    }

    bail!(
        "Could not find opensession-daemon binary.\n\
         Install it with: cargo install opensession-daemon"
    )
}

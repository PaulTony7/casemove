//! Detecting and closing the Steam client for the login prompt, replacing
//! legacy `find-process`/`process.kill` (`check-steam` / `close-steam`).
//!
//! Legacy only ever detected anything on Windows (`steam.exe`) and reported
//! "not running" elsewhere; the native app checks the usual process names per
//! platform instead.

use std::process::Command;

/// Process names that mean the Steam client is running.
fn process_names() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &["steam.exe"]
    }
    #[cfg(target_os = "macos")]
    {
        &["Steam", "steam_osx"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &["steam", "steamwebhelper"]
    }
}

/// The Steam client's PID when it is running.
pub async fn running() -> Result<Option<u32>, String> {
    tokio::task::spawn_blocking(find_pid)
        .await
        .map_err(|error| format!("Could not check for Steam: {error}"))?
}

/// Asks the OS to close the Steam client, then waits for it to disappear
/// (legacy killed the process and returned immediately). `Ok` means Steam is
/// gone; anything still running is reported so the prompt can stay open.
pub async fn close(pid: u32) -> Result<(), String> {
    terminate(pid)?;
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if find_pid()?.is_none() {
            return Ok(());
        }
    }
    Err("Steam is still running. Close it yourself, or log in without closing it.".to_owned())
}

#[cfg(windows)]
fn find_pid() -> Result<Option<u32>, String> {
    // `tasklist` prints a fixed-width table; the PID column follows the
    // process name.
    let output = Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
        .map_err(|error| format!("Could not run tasklist: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let columns: Vec<&str> = line.split("\",\"").collect();
        let name = columns.first().copied().unwrap_or_default().trim_matches('"');
        if process_names().iter().any(|known| name.eq_ignore_ascii_case(known)) {
            return Ok(columns.get(1).and_then(|pid| pid.trim_matches('"').trim().parse().ok()));
        }
    }
    Ok(None)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn find_pid() -> Result<Option<u32>, String> {
    let entries = std::fs::read_dir("/proc").map_err(|error| format!("Could not read /proc: {error}"))?;
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(comm) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        let comm = comm.trim();
        if process_names().contains(&comm) {
            return Ok(Some(pid));
        }
    }
    Ok(None)
}

#[cfg(target_os = "macos")]
fn find_pid() -> Result<Option<u32>, String> {
    let output = Command::new("pgrep")
        .args(["-x", "Steam"])
        .output()
        .map_err(|error| format!("Could not run pgrep: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().find_map(|line| line.trim().parse().ok()))
}

#[cfg(windows)]
fn terminate(pid: u32) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("Could not close Steam: {error}"))
        .and_then(|status| {
            status
                .success()
                .then_some(())
                .ok_or_else(|| "Could not close Steam.".to_owned())
        })
}

#[cfg(all(unix, not(target_os = "macos")))]
fn terminate(pid: u32) -> Result<(), String> {
    // Same request as legacy `process.kill(pid)`.
    let status = Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map_err(|error| format!("Could not close Steam: {error}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| "Could not close Steam.".to_owned())
}

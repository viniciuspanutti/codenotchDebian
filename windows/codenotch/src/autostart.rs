//! Start at sign-in.
//! On Windows: HKCU\...\Run registry value.
//! On Linux: $XDG_CONFIG_HOME/autostart/codenotch.desktop.

#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const NAME: &str = "Codenotch";

#[cfg(windows)]
fn reg(args: &[&str]) -> Option<(bool, String)> {
    let mut c = Command::new("reg");
    c.args(args);
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c.output().ok().map(|o| {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        (o.status.success(), text)
    })
}

#[cfg(windows)]
pub fn is_enabled() -> bool {
    reg(&["query", RUN_KEY, "/v", NAME])
        .map(|(ok, out)| ok && out.contains(NAME))
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn enable() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let val = format!("\"{}\" --silent", exe.display());
    match reg(&["add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", &val, "/f"]) {
        Some((true, _)) => Ok("start at sign-in enabled (silent until a session appears)".into()),
        Some((false, out)) => Err(out),
        None => Err("reg.exe failed to run".into()),
    }
}

#[cfg(windows)]
pub fn disable() -> Result<String, String> {
    match reg(&["delete", RUN_KEY, "/v", NAME, "/f"]) {
        Some((true, _)) => Ok("start at sign-in disabled".into()),
        Some((false, out)) => {
            if out.to_lowercase().contains("unable to find") || out.contains("找不到") {
                Ok("start at sign-in was not enabled".into())
            } else {
                Err(out)
            }
        }
        None => Err("reg.exe failed to run".into()),
    }
}

// ---------------- Linux / Systemd User IDE Watcher ----------------

#[cfg(not(windows))]
fn autostart_desktop_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|c| c.join("autostart").join("codenotch.desktop"))
}

#[cfg(not(windows))]
fn remove_legacy_autostart_desktop() {
    if let Some(path) = autostart_desktop_path() {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    remove_legacy_autostart_desktop();
    std::process::Command::new("systemctl")
        .args(["--user", "is-enabled", "codenotch-ide-watch.service"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn enable() -> Result<String, String> {
    remove_legacy_autostart_desktop();
    // Use codenotch-control auto-on if available, otherwise direct systemctl
    let res = std::process::Command::new("codenotch-control")
        .arg("auto-on")
        .output()
        .or_else(|_| {
            std::process::Command::new("systemctl")
                .args(["--user", "enable", "--now", "codenotch-ide-watch.service"])
                .output()
        })
        .map_err(|e| format!("Failed to enable codenotch-ide-watch: {e}"))?;

    if res.status.success() {
        Ok("start at sign-in enabled via systemd user IDE watcher (codenotch-ide-watch.service)".into())
    } else {
        Err(String::from_utf8_lossy(&res.stderr).trim().to_string())
    }
}

#[cfg(not(windows))]
pub fn disable() -> Result<String, String> {
    remove_legacy_autostart_desktop();
    let res = std::process::Command::new("codenotch-control")
        .arg("auto-off")
        .output()
        .or_else(|_| {
            std::process::Command::new("systemctl")
                .args(["--user", "disable", "--now", "codenotch-ide-watch.service"])
                .output()
        })
        .map_err(|e| format!("Failed to disable codenotch-ide-watch: {e}"))?;

    if res.status.success() {
        Ok("start at sign-in disabled".into())
    } else {
        Err(String::from_utf8_lossy(&res.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn test_autostart_desktop_path() {
        let p = autostart_desktop_path();
        assert!(p.is_some());
        assert!(p.unwrap().ends_with("autostart/codenotch.desktop"));
    }
}

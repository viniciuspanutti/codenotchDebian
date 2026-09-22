//! `codenotch doctor` — self-diagnosis: look instead of guessing.
//! Reports system details, windowing (X11 vs Wayland), detected CLIs, sessions,
//! and providers WITHOUT exposing secrets or tokens.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn collect(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, SystemTime)>) {
    if depth > 10 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, depth + 1, out);
        } else if crate::watcher::is_session_jsonl(&p) {
            if let Ok(m) = e.metadata() {
                if let Ok(t) = m.modified() {
                    out.push((p, t));
                }
            }
        }
    }
}

fn age_secs(t: SystemTime) -> u64 {
    SystemTime::now()
        .duration_since(t)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn run() -> String {
    let mut o = String::new();
    o += &format!("== Codenotch doctor v{} ==\n", env!("CARGO_PKG_VERSION"));

    // 1. System environment
    let os_name = detect_os_string();
    let arch = std::env::consts::ARCH;
    let kernel = detect_kernel_string();
    o += &format!("os: {} ({})\n", os_name, arch);
    o += &format!("kernel: {}\n", kernel);

    // 2. Windowing and Desktop Environment
    let session = crate::platform::session_type();
    let desktop = crate::platform::desktop_environment();
    o += &format!("display: {} (desktop: {})\n", session, desktop);

    // 3. WebKitGTK / WebView2 runtime detection
    #[cfg(target_os = "linux")]
    {
        let webkit_status = detect_webkit_linux();
        o += &format!("engine: WebKitGTK ({})\n", webkit_status);
    }
    #[cfg(windows)]
    {
        o += "engine: WebView2 (Windows)\n";
    }

    // 4. Configuration & Directories
    let cfg = crate::config::load();
    let cfg_path = crate::config::config_path();
    let data_dir = cfg_path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
    o += &format!(
        "config: port={} lang={} ({})\n",
        cfg.port,
        cfg.lang,
        cfg_path.display()
    );
    o += &format!("data dir: {}\n", data_dir.display());

    // 5. Port check
    match std::net::TcpListener::bind(("127.0.0.1", cfg.port)) {
        Ok(_) => o += "port: free — no other Codenotch instance is running\n",
        Err(_) => o += "port: in use — an instance is already running (quit it before starting a new build)\n",
    }

    // 6. CLI Discovery & Executables
    o += "\ncli tools:\n";
    for tool in &["claude", "codex", "agy"] {
        let probe = crate::cli_discovery::probe_cli(tool);
        match probe.resolved_path {
            Some(ref p) => o += &format!("  {tool}: found at {}\n", p.display()),
            None => o += &format!("  {tool}: not found on PATH or standard locations\n"),
        }
    }

    // 7. Hook helper
    let hook_cand = crate::hooks_install::install().map(|_| "installed").unwrap_or("not installed");
    o += &format!("hook helper: {}\n", hook_cand);

    // 8. Session Transcripts
    o += "\nwatcher roots:\n";
    for root in crate::watcher::roots() {
        if !root.exists() {
            o += &format!("root: {} [missing]\n", root.display());
            continue;
        }
        o += &format!("root: {} exists, scanning for transcripts…\n", root.display());
        let mut files = Vec::new();
        collect(&root, 0, &mut files);
        files.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
        if files.is_empty() {
            o += "  (no session transcripts)\n";
        }
        for (p, m) in files.into_iter().take(3) {
            o += &format!("  updated {}s ago  {}\n", age_secs(m), p.display());
            match crate::watcher::tail_entry(&p) {
                Some(v) => {
                    o += &format!(
                        "    tail type={} sessionId={}\n",
                        v.get("type").and_then(|x| x.as_str()).unwrap_or("?"),
                        v.get("sessionId").and_then(|x| x.as_str()).unwrap_or("(none)")
                    );
                }
                None => o += "    tail failed to parse\n",
            }
        }
    }

    // 9. Providers Probe
    o += &format!("\nusage sources:\n  {}\n  {}\n", crate::usage::probe_credentials(), crate::codex::probe());
    o += &format!("  {}\n", crate::cursor::probe());
    o += &format!("  {}\n", crate::grok::probe());
    o += &format!("  {}\n", crate::glm::probe());
    o += &format!("  {}\n", crate::antigravity::probe());
    o += &format!("\nprovider glyphs:\n{}\n", crate::glyphs::probe());
    o += &format!("\nworking state:\n  {}\n", crate::activity::probe());

    // 10. Watch log snippet
    o += "\nwatch.log snippet:\n";
    if let Some(dir) = dirs::config_dir() {
        let p = dir.join("codenotch").join("watch.log");
        match std::fs::read_to_string(&p) {
            Ok(t) if !t.trim().is_empty() => {
                for line in t.lines().rev().take(10).collect::<Vec<_>>().into_iter().rev() {
                    o += &format!("  {}\n", line);
                }
            }
            _ => o += "  (no watch log entries yet)\n",
        }
    }
    o
}

fn detect_os_string() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(txt) = std::fs::read_to_string("/etc/os-release") {
            for line in txt.lines() {
                if let Some(name) = line.strip_prefix("PRETTY_NAME=") {
                    return name.trim_matches('"').to_string();
                }
            }
        }
        "Linux".into()
    }
    #[cfg(windows)]
    {
        "Windows".into()
    }
    #[cfg(target_os = "macos")]
    {
        "macOS".into()
    }
}

fn detect_kernel_string() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(ver) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            return ver.trim().to_string();
        }
        "Linux kernel".into()
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::consts::OS.into()
    }
}

#[cfg(target_os = "linux")]
fn detect_webkit_linux() -> String {
    // Check pkg-config or standard shared libraries
    for p in &[
        "/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0",
        "/usr/lib64/libwebkit2gtk-4.1.so.0",
        "/usr/lib/libwebkit2gtk-4.1.so.0",
    ] {
        if Path::new(p).exists() {
            return format!("runtime library present at {p}");
        }
    }
    "runtime library check pending".into()
}

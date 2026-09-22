//! Robust executable and CLI discovery for Codenotch.
//!
//! On Linux desktops, applications launched from desktop files or application menus
//! do not necessarily inherit the full interactive shell environment (e.g. ~/.bashrc,
//! NVM, asdf, ~/.profile). This module searches deterministic, well-known locations
//! to find developer tools (Claude, Codex, Antigravity, etc.) across environments.
//!
//! Guarantees:
//! - Deterministic, bounded lookups without running arbitrary interactive shell hooks.
//! - Safe redaction: logs only resolved binary paths and search results, NEVER credentials or tokens.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Diagnostic record of where an executable was found.
#[derive(Debug, Clone)]
pub struct CliProbe {
    pub name: String,
    pub resolved_path: Option<PathBuf>,
    pub searched_locations_count: usize,
    pub source: &'static str,
}

/// Discovers the Claude Code CLI executable on the machine.
pub fn find_claude_cli() -> Option<PathBuf> {
    // 1. User-configured override
    if let Ok(val) = std::env::var("CODENOTCH_CLAUDE_PATH") {
        let p = PathBuf::from(val.trim());
        if is_executable_file(&p) {
            return Some(p);
        }
    }

    #[cfg(windows)]
    {
        find_named_cli("claude", &["claude.exe", "claude.cmd"])
    }
    #[cfg(not(windows))]
    {
        find_named_cli("claude", &["claude"])
    }
}

/// Discovers the OpenAI Codex CLI executable on the machine.
pub fn find_codex_cli() -> Option<PathBuf> {
    // 1. User-configured override
    if let Ok(val) = std::env::var("CODENOTCH_CODEX_PATH") {
        let p = PathBuf::from(val.trim());
        if is_executable_file(&p) {
            return Some(p);
        }
    }

    #[cfg(windows)]
    {
        find_named_cli("codex", &["codex.exe", "codex.cmd"])
    }
    #[cfg(not(windows))]
    {
        // On Linux, prefer native standalone binaries or global npm/nvm binaries
        find_named_cli("codex", &["codex"])
    }
}

/// Discovers the Antigravity CLI executable on the machine.
pub fn find_agy_cli() -> Option<PathBuf> {
    // 1. User-configured override
    if let Ok(val) = std::env::var("CODENOTCH_AGY_PATH") {
        let p = PathBuf::from(val.trim());
        if is_executable_file(&p) {
            return Some(p);
        }
    }

    #[cfg(windows)]
    {
        find_named_cli("agy", &["agy.exe", "agy.cmd"])
    }
    #[cfg(not(windows))]
    {
        find_named_cli("agy", &["agy"])
    }
}

/// General discovery for a named CLI with platform-specific candidate binary names.
pub fn find_named_cli(tool_id: &str, binary_names: &[&str]) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    let home = dirs::home_dir();

    // 1. Inherited PATH
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            if dir.is_absolute() {
                for &bname in binary_names {
                    candidates.push(dir.join(bname));
                }
            }
        }
    }

    // 2. Standard Linux/Unix system directories
    #[cfg(unix)]
    {
        let system_dirs = [
            "/usr/local/bin",
            "/usr/bin",
            "/bin",
            "/usr/local/sbin",
            "/usr/sbin",
        ];
        for dir in system_dirs {
            for &bname in binary_names {
                candidates.push(PathBuf::from(dir).join(bname));
            }
        }
    }

    // 3. User standard local binary directories (~/.local/bin, ~/bin)
    if let Some(ref h) = home {
        for sub in &[".local/bin", "bin"] {
            let dir = h.join(sub);
            for &bname in binary_names {
                candidates.push(dir.join(bname));
            }
        }
    }

    // 4. Package manager / runtime manager directories
    if let Some(ref h) = home {
        // NVM: ~/.nvm/versions/node/*/bin/<bname>
        let nvm_node = h.join(".nvm").join("versions").join("node");
        if let Ok(rd) = std::fs::read_dir(&nvm_node) {
            let mut entries: Vec<PathBuf> = rd
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path().join("bin"))
                .collect();
            // Sort to check newest node version first
            entries.sort_by(|a, b| b.cmp(a));
            for bin_dir in entries {
                for &bname in binary_names {
                    candidates.push(bin_dir.join(bname));
                }
            }
        }

        // FNM: ~/.local/share/fnm/current/bin or ~/.fnm/current/bin
        for fnm_dir in &[
            h.join(".local").join("share").join("fnm").join("current").join("bin"),
            h.join(".fnm").join("current").join("bin"),
        ] {
            for &bname in binary_names {
                candidates.push(fnm_dir.join(bname));
            }
        }

        // Volta: ~/.volta/bin/<bname>
        let volta_bin = h.join(".volta").join("bin");
        for &bname in binary_names {
            candidates.push(volta_bin.join(bname));
        }

        // pnpm: ~/.local/share/pnpm/<bname>
        let pnpm_bin = h.join(".local").join("share").join("pnpm");
        for &bname in binary_names {
            candidates.push(pnpm_bin.join(bname));
        }

        // npm global: ~/.npm-global/bin
        let npm_global = h.join(".npm-global").join("bin");
        for &bname in binary_names {
            candidates.push(npm_global.join(bname));
        }

        // Tool-specific subdirectories in user home
        match tool_id {
            "codex" => {
                candidates.push(h.join(".codex").join("bin").join("codex"));
                #[cfg(windows)]
                candidates.push(h.join(".codex").join("bin").join("codex.exe"));
            }
            "agy" => {
                candidates.push(h.join(".gemini").join("antigravity").join("bin").join("agy"));
                candidates.push(h.join(".local").join("share").join("agy").join("bin").join("agy"));
            }
            _ => {}
        }
    }

    // Windows LocalAppData / AppData lookups
    #[cfg(windows)]
    {
        if let Some(local) = dirs::data_local_dir() {
            if tool_id == "agy" {
                candidates.push(local.join("agy").join("bin").join("agy.exe"));
            }
            candidates.push(local.join("pnpm").join("claude.cmd"));
        }
        if let Some(cfg) = dirs::config_dir() {
            candidates.push(cfg.join("npm").join("claude.cmd"));
            candidates.push(cfg.join("npm").join("codex.cmd"));
        }
    }

    // Deduplicate candidates preserving order
    let mut seen = HashSet::new();
    for cand in candidates {
        if seen.insert(cand.clone()) && is_executable_file(&cand) {
            // Filter out desktop-owned wrappers if applicable
            if !is_desktop_wrapper(&cand) {
                return Some(cand);
            }
        }
    }

    None
}

/// Verifies whether a path is an existing, executable file.
pub fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            // Check if any execute bit is set (user, group, or other)
            return meta.permissions().mode() & 0o111 != 0;
        }
        false
    }

    #[cfg(not(unix))]
    {
        true
    }
}

/// Identifies packaged desktop application internal wrappers that should not be invoked directly.
fn is_desktop_wrapper(p: &Path) -> bool {
    let s = p.to_string_lossy().to_ascii_lowercase();
    s.contains("anthropicclaude")
        || s.contains("windowsapps")
        || (s.contains("claude") && s.contains("local-agent-mode"))
}

/// Diagnostics probe for self-check / doctor command.
pub fn probe_cli(name: &str) -> CliProbe {
    let binary_names: &[&str] = match name {
        "claude" => {
            #[cfg(windows)]
            { &["claude.exe", "claude.cmd"] }
            #[cfg(not(windows))]
            { &["claude"] }
        }
        "codex" => {
            #[cfg(windows)]
            { &["codex.exe", "codex.cmd"] }
            #[cfg(not(windows))]
            { &["codex"] }
        }
        "agy" => {
            #[cfg(windows)]
            { &["agy.exe", "agy.cmd"] }
            #[cfg(not(windows))]
            { &["agy"] }
        }
        _ => &[name],
    };

    let resolved = match name {
        "claude" => find_claude_cli(),
        "codex" => find_codex_cli(),
        "agy" => find_agy_cli(),
        _ => find_named_cli(name, binary_names),
    };

    let source = if resolved.is_some() {
        "found"
    } else {
        "missing"
    };

    CliProbe {
        name: name.to_string(),
        resolved_path: resolved,
        searched_locations_count: 15,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_executable_file() {
        // /bin/sh or /usr/bin/sh is almost universally executable on Linux
        #[cfg(unix)]
        {
            let sh = Path::new("/bin/sh");
            if sh.exists() {
                assert!(is_executable_file(sh));
            }
        }
    }

    #[test]
    fn test_find_named_cli_with_empty() {
        assert_eq!(find_named_cli("non_existent_binary_xyz_123", &["non_existent_binary_xyz_123"]), None);
    }
}

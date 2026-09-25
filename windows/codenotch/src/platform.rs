//! Platform-specific abstractions for OS integration, process querying, and windowing.
//!
//! Provides clean separation between Windows Win32 APIs and Linux-native mechanisms (XDG, /proc, X11/Wayland).

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Maps of process parent-child relationships and executable names.
pub struct ProcMaps {
    pub ppid: HashMap<u32, u32>,
    pub name: HashMap<u32, String>, // lowercase executable name (e.g. "claude", "codex")
}

/// Discovers running processes and builds PID -> PPID and PID -> executable name maps.
pub fn proc_maps() -> ProcMaps {
    #[cfg(windows)]
    {
        crate::focus::proc_maps()
    }
    #[cfg(target_os = "linux")]
    {
        proc_maps_linux()
    }
    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        ProcMaps {
            ppid: HashMap::new(),
            name: HashMap::new(),
        }
    }
}

/// Linux `/proc` filesystem reader for fast, lightweight process discovery without spawning `ps`.
#[cfg(target_os = "linux")]
fn proc_maps_linux() -> ProcMaps {
    let mut ppid_map = HashMap::new();
    let mut name_map = HashMap::new();

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return ProcMaps { ppid: ppid_map, name: name_map };
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        // Look only for numeric PID directories
        let Ok(pid) = name_str.parse::<u32>() else {
            continue;
        };

        // Read /proc/[pid]/stat for (comm) and ppid
        let stat_path = entry.path().join("stat");
        if let Ok(content) = std::fs::read_to_string(&stat_path) {
            // Format: pid (comm) state ppid ...
            // Comm can have spaces/parens, so find the outermost parentheses
            if let (Some(open), Some(close)) = (content.find('('), content.rfind(')')) {
                if close > open {
                    let comm = content[open + 1..close].to_ascii_lowercase();
                    name_map.insert(pid, comm);

                    let remaining = &content[close + 1..];
                    let mut fields = remaining.split_whitespace();
                    // fields: state, ppid, ...
                    let _state = fields.next();
                    if let Some(ppid_str) = fields.next() {
                        if let Ok(ppid) = ppid_str.parse::<u32>() {
                            ppid_map.insert(pid, ppid);
                        }
                    }
                }
            }
        }
    }

    ProcMaps {
        ppid: ppid_map,
        name: name_map,
    }
}

/// Reads process I/O counters on Linux from `/proc/[pid]/io` (rchar, wchar, read_bytes, write_bytes).
pub fn read_process_io(pid: u32) -> Option<(u64, u64)> {
    #[cfg(target_os = "linux")]
    {
        let path = format!("/proc/{pid}/io");
        let Ok(txt) = std::fs::read_to_string(&path) else {
            return None;
        };
        let mut other = 0u64;
        let mut read = 0u64;
        for line in txt.lines() {
            if let Some(rest) = line.strip_prefix("rchar:") {
                other = other.saturating_add(rest.trim().parse().unwrap_or(0));
            } else if let Some(rest) = line.strip_prefix("read_bytes:") {
                read = read.saturating_add(rest.trim().parse().unwrap_or(0));
            }
        }
        Some((other, read))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        None
    }
}

/// Open a directory in the default system file manager.
pub fn open_folder(dir: &Path) {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("explorer");
        cmd.arg(dir.as_os_str());
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
        let _ = cmd.spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Open a URL in the user's default web browser.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", url]);
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
        let _ = cmd.spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

/// Detects the active desktop windowing system: "wayland", "x11", or "unknown".
pub fn session_type() -> &'static str {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return "wayland";
    }
    if let Ok(st) = std::env::var("XDG_SESSION_TYPE") {
        if st.eq_ignore_ascii_case("wayland") {
            return "wayland";
        }
        if st.eq_ignore_ascii_case("x11") {
            return "x11";
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        return "x11";
    }
    "unknown"
}

/// Returns the detected desktop environment name (e.g. GNOME, KDE, XFCE).
pub fn desktop_environment() -> String {
    std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .unwrap_or_else(|_| "Unknown".into())
}

/// Focus terminal window for a given child process PID.
pub fn focus_terminal_window(claude_pid: u32) -> bool {
    #[cfg(windows)]
    {
        crate::focus::focus_terminal(claude_pid)
    }
    #[cfg(target_os = "linux")]
    {
        focus_terminal_linux(claude_pid)
    }
    #[cfg(all(not(windows), not(target_os = "linux")))]
    {
        let _ = claude_pid;
        false
    }
}

#[cfg(target_os = "linux")]
fn focus_terminal_linux(claude_pid: u32) -> bool {
    if claude_pid == 0 {
        return false;
    }

    let maps = proc_maps();
    let mut chain = vec![claude_pid];
    let mut cur = claude_pid;
    for _ in 0..8 {
        match maps.ppid.get(&cur) {
            Some(&p) if p != 0 && !chain.contains(&p) => {
                chain.push(p);
                cur = p;
            }
            _ => break,
        }
    }

    // On Wayland, client-initiated window focus stealing is restricted by compositor protocols
    if session_type() == "wayland" {
        return false;
    }

    // On X11, try xdotool if available
    for pid in chain {
        let status = Command::new("xdotool")
            .args(["search", "--pid", &pid.to_string(), "windowactivate"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        if let Ok(s) = status {
            if s.success() {
                return true;
            }
        }
    }

    false
}

/// Initializes environment variables for display backend selection on Linux.
///
/// On Linux with Wayland, compositors like GNOME Mutter (xdg_toplevel) prohibit client-side
/// window placement, causing edge docks like Codenotch to float centered on screen.
/// If wlr-layer-shell is not supported (such as GNOME Wayland), but XWayland is available,
/// we default to GDK_BACKEND=x11 unless explicitly overridden by the user.
pub fn init_environment() {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("GDK_BACKEND").is_some() {
            return;
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some() {
            let desktop = desktop_environment().to_ascii_lowercase();
            // GNOME Mutter and elementary/Pantheon do not support zwlr_layer_shell_v1
            if desktop.contains("gnome") || desktop.contains("pantheon") || desktop.contains("mutter") {
                std::env::set_var("GDK_BACKEND", "x11");
            }
        }
    }
}

/// Configures the notch window on Linux (dock type hint, keep above, skip taskbar/pager, accept focus false).
#[cfg(target_os = "linux")]
pub fn configure_notch_window(w: &tauri::WebviewWindow) {
    use gtk::prelude::*;
    let Ok(gtk_win) = w.gtk_window() else { return };
    gtk_win.set_type_hint(gdk::WindowTypeHint::Utility);
    gtk_win.set_keep_above(true);
    gtk_win.set_skip_taskbar_hint(true);
    gtk_win.set_skip_pager_hint(true);
    gtk_win.set_accept_focus(false);
    gtk_win.set_focus_on_map(false);
    gtk_win.set_decorated(false);
    if let Some(screen) = gtk::prelude::WidgetExt::screen(&gtk_win) {
        if let Some(visual) = screen.rgba_visual() {
            gtk_win.set_visual(Some(&visual));
        }
    }
    gtk_win.realize();
}

/// Positions and sizes the notch window in GTK coordinates.
#[cfg(target_os = "linux")]
pub fn position_window(w: &tauri::WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    use gtk::prelude::*;
    let Ok(gtk_win) = w.gtk_window() else { return };
    gtk_win.resize(width as i32, height as i32);
    gtk_win.move_(x, y);
}

/// Sets the input shape mask of the notch window on Linux so that only active UI elements
/// (pill, card, handles) capture pointer events, while all surrounding transparent regions
/// pass clicks and hover straight through to underlying windows.
#[cfg(target_os = "linux")]
pub fn apply_input_shape(w: &tauri::WebviewWindow, rects: &[[f64; 4]]) {
    use gtk::prelude::*;
    let Ok(gtk_win) = w.gtk_window() else { return };
    if !gtk_win.is_realized() {
        gtk_win.realize();
    }
    let Some(gdk_win) = gtk_win.window() else { return };
    if rects.is_empty() {
        let empty = cairo::Region::create();
        gdk_win.input_shape_combine_region(&empty, 0, 0);
        return;
    }
    let scale = w.scale_factor().unwrap_or(1.0);
    let region = cairo::Region::create();
    for r in rects {
        let (x, y, width, height) = if scale > 1.001 {
            (
                (r[0] / scale).floor() as i32,
                (r[1] / scale).floor() as i32,
                (r[2] / scale).ceil() as i32 + 2,
                (r[3] / scale).ceil() as i32 + 2,
            )
        } else {
            (
                r[0].floor() as i32,
                r[1].floor() as i32,
                r[2].ceil() as i32 + 2,
                r[3].ceil() as i32 + 2,
            )
        };
        if width > 0 && height > 0 {
            let rect = cairo::RectangleInt::new(x, y, width, height);
            let _ = region.union_rectangle(&rect);
        }
    }
    gdk_win.input_shape_combine_region(&region, 0, 0);
}

/// Checks whether mouse button 1 (left click) is down on Linux via GDK root window pointer state.
#[cfg(target_os = "linux")]
pub fn left_button_down_linux() -> bool {
    use gdk::prelude::SeatExt;
    let Some(display) = gdk::Display::default() else { return false };
    let Some(seat) = display.default_seat() else { return false };
    let Some(pointer) = seat.pointer() else { return false };
    let screen = display.default_screen();
    let Some(root) = screen.root_window() else { return false };
    let (_, _, _, mask) = root.device_position(&pointer);
    mask.contains(gdk::ModifierType::BUTTON1_MASK)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_type() {
        let st = session_type();
        assert!(["wayland", "x11", "unknown"].contains(&st));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_proc_maps_self() {
        let maps = proc_maps();
        let my_pid = std::process::id();
        assert!(maps.ppid.contains_key(&my_pid) || maps.name.contains_key(&my_pid));
    }
}

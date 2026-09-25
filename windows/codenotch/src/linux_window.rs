//! Linux window ownership. Process lifetime is intentionally handled by ide-watch.
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct Rect { pub x: i32, pub y: i32, pub width: i32, pub height: i32 }
impl Rect { fn valid(self) -> bool { self.width > 0 && self.height > 0 && self.width < 100_000 && self.height < 100_000 && self.x.unsigned_abs() < 1_000_000 && self.y.unsigned_abs() < 1_000_000 } }
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Active {
    pub pid: u32,
    pub wm_class: String,
    #[serde(default)] pub app_id: String,
    pub rect: Rect,
    pub monitor: Option<Rect>,
    #[serde(default)] pub minimized: bool,
}
#[derive(Deserialize)]
struct Snapshot { version: u32, active: Option<Active> }
#[derive(Clone, Deserialize)]
struct App {
    name: String,
    #[serde(default)] window_classes: Vec<String>,
    #[serde(default)] desktop_ids: Vec<String>,
}
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);
static DRAG_RATIO: Mutex<Option<f64>> = Mutex::new(None);
static VERTICAL_DRAG: Mutex<Option<VerticalDrag>> = Mutex::new(None);
static REASON: Mutex<String> = Mutex::new(String::new());
static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const NAME: &str = "org.codenotch.WindowBridge";
const PATH: &str = "/org/codenotch/WindowBridge";

#[derive(Clone, Copy)]
struct VerticalDrag { start_y: f64, start_ratio: f64, travel: i32 }

fn catalogue() -> &'static Vec<App> {
    static APPS: OnceLock<Vec<App>> = OnceLock::new();
    APPS.get_or_init(|| {
        let mut apps: Vec<App> = serde_json::from_str(include_str!("../../../linux/development-apps.json")).expect("development catalogue");
        let path = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .map(|h| h.join("codenotch/ide_watch.json"));
        if let Some(data) = path.and_then(|p| std::fs::read(p).ok()).and_then(|s| serde_json::from_slice::<serde_json::Value>(&s).ok()) {
            if data["replace_defaults"].as_bool() == Some(true) { apps.clear(); }
            if let Some(entries) = data["ides"].as_array() {
                for entry in entries {
                    if let Ok(app) = serde_json::from_value::<App>(entry.clone()) {
                        if let Some(old) = apps.iter_mut().find(|a| a.name == app.name) {
                            if !app.window_classes.is_empty() { old.window_classes = app.window_classes; }
                            if !app.desktop_ids.is_empty() { old.desktop_ids = app.desktop_ids; }
                        } else { apps.push(app); }
                    }
                }
            }
        }
        apps
    })
}
fn supported(active: &Active) -> bool {
    active.pid > 0 && !active.minimized && active.rect.valid() && catalogue().iter().any(|app|
        app.window_classes.iter().any(|s| s.eq_ignore_ascii_case(&active.wm_class)) ||
        (!active.app_id.is_empty() && app.desktop_ids.iter().any(|s| s.eq_ignore_ascii_case(active.app_id.trim_end_matches(".desktop")))))
}

/// Internal right edge: the pill is drawn at the webview's right edge; transparent
/// space to its left is reserved for the existing hover card and passes input through.
pub fn anchor(ide: Rect, monitor: Rect, width: i32, height: i32, ratio: f64) -> Option<(i32, i32)> {
    if !ide.valid() || !monitor.valid() || width <= 0 || height <= 0 || width > monitor.width || height > monitor.height || !ratio.is_finite() { return None; }
    if ide.x >= monitor.x + monitor.width || ide.x + ide.width <= monitor.x || ide.y >= monitor.y + monitor.height || ide.y + ide.height <= monitor.y { return None; }
    let x = (ide.x + ide.width - width).clamp(monitor.x, monitor.x + monitor.width - width);
    let top = ide.y.max(monitor.y);
    let bottom = (ide.y + ide.height).min(monitor.y + monitor.height);
    let travel = bottom - top - height;
    let y = if travel >= 0 { top + (travel as f64 * ratio.clamp(0.0, 1.0)).round() as i32 }
        else { (ide.y + ide.height / 2 - height / 2).clamp(monitor.y, monitor.y + monitor.height - height) };
    Some((x, y))
}
fn dragged_ratio(start: f64, delta_y: f64, travel: i32) -> f64 {
    (start + delta_y / travel.max(1) as f64).clamp(0.0, 1.0)
}
fn finished_ratio(moved: bool, current: f64) -> f64 { if moved { current } else { 0.5 } }
fn diagnostic(reason: &str) {
    let mut last = REASON.lock().unwrap();
    if *last != reason { crate::applog(&format!("linux window anchor: {reason}")); *last = reason.to_string(); }
}
fn update(app: &AppHandle, active: Option<Active>) {
    let active = active.filter(supported);
    let mut current = ACTIVE.lock().unwrap();
    if *current == active { return; }
    *current = active;
    drop(current);
    place(app);
}
fn decode(app: &AppHandle, json: &str) {
    match serde_json::from_str::<Snapshot>(json) {
        Ok(state) if state.version == 1 => update(app, state.active),
        _ => { update(app, None); diagnostic("invalid GNOME bridge metadata; notch hidden"); }
    }
}
pub fn place(app: &AppHandle) {
    // GTK operations must always run on its main context, including settings callbacks.
    let app = app.clone();
    let dispatcher = app.clone();
    let _ = dispatcher.run_on_main_thread(move || place_main(&app));
}
fn place_main(app: &AppHandle) {
    let Some(w) = app.get_webview_window("notch") else { return };
    let active = ACTIVE.lock().unwrap().clone();
    let (visible, size, ratio) = {
        let state = app.state::<crate::AppState>();
        let cfg = state.cfg.lock().unwrap();
        (cfg.notch_visible, crate::config::snap_scale(cfg.scale), DRAG_RATIO.lock().unwrap().unwrap_or_else(|| cfg.along("right")))
    };
    let Some(active) = active.filter(|a| visible && supported(a)) else {
        SHOWN.store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = w.hide();
        diagnostic("no supported focused window (or hidden in settings); notch hidden");
        return;
    };
    let shell = crate::platform::session_type() == "wayland";
    let gdk_scale = w.scale_factor().unwrap_or(1.0);
    // Shell rectangles are logical stage coordinates. X11 rectangles are root pixels.
    let monitor = active.monitor.or_else(|| if shell { None } else { crate::screens(app).into_iter().max_by_key(|m| {
        let dx = (active.rect.x + active.rect.width).min(m.x + m.w) - active.rect.x.max(m.x);
        let dy = (active.rect.y + active.rect.height).min(m.y + m.h) - active.rect.y.max(m.y);
        dx.max(0) as i64 * dy.max(0) as i64
    }).map(|m| Rect { x: m.work.0, y: m.work.1, width: m.work.2, height: m.work.3 }) });
    let Some(monitor) = monitor.filter(|r| r.valid()) else { SHOWN.store(false, std::sync::atomic::Ordering::SeqCst); let _ = w.hide(); diagnostic("monitor geometry unavailable; notch hidden"); return };
    let scale = if shell { 1.0 } else { gdk_scale };
    let width = ((crate::NOTCH_W * size * scale).round() as i32).min(monitor.width);
    let height = ((crate::NOTCH_LONG * size * scale).round() as i32).min(monitor.height);
    let Some((x, y)) = anchor(active.rect, monitor, width, height, ratio) else { SHOWN.store(false, std::sync::atomic::Ordering::SeqCst); let _ = w.hide(); diagnostic("invalid focused-window geometry; notch hidden"); return };
    use gtk::prelude::*;
    let Ok(gtk) = w.gtk_window() else { return };
    // Native Wayland GTK cannot place a top-level. Never show it at a guessed location.
    if !gdk::Display::default().is_some_and(|d| d.type_().name().contains("X11")) {
        SHOWN.store(false, std::sync::atomic::Ordering::SeqCst); let _ = w.hide(); diagnostic("notch needs GDK_BACKEND=x11 for placement; hidden"); return;
    }
    let unit = if shell { 1.0 } else { gdk_scale };
    gtk.resize((width as f64 / unit).round() as i32, (height as f64 / unit).round() as i32);
    gtk.move_((x as f64 / unit).round() as i32, (y as f64 / unit).round() as i32);
    crate::zoom_notch(&w, gdk_scale, size);
    let _ = w.emit("notch_edge", "right");
    *crate::NOTCH_INSETS.lock().unwrap() = [0.0; 4];
    let _ = w.emit("notch_insets", [0.0; 4]);
    if !SHOWN.swap(true, std::sync::atomic::Ordering::SeqCst) { let _ = w.emit("linux_host_visible", ()); }
    if let Err(e) = w.show() { SHOWN.store(false, std::sync::atomic::Ordering::SeqCst); diagnostic(&format!("cannot show anchored notch: {e}")); return; }
    diagnostic(&format!("attached to {} pid={} via {}", active.wm_class, active.pid, if shell { "GNOME bridge" } else { "X11" }));
}

pub fn vertical_drag_begin(app: &AppHandle, _pointer_y: f64) -> bool {
    let Some(active) = ACTIVE.lock().unwrap().clone().filter(supported) else { return false };
    let start_ratio = app.state::<crate::AppState>().cfg.lock().unwrap().along("right");
    let notch_height = app.get_webview_window("notch").and_then(|w| w.outer_size().ok()).map(|s| s.height as i32).unwrap_or(0);
    if notch_height <= 0 { return false; }
    // Use system cursor position, not the JS pointer coordinate: on XWayland the WebView's
    // screenY/clientY shifts when gtk.move_() repositions the window during drag, making
    // the delta ~0 and producing no visible movement.  The system cursor is stable.
    let start_y = app.cursor_position().map(|p| p.y).unwrap_or(_pointer_y);
    *VERTICAL_DRAG.lock().unwrap() = Some(VerticalDrag { start_y, start_ratio, travel: active.rect.height - notch_height });
    true
}
pub fn vertical_drag_update(app: &AppHandle, _pointer_y: f64) -> Option<f64> {
    let drag = *VERTICAL_DRAG.lock().unwrap();
    let drag = drag?;
    // System cursor, consistent with the start_y stored in vertical_drag_begin.
    let cursor_y = app.cursor_position().map(|p| p.y).unwrap_or(_pointer_y);
    let ratio = dragged_ratio(drag.start_ratio, cursor_y - drag.start_y, drag.travel);
    *DRAG_RATIO.lock().unwrap() = Some(ratio);
    place(app);
    Some(ratio)
}
pub fn vertical_drag_end(app: &AppHandle, moved: bool) -> Option<f64> {
    let drag = VERTICAL_DRAG.lock().unwrap().take()?;
    let ratio = finished_ratio(moved, DRAG_RATIO.lock().unwrap().unwrap_or(drag.start_ratio));
    {
        let state = app.state::<crate::AppState>();
        let mut cfg = state.cfg.lock().unwrap();
        cfg.set_along("right", ratio);
        crate::config::save(&cfg);
    }
    *DRAG_RATIO.lock().unwrap() = None;
    place(app);
    Some(ratio)
}

pub fn start(app: AppHandle) {
    if crate::platform::session_type() == "wayland" { start_gnome(app); }
    else { super::linux_x11::start(app); }
}
fn start_gnome(app: AppHandle) {
    use gtk::gio;
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        diagnostic("session bus unavailable; notch hidden"); return;
    };
    let a = app.clone();
    bus.signal_subscribe(Some(NAME), Some(NAME), Some("Changed"), Some(PATH), None,
        gio::DBusSignalFlags::NONE, move |_, _, _, _, _, args| {
            if let Some((json,)) = args.get::<(String,)>() { decode(&a, &json); }
        });
    let a = app.clone();
    bus.signal_subscribe(Some("org.freedesktop.DBus"), Some("org.freedesktop.DBus"), Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"), Some(NAME), gio::DBusSignalFlags::NONE,
        move |bus, _, _, _, _, args| {
            if let Some((_, _, owner)) = args.get::<(String, String, String)>() {
                update(&a, None);
                if owner.is_empty() { diagnostic("GNOME bridge disconnected; notch hidden"); }
                else { read_bridge(bus, &a); }
            }
        });
    read_bridge(&bus, &app);
    // The session connection and subscriptions are owned by GIO until process exit.
}
fn read_bridge(bus: &gtk::gio::DBusConnection, app: &AppHandle) {
    use gtk::gio;
    let a = app.clone();
    bus.call(Some(NAME), PATH, NAME, "GetState", None, None, gio::DBusCallFlags::NO_AUTO_START, 1500,
        gio::Cancellable::NONE, move |result| {
            if let Ok(value) = result {
                if let Some((json,)) = value.get::<(String,)>() { decode(&a, &json); return; }
            }
            update(&a, None);
            diagnostic("GNOME Window Bridge unavailable; notch hidden. Run linux/bin/codenotch-gnome-bridge install, then log out/in if needed");
        });
}
pub(crate) fn receive_x11(app: &AppHandle, active: Option<Active>) { update(app, active); }
pub(crate) fn failed(message: &str) { diagnostic(message); }

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: i32, y: i32, width: i32, height: i32) -> Rect { Rect{x,y,width,height} }
    fn active(class: &str) -> Active { Active{pid:42, wm_class:class.into(), app_id:String::new(), rect:rect(100,100,900,800), monitor:None, minimized:false} }
    #[test] fn linux_apps_exact_identity() {
        for class in ["jetbrains-studio","antigravity","Chatgpt","jetbrains-idea","jetbrains-pycharm","jetbrains-clion","jetbrains-webstorm","Code","Cursor"] { assert!(supported(&active(class)), "{class}"); }
        for class in ["java","electron","chrome","code-helper","codex"] { assert!(!supported(&active(class)), "{class}"); }
    }
    #[test] fn linux_native_app_id_and_minimized() {
        let mut a = active(""); a.app_id = "code.desktop".into(); assert!(supported(&a));
        a.minimized=true; assert!(!supported(&a));
        a.minimized=false; a.rect.width=0; assert!(!supported(&a));
    }
    #[test] fn linux_active_selection_has_no_previous_ide_fallback() {
        let windows = [active("Code"), active("jetbrains-idea"), active("firefox")];
        for focus in [Some(0), Some(1), Some(2), None] {
            let selected=focus.map(|i| windows[i].clone()).filter(supported);
            assert_eq!(selected.map(|s| s.wm_class), match focus {Some(0)=>Some("Code".into()),Some(1)=>Some("jetbrains-idea".into()),_=>None});
        }
    }
    #[test] fn linux_anchor_tracks_window_and_clamps_monitor() {
        let m=rect(0,0,1920,1080);
        assert_eq!(anchor(rect(100,100,900,800),m,360,650,0.5),Some((640,175)));
        assert_eq!(anchor(rect(100,100,900,800),m,360,650,0.0),Some((640,100)));
        assert_eq!(anchor(rect(100,100,900,800),m,360,650,1.0),Some((640,250)));
        assert_eq!(anchor(rect(300,200,900,800),m,360,650,0.5),Some((840,275)));
        assert_eq!(anchor(rect(0,0,1920,1080),m,360,650,0.5),Some((1560,215)));
        assert_eq!(anchor(rect(1600,900,900,800),m,360,650,0.5),Some((1560,430)));
        assert_eq!(anchor(rect(-1800,100,900,800),rect(-1920,0,1920,1080),360,650,0.5),Some((-1260,175)));
        assert_eq!(anchor(rect(0,0,0,0),m,360,650,0.5),None);
        assert_eq!(anchor(rect(3000,0,800,600),m,360,650,0.5),None);
    }
    #[test] fn linux_drag_ratio_is_relative_and_clamped() {
        assert_eq!(dragged_ratio(0.5, 50.0, 200), 0.75);
        assert_eq!(dragged_ratio(0.5, -800.0, 200), 0.0);
        assert_eq!(dragged_ratio(0.5, 800.0, 200), 1.0);
    }
    #[test] fn linux_click_recentres_and_drag_keeps_position() {
        assert_eq!(finished_ratio(false, 0.8), 0.5);
        assert_eq!(finished_ratio(true, 0.8), 0.8);
    }
}

//! EWMH/X11 event backend. Never used to infer native Wayland focus.
use crate::linux_window::{Active, Rect};
use std::{ffi::{CStr, CString}, ptr};
use tauri::AppHandle;
use x11_dl::xlib;

struct Connection { x: xlib::Xlib, display: *mut xlib::Display, root: u64 }
impl Drop for Connection { fn drop(&mut self) { unsafe { (self.x.XCloseDisplay)(self.display); } } }
impl Connection {
    fn atom(&self, name: &str) -> u64 { unsafe { (self.x.XInternAtom)(self.display, CString::new(name).unwrap().as_ptr(), 0) } }
    fn property(&self, window: u64, name: &str) -> Vec<u64> {
        unsafe {
            let (mut kind, mut format, mut count, mut remaining) = (0,0,0,0);
            let mut data = ptr::null_mut();
            let status=(self.x.XGetWindowProperty)(self.display,window,self.atom(name),0,64,0,xlib::AnyPropertyType as u64,&mut kind,&mut format,&mut count,&mut remaining,&mut data);
            let result=if status==0 && format==32 && !data.is_null() { std::slice::from_raw_parts(data.cast::<u64>(),count as usize).to_vec() } else {Vec::new()};
            if !data.is_null() { (self.x.XFree)(data.cast()); }
            result
        }
    }
    fn focused(&self) -> u64 { self.property(self.root,"_NET_ACTIVE_WINDOW").first().copied().unwrap_or(0) }
    fn snapshot(&self, window: u64) -> Option<Active> {
        if window==0 { return None; }
        unsafe {
            let mut attrs: xlib::XWindowAttributes=std::mem::zeroed();
            if (self.x.XGetWindowAttributes)(self.display,window,&mut attrs)==0 || attrs.map_state!=xlib::IsViewable {return None;}
            if self.property(window,"_NET_WM_STATE").contains(&self.atom("_NET_WM_STATE_HIDDEN")) {return None;}
            let pid=self.property(window,"_NET_WM_PID").first().copied()? as u32;
            let mut hint: xlib::XClassHint=std::mem::zeroed();
            if (self.x.XGetClassHint)(self.display,window,&mut hint)==0 {return None;}
            let class=if hint.res_class.is_null() {String::new()} else {CStr::from_ptr(hint.res_class).to_string_lossy().into_owned()};
            if !hint.res_name.is_null() {(self.x.XFree)(hint.res_name.cast());}
            if !hint.res_class.is_null() {(self.x.XFree)(hint.res_class.cast());}
            let (mut x,mut y,mut child)=(0,0,0);
            if (self.x.XTranslateCoordinates)(self.display,window,self.root,0,0,&mut x,&mut y,&mut child)==0 {return None;}
            let ext=self.property(window,"_NET_FRAME_EXTENTS");
            let (l,r,t,b)=if ext.len()==4 {(ext[0] as i32,ext[1] as i32,ext[2] as i32,ext[3] as i32)} else {(0,0,0,0)};
            Some(Active{pid,wm_class:class,app_id:String::new(),rect:Rect{x:x-l,y:y-t,width:attrs.width+l+r,height:attrs.height+t+b},monitor:None,minimized:false})
        }
    }
}
// A window can disappear between its focus event and a property read. Xlib's default
// handler would terminate the whole app on BadWindow; treat that race as no geometry.
unsafe extern "C" fn window_race(_d: *mut xlib::Display, _event: *mut xlib::XErrorEvent) -> i32 { 0 }
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let Ok(x)=xlib::Xlib::open() else {crate::linux_window::failed("libX11 unavailable; notch hidden");return;};
        unsafe {
            let display=(x.XOpenDisplay)(ptr::null());
            if display.is_null() {crate::linux_window::failed("X11 display unavailable; notch hidden");return;}
            (x.XSetErrorHandler)(Some(window_race));
            let root=(x.XDefaultRootWindow)(display);
            let c=Connection{x,display,root};
            (c.x.XSelectInput)(display,root,xlib::PropertyChangeMask);
            let mut current=0;
            loop {
                let focus=c.focused();
                if current!=focus {
                    if current!=0 {(c.x.XSelectInput)(display,current,0);}
                    current=focus;
                    if current!=0 {(c.x.XSelectInput)(display,current,xlib::StructureNotifyMask | xlib::PropertyChangeMask);}
                }
                let state=c.snapshot(current);
                let a=app.clone();
                let _=app.run_on_main_thread(move || crate::linux_window::receive_x11(&a,state));
                // Block in the X server while idle; no periodic subprocesses or polling.
                loop {
                    let mut event: xlib::XEvent=std::mem::zeroed();
                    (c.x.XNextEvent)(display,&mut event);
                    let relevant=match event.get_type() {
                        xlib::PropertyNotify => {
                            let e=event.property;
                            e.window==current || (e.window==root && ["_NET_ACTIVE_WINDOW","_NET_CURRENT_DESKTOP","_NET_WORKAREA"].iter().any(|a|c.atom(a)==e.atom))
                        },
                        xlib::ConfigureNotify | xlib::DestroyNotify | xlib::UnmapNotify | xlib::MapNotify | xlib::ReparentNotify => true,
                        _ => false,
                    };
                    if relevant {break;}
                }
            }
        }
    });
}

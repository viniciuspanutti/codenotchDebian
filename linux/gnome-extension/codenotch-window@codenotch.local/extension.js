import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const NAME = 'org.codenotch.WindowBridge';
const PATH = '/org/codenotch/WindowBridge';
const XML = `<node><interface name="${NAME}">
  <method name="GetState"><arg type="s" direction="out"/></method>
  <signal name="Changed"><arg type="s"/></signal>
</interface></node>`;
const rect = r => ({x: r.x, y: r.y, width: r.width, height: r.height});

export default class WindowBridge extends Extension {
    enable() {
        this._connections = [];
        this._windows = new Map();
        this._pending = 0;
        this._dbus = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._dbus.export(Gio.DBus.session, PATH);
        this._owner = Gio.bus_own_name_on_connection(Gio.DBus.session, NAME,
            Gio.BusNameOwnerFlags.NONE, null, null);
        const connect = (object, signal, callback) =>
            this._connections.push([object, object.connect(signal, callback)]);
        connect(global.display, 'notify::focus-window', () => this._queue());
        connect(global.display, 'window-created', (_display, window) => {
            this._watch(window);
            this._queue();
        });
        connect(global.workspace_manager, 'active-workspace-changed', () => this._queue());
        connect(Main.layoutManager, 'monitors-changed', () => this._queue());
        connect(Main.overview, 'showing', () => this._queue());
        connect(Main.overview, 'hidden', () => this._queue());
        connect(Main.sessionMode, 'updated', () => this._queue());
        for (const actor of global.get_window_actors()) this._watch(actor.meta_window);
        this._queue();
    }

    _watch(window) {
        if (this._windows.has(window)) return;
        const ids = [];
        for (const signal of ['position-changed', 'size-changed', 'notify::minimized',
            'workspace-changed', 'notify::wm-class', 'notify::gtk-application-id'])
            ids.push(window.connect(signal, () => this._queue()));
        ids.push(window.connect('unmanaged', () => {
            for (const id of this._windows.get(window) ?? []) window.disconnect(id);
            this._windows.delete(window);
            this._queue();
        }));
        this._windows.set(window, ids);
    }

    GetState() {
        const window = global.display.focus_window;
        let active = null;
        if (window && !window.minimized && window.showing_on_its_workspace() &&
            !Main.overview.visible && !Main.sessionMode.isLocked) {
            const monitor = Main.layoutManager.monitors[window.get_monitor()];
            if (monitor) active = {
                pid: window.get_pid(),
                wm_class: window.get_wm_class() ?? '',
                app_id: window.get_gtk_application_id() ?? '',
                backend: window.get_client_type() === Meta.WindowClientType.WAYLAND ? 'wayland' : 'x11',
                rect: rect(window.get_frame_rect()),
                monitor: rect(window.is_fullscreen() ? monitor :
                    window.get_work_area_for_monitor(window.get_monitor())),
                minimized: false,
            };
        }
        return JSON.stringify({version: 1, active});
    }

    _queue() {
        if (this._pending) return;
        // Coalesce a frame's move/resize signals; zero timers while idle.
        this._pending = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => {
            this._pending = 0;
            const state = this.GetState();
            if (state !== this._last) {
                this._last = state;
                this._dbus.emit_signal('Changed', new GLib.Variant('(s)', [state]));
            }
            return GLib.SOURCE_REMOVE;
        });
    }

    disable() {
        if (this._pending) GLib.source_remove(this._pending);
        this._pending = 0;
        for (const [object, id] of this._connections) object.disconnect(id);
        for (const [window, ids] of this._windows)
            for (const id of ids) window.disconnect(id);
        this._windows.clear();
        this._connections = [];
        this._dbus.unexport();
        Gio.bus_unown_name(this._owner);
        this._dbus = null;
        this._last = null;
    }
}

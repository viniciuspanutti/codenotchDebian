"""Shared process catalogue. Never identify an IDE by generic JVM/Electron names."""
import json
import os
import re
from pathlib import Path

CATALOGUE = Path(__file__).resolve().parent.parent / "development-apps.json"
DEFAULT_IDE_CONFIG = {"interval_seconds": 3, "ides": json.loads(CATALOGUE.read_text())}


def merge_config(data):
    # Old three-IDE configs acquire the new built-ins; custom apps remain supported.
    if data.get("replace_defaults", False):
        return data
    apps = {a["name"]: dict(a) for a in DEFAULT_IDE_CONFIG["ides"]}
    for entry in data.get("ides", []):
        apps[entry["name"]] = {**apps.get(entry["name"], {}), **entry}
    return {**DEFAULT_IDE_CONFIG, **data, "ides": list(apps.values())}


def identify_process(exe, args, apps):
    # Renderers, crash handlers, language servers and CLI Codex are not GUI hosts.
    if any(a.startswith("--type=") for a in args):
        return None
    base = Path(exe.removesuffix(" (deleted)")).name
    for app in apps:
        if base not in {"java", "electron", "chrome", "node", "codex"}:
            if exe in app.get("executables", []) or base in app.get("process_names", []):
                return app["name"]
        if base == "java":
            selector = next((a.split("=", 1)[1] for a in args if a.startswith("-Didea.paths.selector=")), "")
            if any(re.fullmatch(re.escape(s) + r"[0-9].*", selector)
                   for s in app.get("java_selectors", [])) and any(
                       a == "com.intellij.idea.Main" or a == "com.android.tools.idea.MainWrapper"
                       for a in args):
                return app["name"]
    return None


def find_running_ides(apps, proc_root=Path("/proc")):
    found = set()
    for p in proc_root.iterdir():
        if not p.name.isdigit():
            continue
        try:
            if p.stat().st_uid != os.getuid():
                continue
            exe = os.readlink(p / "exe")
            args = (p / "cmdline").read_bytes().decode(errors="replace").split("\0")
            app = identify_process(exe, args, apps)
            if app:
                found.add(app)
        except (OSError, ValueError):
            continue
    return sorted(found)


def watcher_action(running, state, has_notch):
    """Open processes decide lifetime only. Focus never starts a process."""
    if not running:
        return "stop" if has_notch else "idle"
    if state.get("auto_enabled", True) is False or state.get("paused", False):
        return "idle"
    if not (set(running) - set(state.get("stopped_ides", []))):
        return "idle"
    return "idle" if has_notch else "start"

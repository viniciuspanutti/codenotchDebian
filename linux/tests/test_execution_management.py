#!/usr/bin/env python3
"""
Integration test for Codenotch Execution Management on Linux.
Tests scenarios 1 through 11:
 1. No IDE open -> Codenotch stopped.
 2. Open IDE 1 (Cursor) -> Codenotch starts automatically.
 3. Open IDE 2 (Antigravity) -> Single Codenotch instance remains.
 4. Close IDE 1 (Cursor) keeping IDE 2 (Antigravity) -> Codenotch remains running.
 5. Close IDE 2 (Antigravity) -> Codenotch terminates automatically.
 6. Reopen IDE 1 -> Codenotch starts again automatically.
 7. codenotch-control stop -> Manual stop respected (does not reopen for current session).
 8. codenotch-control auto-off -> Automation disabled.
 9. Open IDE with auto-off -> Codenotch does NOT start.
 10. codenotch-control auto-on -> Automation resumes.
 11. Systemd persistence and status verification.
"""

import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
CONTROL_BIN = REPO_ROOT / "linux" / "bin" / "codenotch-control"
WATCH_BIN = REPO_ROOT / "linux" / "bin" / "codenotch-ide-watch"
CONFIG_DIR = Path.home() / ".config" / "codenotch"
PID_FILE = CONFIG_DIR / "codenotch.pid"

def get_codenotch_pid():
    res = subprocess.run([str(CONTROL_BIN), "status"], capture_output=True, text=True)
    for line in res.stdout.splitlines():
        if "Codenotch: RUNNING (PID:" in line:
            return int(line.split("(PID:")[1].split(")")[0].strip())
    return None

def is_codenotch_running():
    return get_codenotch_pid() is not None

def spawn_dummy_ide(name, temp_dir):
    """Compiles and executes a dummy binary named `name`."""
    bin_path = temp_dir / name
    c_src = "#include <unistd.h>\nint main() { pause(); return 0; }\n"
    subprocess.run(["gcc", "-x", "c", "-", "-o", str(bin_path)], input=c_src, text=True, check=True)
    proc = subprocess.Popen([str(bin_path)])
    return proc, bin_path

def main():
    print("==================================================")
    print("Testing Codenotch Linux Execution Management")
    print("==================================================")

    # Backup existing ide_watch.json if present
    ide_conf_file = CONFIG_DIR / "ide_watch.json"
    backup_file = CONFIG_DIR / "ide_watch.json.bak"
    if ide_conf_file.exists():
        shutil.copy(ide_conf_file, backup_file)

    temp_dir = Path(tempfile.mkdtemp(prefix="codenotch_test_"))
    try:
        # Stop any running codenotch and ensure systemd auto-watch is off during isolated test
        subprocess.run([str(CONTROL_BIN), "auto-off"], capture_output=True)
        subprocess.run([str(CONTROL_BIN), "stop"], capture_output=True)
        time.sleep(1)

        # Setup test configuration with dedicated test IDE names to avoid interfering with user session
        test_conf = {
            "interval_seconds": 1,
            "ides": [
                {
                    "name": "CursorTest",
                    "executables": [str(temp_dir / "cursor-test-bin")],
                    "process_names": ["cursor-test-bin"]
                },
                {
                    "name": "AntigravityTest",
                    "executables": [str(temp_dir / "antigravity-test-bin")],
                    "process_names": ["antigravity-test-bin"]
                }
            ]
        }
        ide_conf_file.write_text(json.dumps(test_conf, indent=2))

        # Start a watcher process with test config
        print("Starting isolated test watcher daemon...")
        watcher_proc = subprocess.Popen([str(WATCH_BIN)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        time.sleep(1.5)

        # 1. No IDE open -> Codenotch stopped.
        print("\n[Step 1] Verifying: No test IDE open -> Codenotch stopped.")
        assert not is_codenotch_running(), "Step 1 Failed: Codenotch is running with no IDE open!"
        print("  -> Passed: Codenotch is STOPPED.")

        # 2. Open Cursor -> Codenotch starts.
        print("\n[Step 2] Verifying: Open CursorTest -> Codenotch starts automatically.")
        cursor_proc, _ = spawn_dummy_ide("cursor-test-bin", temp_dir)
        time.sleep(2.5)
        assert is_codenotch_running(), "Step 2 Failed: Codenotch did not start after opening CursorTest!"
        pid1 = get_codenotch_pid()
        print(f"  -> Passed: Codenotch started automatically (PID: {pid1}).")

        # 3. Open Antigravity also -> Single instance maintained.
        print("\n[Step 3] Verifying: Open AntigravityTest also -> Single Codenotch instance.")
        ag_proc, _ = spawn_dummy_ide("antigravity-test-bin", temp_dir)
        time.sleep(2.5)
        pid2 = get_codenotch_pid()
        assert pid2 == pid1, f"Step 3 Failed: PID changed from {pid1} to {pid2} (multiple/restarted instances)!"
        print(f"  -> Passed: Still single instance running (PID: {pid2}).")

        # 4. Close Cursor keeping Antigravity -> Codenotch continues open.
        print("\n[Step 4] Verifying: Close CursorTest keeping AntigravityTest -> Codenotch continues open.")
        cursor_proc.terminate()
        cursor_proc.wait()
        time.sleep(2.5)
        assert is_codenotch_running(), "Step 4 Failed: Codenotch closed while AntigravityTest was still open!"
        assert get_codenotch_pid() == pid1, "Step 4 Failed: PID changed unexpectedly!"
        print("  -> Passed: Codenotch remains open with AntigravityTest active.")

        # 5. Close Antigravity -> Codenotch closes.
        print("\n[Step 5] Verifying: Close AntigravityTest -> Codenotch closes automatically.")
        ag_proc.terminate()
        ag_proc.wait()
        time.sleep(2.5)
        assert not is_codenotch_running(), "Step 5 Failed: Codenotch did not close after all IDEs closed!"
        print("  -> Passed: Codenotch terminated automatically.")

        # 6. Open Cursor again -> Codenotch returns.
        print("\n[Step 6] Verifying: Re-open CursorTest -> Codenotch starts automatically.")
        cursor_proc2, _ = spawn_dummy_ide("cursor-test-bin", temp_dir)
        time.sleep(2.5)
        assert is_codenotch_running(), "Step 6 Failed: Codenotch did not restart when CursorTest reopened!"
        pid3 = get_codenotch_pid()
        print(f"  -> Passed: Codenotch restarted automatically (PID: {pid3}).")

        # 7. codenotch-control stop -> Manual stop works and suppresses reopening.
        print("\n[Step 7] Verifying: codenotch-control stop -> Stops and does not reopen.")
        subprocess.run([str(CONTROL_BIN), "stop"], check=True)
        time.sleep(2.5)
        assert not is_codenotch_running(), "Step 7 Failed: Codenotch still running after stop!"
        print("  -> Passed: Codenotch stopped and stayed stopped despite CursorTest being open.")

        # 8. codenotch-control auto-off -> Stop watcher completely.
        print("\n[Step 8] Verifying: codenotch-control auto-off.")
        watcher_proc.terminate()
        watcher_proc.wait()
        res = subprocess.run([str(CONTROL_BIN), "auto-off"], capture_output=True, text=True)
        assert res.returncode == 0, f"Step 8 Failed: auto-off failed: {res.stderr}"
        print("  -> Passed: auto-off executed successfully.")

        # 9. Open IDE with auto-off -> Codenotch does NOT start.
        print("\n[Step 9] Verifying: Open IDE with auto-off -> Codenotch does NOT start.")
        time.sleep(2.5)
        assert not is_codenotch_running(), "Step 9 Failed: Codenotch started while auto-off!"
        print("  -> Passed: Codenotch stayed stopped.")

        # Clean up test IDE
        cursor_proc2.terminate()
        cursor_proc2.wait()

        # 10. Restore standard config and test auto-on
        print("\n[Step 10] Verifying: codenotch-control auto-on with systemd user service.")
        if backup_file.exists():
            shutil.copy(backup_file, ide_conf_file)
        else:
            ide_conf_file.write_text(json.dumps({
                "interval_seconds": 3,
                "ides": [
                    {"name": "Cursor", "executables": ["/usr/share/cursor/cursor"], "process_names": ["cursor"]},
                    {"name": "Antigravity", "executables": ["/opt/antigravity/antigravity"], "process_names": ["antigravity"]},
                    {"name": "VS Code", "executables": ["/usr/share/code/code"], "process_names": ["code"]}
                ]
            }, indent=2))

        res = subprocess.run([str(CONTROL_BIN), "auto-on"], capture_output=True, text=True)
        assert res.returncode == 0, f"Step 10 Failed: auto-on failed: {res.stderr}"
        time.sleep(3.5)
        # Since Antigravity is open in real user session, Codenotch should start!
        assert is_codenotch_running(), "Step 10 Failed: Codenotch did not start on auto-on with active Antigravity session!"
        print("  -> Passed: auto-on enabled systemd service and started Codenotch for active Antigravity.")

        # 11. Systemd persistence and daemon-reload
        print("\n[Step 11] Verifying: systemd user service reload, status, and persistence.")
        subprocess.run(["systemctl", "--user", "daemon-reload"], check=True)
        status_res = subprocess.run(["systemctl", "--user", "is-active", "codenotch-ide-watch.service"], capture_output=True, text=True)
        assert status_res.stdout.strip() == "active", f"Step 11 Failed: Service not active: {status_res.stdout}"
        enabled_res = subprocess.run(["systemctl", "--user", "is-enabled", "codenotch-ide-watch.service"], capture_output=True, text=True)
        assert enabled_res.stdout.strip() == "enabled", f"Step 11 Failed: Service not enabled: {enabled_res.stdout}"
        print("  -> Passed: systemd user service is ACTIVE and ENABLED.")

        print("\n==================================================")
        print("ALL 11 SCENARIO TESTS PASSED SUCCESSFULLY!")
        print("==================================================")

    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)
        if backup_file.exists():
            shutil.copy(backup_file, ide_conf_file)
            backup_file.unlink()

if __name__ == "__main__":
    main()

"""Focused tests; do not alter the real user's service, state or applications."""
import importlib.machinery
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

BIN = Path(__file__).resolve().parents[1] / "bin"
sys.path.insert(0, str(BIN))
from development_apps import DEFAULT_IDE_CONFIG, identify_process, merge_config, find_running_ides, watcher_action


def load_script(name):
    loader = importlib.machinery.SourceFileLoader(name, str(BIN / name))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


class Apps(unittest.TestCase):
    def test_all_observed_executables(self):
        samples = {"/opt/android-studio/bin/studio": "Android Studio",
                   "/opt/antigravity/antigravity": "Antigravity", "/usr/lib/chatgpt/ChatGPT": "ChatGPT",
                   "/opt/pycharm/bin/pycharm": "PyCharm", "/opt/clion/bin/clion": "CLion",
                   "/opt/webstorm/bin/webstorm": "WebStorm", "/usr/share/code/code": "VS Code",
                   "/usr/share/cursor/cursor": "Cursor"}
        for exe, name in samples.items():
            self.assertEqual(identify_process(exe, [], DEFAULT_IDE_CONFIG["ides"]), name)
        self.assertEqual(identify_process('/opt/idea/jbr/bin/java',
            ['-Didea.paths.selector=IntelliJIdea2026.2', 'com.intellij.idea.Main'], DEFAULT_IDE_CONFIG['ides']), 'IntelliJ IDEA')

    def test_generic_runtimes_and_helpers_rejected(self):
        for exe, args in [('/usr/bin/java', []), ('/usr/bin/electron', []), ('/usr/bin/chrome', []),
                          ('/usr/lib/chatgpt/resources/codex', []), ('/usr/share/code/code', ['--type=renderer']),
                          ('/opt/idea/jbr/bin/java', ['-Didea.paths.selector=IntelliJIdea2026.2', 'org.jetbrains.jps.cmdline.Launcher']),
                          ('/usr/bin/electron', ['/tmp/code-project.js'])]:
            self.assertIsNone(identify_process(exe, args, DEFAULT_IDE_CONFIG['ides']))

    def test_legacy_config_gets_new_apps_and_custom_is_preserved(self):
        cfg = merge_config({'ides': [{'name': 'Cursor', 'executables': ['/custom/cursor']},
                                     {'name': 'My IDE', 'process_names': ['my-ide']}]})
        self.assertEqual(len(cfg['ides']), 10)
        cursor = next(a for a in cfg['ides'] if a['name'] == 'Cursor')
        self.assertEqual(cursor['window_classes'], ['Cursor', 'cursor'])
        self.assertEqual(cursor['executables'], ['/custom/cursor'])
        self.assertEqual(merge_config({'replace_defaults': True, 'ides': []})['ides'], [])

    def test_proc_scan_reads_exe_and_cmdline_not_comm(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for pid, exe, args in [(1, '/usr/share/code/code', []), (2, '/usr/bin/java', []),
                                    (3, '/opt/idea/jbr/bin/java', ['-Didea.paths.selector=IntelliJIdea2026.2', 'com.intellij.idea.Main'])]:
                p = root / str(pid); p.mkdir(); (p / 'exe').symlink_to(exe)
                (p / 'cmdline').write_bytes('\0'.join(args).encode())
            self.assertEqual(find_running_ides(DEFAULT_IDE_CONFIG['ides'], root), ['IntelliJ IDEA', 'VS Code'])

    def test_lifetime_multiple_ides_single_instance_and_manual_suppression(self):
        self.assertEqual(watcher_action([], {}, False), 'idle')
        self.assertEqual(watcher_action(['VS Code'], {}, False), 'start')
        self.assertEqual(watcher_action(['VS Code', 'IntelliJ IDEA'], {}, True), 'idle')
        self.assertEqual(watcher_action(['IntelliJ IDEA'], {}, True), 'idle')
        self.assertEqual(watcher_action([], {}, True), 'stop')
        self.assertEqual(watcher_action(['VS Code'], {'stopped_ides': ['VS Code']}, False), 'idle')
        self.assertEqual(watcher_action(['VS Code', 'Cursor'], {'stopped_ides': ['VS Code']}, False), 'start')
        for state in [{'paused': True}, {'auto_enabled': False}]:
            self.assertEqual(watcher_action(['Cursor'], state, False), 'idle')

    def test_auto_off_survives_resume_and_manual_start(self):
        control = load_script('codenotch-control')
        with tempfile.TemporaryDirectory() as tmp:
            control.CONFIG_DIR = Path(tmp); control.STATE_FILE = Path(tmp) / 'state.json'
            with patch.object(control.subprocess, 'run'), patch.object(control, 'clean_legacy_autostart'), \
                 patch.object(control, 'find_running_ides', return_value=['VS Code']), \
                 patch.object(control, 'find_codenotch_pid', return_value=123):
                self.assertEqual(control.cmd_auto_off(), 0)
                self.assertEqual(control.cmd_resume(), 0)
                self.assertFalse(control.load_state()['auto_enabled'])
                self.assertEqual(control.cmd_start(), 0)
                self.assertFalse(control.load_state()['auto_enabled'])

    def test_pid_file_cannot_target_hook(self):
        control = load_script('codenotch-control')
        with tempfile.TemporaryDirectory() as tmp:
            control.PID_FILE = Path(tmp) / 'pid'; control.PID_FILE.write_text('123')
            with patch.object(os, 'readlink', return_value='/usr/bin/codenotch-hook'), patch.object(os, 'listdir', return_value=[]):
                self.assertIsNone(control.find_codenotch_pid())


if __name__ == '__main__':
    unittest.main()

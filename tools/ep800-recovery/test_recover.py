"""Controller tests use fake tools only; no adaptor or firmware is needed."""
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import recover


class ControllerTests(unittest.TestCase):
    def test_windows_binary_suffix(self):
        with patch.object(recover.sys, "platform", "win32"):
            self.assertEqual(recover.binary_path(Path("tools"), "probe"), Path("tools/probe.exe"))
        with patch.object(recover.sys, "platform", "darwin"):
            self.assertEqual(recover.binary_path(Path("tools"), "probe"), Path("tools/probe"))

    def run_controller(self, summaries, restore_code=0, execute=True):
        self.commands = []
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            data = root / "stock"
            data.write_bytes(b"synthetic input")
            for name in ("sustained_key_epochs", "restore_sustained_boot"):
                (root / name).touch()
            args = ["recover.py", "--d", str(data), "--m", str(data),
                    "--serial", "010203040506", "--output", str(root / "evidence"),
                    "--bin-dir", str(root), "--countdown", "0", "--attempts", "3"]
            if execute:
                args.append("--execute")

            def probe(command, stdout, stderr):
                self.commands.append(command)
                summary = summaries.pop(0)
                stdout.write(json.dumps(summary))
                return SimpleNamespace(returncode=0)

            def restore(command, **kwargs):
                self.commands.append(command)
                return SimpleNamespace(stdout=iter(["finish result\n"]), wait=lambda: restore_code)

            digest = hashlib.sha256(data.read_bytes()).hexdigest()
            with patch.object(sys, "argv", args), patch.object(recover, "D_SHA", digest), \
                 patch.object(recover, "M_SHA", digest), patch.object(recover.subprocess, "run", probe), \
                 patch.object(recover.subprocess, "Popen", restore), patch.object(recover.time, "sleep"), \
                 patch("sys.stdout", new_callable=io.StringIO):
                return recover.main()

    def test_preflight_never_opens_hardware(self):
        self.assertEqual(self.run_controller([], execute=False), 0)
        self.assertEqual(self.commands, [])

    def test_keys_only_retry_then_one_restore(self):
        self.assertEqual(self.run_controller([{}, {"full_handshake": True}]), 0)
        self.assertEqual(len(self.commands), 3)
        self.assertIn("restore_sustained_boot", self.commands[-1][0])
        self.assertEqual(self.commands[-1][-1], "010203040506")

    def test_restore_failure_is_never_retried(self):
        self.assertEqual(self.run_controller([{"full_handshake": True}], restore_code=9), 9)
        self.assertEqual(len(self.commands), 2)

    def test_exhausted_keys_do_not_restore(self):
        self.assertEqual(self.run_controller([{}, {}, {}]), 1)
        self.assertTrue(all("sustained_key_epochs" in c[0] for c in self.commands))

    def test_modified_firmware_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "bad.dat"
            path.write_bytes(b"changed")
            with self.assertRaises(ValueError):
                recover.check_file(path, recover.D_SHA)


if __name__ == "__main__":
    unittest.main()

"""CLI integration and protection of firmware/state inputs from report writes."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import sim


class SimulatorCliTests(unittest.TestCase):
    def invoke(self, *args):
        return subprocess.run([sys.executable, str(Path(sim.__file__)), *map(str,args)],
                              capture_output=True,text=True)

    def test_upstream_and_mn101_commands_remain_registered(self):
        result=self.invoke('--help')
        self.assertEqual(result.returncode,0,result.stderr)
        for command in ('pull','run','serve','resident','cpu-probe','region-storage'):
            self.assertIn(command,result.stdout)

    def test_reports_cannot_alias_images_or_storage(self):
        for command in ('resident','cpu-probe','region-storage'):
            for source_name in (('image',) if command=='resident' else ('motor',) if command=='cpu-probe' else ('motor','state')):
                for alias_kind in ('direct','symlink','hardlink'):
                    with self.subTest(command=command,source=source_name,alias=alias_kind),tempfile.TemporaryDirectory() as tmp:
                        root=Path(tmp)
                        motor,state=root/'motor.bin',root/'state.bin'
                        motor.write_bytes(b'synthetic image bytes')
                        state.write_bytes(b'synthetic persistent bytes')
                        source=motor if source_name in ('image','motor') else state
                        output=source if alias_kind=='direct' else root/'report.json'
                        if alias_kind=='symlink': output.symlink_to(source)
                        if alias_kind=='hardlink': output.hardlink_to(source)
                        args=[command,'--image' if command=='resident' else '--motor',motor,'--output',output]
                        if command=='region-storage': args+=['--state',state,'--region','1']
                        result=self.invoke(*args)
                        self.assertEqual(result.returncode,2,result.stderr)
                        self.assertIn(f'--output must not refer to the {source_name} file',result.stderr)
                        self.assertEqual(motor.read_bytes(),b'synthetic image bytes')
                        self.assertEqual(state.read_bytes(),b'synthetic persistent bytes')

    def test_absent_storage_report_alias_is_rejected_before_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            state=root/'state.bin'
            result=self.invoke('region-storage','--motor',root/'motor.bin',
                               '--state',state,'--region',1,'--output',root/'sub'/ '..'/'state.bin')
            self.assertEqual(result.returncode,2,result.stderr)
            self.assertIn('--output must not refer to the state file',result.stderr)
            self.assertFalse(state.exists())

    def test_invalid_report_parent_is_detected_before_storage_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            args=['sim.py','region-storage','--motor',str(root/'motor.bin'),
                  '--state',str(root/'state.bin'),'--region','1',
                  '--output',str(root/'missing'/'report.json')]
            with patch.object(sys,'argv',args),patch('fw_sim.mn101_storage.verify_region') as verify:
                with self.assertRaises(FileNotFoundError): sim.main()
                verify.assert_not_called()
            self.assertFalse((root/'state.bin').exists())


if __name__=='__main__':
    unittest.main()

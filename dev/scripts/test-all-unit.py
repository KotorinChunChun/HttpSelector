"""一括試験の失敗判定・対応表・タイムアウトを検証する。"""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location('runner', Path(__file__).with_name('test-all.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_rust_results_keep_ignored_and_failed_separate_from_passes(self):
        parsed = runner.rust_cases('test a::one ... ok\ntest a::two ... FAILED\ntest a::three ... ignored, requires network\n')
        self.assertEqual([case['status'] for case in parsed], ['passed', 'failed', 'skipped'])

    def test_case_map_rejects_deleted_or_renamed_tests(self):
        with self.assertRaises(ValueError):
            runner.check_case_map([{'id': 'URL', 'test_prefixes': ['missing::']}], {'actual::test'})
        matched = runner.check_case_map([{'id': 'URL', 'test_prefixes': ['actual::']}], {'actual::test'})
        self.assertEqual(matched[0]['resolved_tests'], ['actual::test'])

    def test_junit_cannot_hide_a_process_failure_behind_successful_cases(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'junit.xml'
            runner.write_junit([{'name': 'rust', 'status': 'failed', 'cases': [{'name': 'ok', 'status': 'passed'}]}], path)
            root = ET.parse(path).getroot()
            self.assertEqual(len(root.findall('.//failure')), 1)

    def test_command_failure_is_reported_and_log_is_kept(self):
        with tempfile.TemporaryDirectory() as folder:
            result, output = runner.execute('failed', [sys.executable, '-c', 'print("failure evidence"); raise SystemExit(7)'], Path(folder), 5)
            self.assertEqual(result['status'], 'failed')
            self.assertEqual(result['exit_code'], 7)
            self.assertIn('failure evidence', output)

    def test_timeout_kills_only_the_started_process_and_is_a_failure(self):
        with tempfile.TemporaryDirectory() as folder:
            result, _ = runner.execute('timeout', [sys.executable, '-c', 'import time; time.sleep(60)'], Path(folder), 0.05)
            self.assertTrue(result['timed_out'])
            self.assertEqual(result['status'], 'failed')


if __name__ == '__main__':
    unittest.main()

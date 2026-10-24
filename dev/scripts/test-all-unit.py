"""一括試験の失敗判定・対応表・タイムアウトを検証する。"""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from types import SimpleNamespace
import xml.etree.ElementTree as ET
sys.dont_write_bytecode = True
import cli_test_report as report

spec = importlib.util.spec_from_file_location('runner', Path(__file__).with_name('test-all.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_plan_preserves_order_and_requires_explicit_external_options(self):
        args = SimpleNamespace(cargo='cargo', pwsh='pwsh', signed_package=None, release_build=False,
                               live_quad9=False, interactive_desktop=False)
        plan = runner.build_test_plan(args)
        self.assertEqual([step.name for step in plan], ['fmt', 'rust-list', 'rust', 'clippy', 'runner-unit',
                         'ftps-plan', 'signing-policy', 'msix-scripts', 'release-build', 'quad9-live', 'desktop-live'])
        self.assertTrue(all(step.enabled for step in plan[:8]))
        self.assertTrue(all(not step.enabled for step in plan[8:]))
        args.interactive_desktop = True
        self.assertTrue(runner.build_test_plan(args)[-1].enabled)

    def test_orchestration_uses_inventory_for_optional_cases_and_keeps_skips(self):
        names = [f'suite::interactive_desktop_{i}' for i in range(3)]
        calls = []
        def fake_execute(name, command, directory, timeout, env):
            calls.append(name)
            output = '\n'.join(f'{case}: test' for case in names) if name == 'list' else '\n'.join(f'test {case} ... ok' for case in names)
            return {'name': name, 'status': 'passed', 'cases': report.rust_cases(output)}, output
        run = runner.TestRun(SimpleNamespace(timeout=1, signed_package=None), Path('.'), {},
                             executor=fake_execute, features=[{'id': 'UI', 'test_prefixes': ['suite::']}])
        self.assertFalse(run.report('0.0.3.8')['passed'])
        run.run([runner.TestCommand('list', [], kind='inventory'),
                 runner.TestCommand('desktop', [], kind='rust', selection='interactive_desktop_'),
                 runner.TestCommand('network', [], enabled=False)])
        result = run.report('0.0.3.8')
        self.assertTrue(result['passed'])
        self.assertEqual(calls, ['list', 'desktop'])
        self.assertEqual(result['steps'][-1]['status'], 'skipped')
        self.assertEqual(len(result['steps'][-2]['cases']), 3)

    def test_case_validation_rejects_missing_duplicate_and_empty_results(self):
        good = {'status': 'passed', 'cases': [{'name': 'a', 'status': 'passed'}, {'name': 'b', 'status': 'passed'}]}
        self.assertEqual(report.validate_rust_result(good, ['a', 'b'])['status'], 'passed')
        for names in [[], ['a'], ['a', 'a', 'b']]:
            self.assertEqual(report.validate_rust_result(good, names)['status'], 'failed')
        duplicate = {**good, 'cases': good['cases'] + [good['cases'][0]]}
        self.assertEqual(report.validate_rust_result(duplicate, ['a', 'b'])['status'], 'failed')

    def test_explicit_tests_cannot_be_skipped_and_process_failure_is_preserved(self):
        skipped = {'status': 'passed', 'cases': [{'name': 'a', 'status': 'skipped'}]}
        self.assertEqual(report.validate_rust_result(skipped, ['a'])['status'], 'passed')
        self.assertEqual(report.validate_rust_result(skipped, ['a'], require_success=True)['status'], 'failed')
        failed_process = {'status': 'failed', 'cases': [{'name': 'a', 'status': 'passed'}]}
        self.assertEqual(report.validate_rust_result(failed_process, ['a'])['status'], 'failed')
        failed_case = {'status': 'passed', 'cases': [{'name': 'a', 'status': 'failed'}]}
        self.assertEqual(report.validate_rust_result(failed_case, ['a'])['status'], 'failed')

    def test_optional_cases_are_selected_from_inventory_without_a_fixed_count(self):
        names = report.rust_inventory('a::interactive_desktop_one: test\na::interactive_desktop_two: test\na::interactive_desktop_three: test\nquad9::live: test\n')
        self.assertEqual(len(report.selected_cases(names, 'interactive_desktop_')), 3)
        self.assertEqual(report.selected_cases(names, 'quad9::live', exact=True), ['quad9::live'])
        self.assertEqual(report.selected_cases(names, 'quad9', exact=True), [])

    def test_json_and_junit_reports_preserve_schema_and_unicode(self):
        with tempfile.TemporaryDirectory() as folder:
            directory = Path(folder)
            original = {'version': '0.0.3.8', 'passed': True, 'steps': [{'name': '通常試験', 'status': 'passed'}],
                        'features': [{'id': 'F01', 'feature': 'URL受付'}], 'external_boundaries': [], 'signed_package': None}
            report.write_reports(original, directory)
            import json
            self.assertEqual(json.loads((directory / 'results.json').read_text(encoding='utf-8')), original)
            self.assertEqual(json.loads((directory / 'cases.json').read_text(encoding='utf-8')), original['features'])
            self.assertEqual(ET.parse(directory / 'junit.xml').getroot().find('testsuite').get('name'), '通常試験')

    def test_rust_results_keep_ignored_and_failed_separate_from_passes(self):
        parsed = report.rust_cases('test a::one ... ok\ntest a::two ... FAILED\ntest a::three ... ignored, requires network\n')
        self.assertEqual([case['status'] for case in parsed], ['passed', 'failed', 'skipped'])

    def test_case_map_rejects_deleted_or_renamed_tests(self):
        with self.assertRaises(ValueError):
            report.check_case_map([{'id': 'URL', 'test_prefixes': ['missing::']}], {'actual::test'})
        matched = report.check_case_map([{'id': 'URL', 'test_prefixes': ['actual::']}], {'actual::test'})
        self.assertEqual(matched[0]['resolved_tests'], ['actual::test'])

    def test_junit_cannot_hide_a_process_failure_behind_successful_cases(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'junit.xml'
            report.write_junit([{'name': 'rust', 'status': 'failed', 'cases': [{'name': 'ok', 'status': 'passed'}]}], path)
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

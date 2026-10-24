"""CLI試験のケース照合とレポート。プロセス起動や環境変数に依存しない。"""
from collections import Counter
import json
import re
import xml.etree.ElementTree as ET

RESULT_LINE = re.compile(r'^test (\S+) \.\.\. (ok|FAILED|ignored)(?:,.*)?$', re.MULTILINE)
INVENTORY_LINE = re.compile(r'^(\S+): test$', re.MULTILINE)


def rust_cases(output):
    statuses = {'ok': 'passed', 'FAILED': 'failed', 'ignored': 'skipped'}
    return [{'name': name, 'status': statuses[result]} for name, result in RESULT_LINE.findall(output)]


def rust_inventory(output):
    return INVENTORY_LINE.findall(output)


def selected_cases(names, pattern, exact=False):
    return [name for name in names if name == pattern or (not exact and pattern in name)]


def validate_rust_result(result, expected, require_success=False):
    """実行件数も照合する。ケース成功でプロセス失敗を上書きしない。"""
    cases = result.get('cases', [])
    reason = None
    if not expected or Counter(case['name'] for case in cases) != Counter(expected):
        reason = '一覧と実行されたRust試験が一致しません'
    elif any(case['status'] == 'failed' for case in cases):
        reason = '失敗したRust試験があります'
    elif require_success and any(case['status'] != 'passed' for case in cases):
        reason = '明示実行した対象試験が成功していません'
    return {**result, 'status': 'failed', 'reason': reason} if reason else dict(result)


def check_case_map(features, names):
    """削除・改名されて対応表だけが残った場合は失敗する。"""
    resolved = []
    for feature in features:
        matches = []
        for prefix in feature.get('test_prefixes', []):
            found = sorted(name for name in names if name.startswith(prefix))
            if not found:
                raise ValueError(f"{feature['id']}: 実在しないテスト指定 {prefix}")
            matches.extend(found)
        resolved.append({**feature, 'resolved_tests': sorted(set(matches))})
    return resolved


def write_junit(steps, path):
    root = ET.Element('testsuites')
    for step in steps:
        cases = list(step.get('cases', []))
        # ケース成功とプロセス成功を区別し、コマンド失敗を隠さない。
        cases.append({'name': step['name'], 'status': step['status']})
        suite = ET.SubElement(root, 'testsuite', name=step['name'], tests=str(len(cases)),
                              failures=str(sum(case['status'] == 'failed' for case in cases)),
                              skipped=str(sum(case['status'] == 'skipped' for case in cases)))
        for case in cases:
            node = ET.SubElement(suite, 'testcase', name=case['name'], classname=step['name'])
            if case['status'] == 'failed':
                ET.SubElement(node, 'failure', message=step.get('reason', '試験失敗')).text = step.get('log', '')
            if case['status'] == 'skipped':
                ET.SubElement(node, 'skipped', message=step.get('reason', '明示実行が必要'))
    ET.ElementTree(root).write(path, encoding='utf-8', xml_declaration=True)


def write_reports(report, directory):
    for name, value in [('results.json', report), ('cases.json', report['features'])]:
        (directory / name).write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding='utf-8')
    write_junit(report['steps'], directory / 'junit.xml')

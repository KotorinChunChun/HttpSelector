"""HttpSelectorのCLI試験。実利用者設定・OS関連付け・信頼ストアの変更なし。"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import uuid
from cli_test_report import check_case_map, rust_cases, write_junit

ROOT = Path(__file__).resolve().parents[2]
def execute(name, command, directory, timeout, env=None):
    started = time.monotonic()
    log = directory / f'{name}.log'
    print(f'実行: {name}', flush=True)
    code, timed_out = -1, False
    with log.open('w', encoding='utf-8') as stream:
        try:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT,
                                       creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == 'nt' else 0)
            try:
                code = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                try:
                    if os.name == 'nt':
                        subprocess.run(['taskkill', '/PID', str(process.pid), '/T', '/F'], stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL, timeout=15, check=False)
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.wait(timeout=15)
        except (OSError, subprocess.SubprocessError) as error:
            stream.write(f'実行失敗: {error}\n')
    output = log.read_text(encoding='utf-8', errors='replace')
    status = 'passed' if code == 0 and not timed_out else 'failed'
    result = {'name': name, 'status': status, 'exit_code': code, 'timed_out': timed_out,
              'seconds': round(time.monotonic() - started, 3), 'command': command, 'log': str(log), 'cases': rust_cases(output)}
    print(f"{name}: {status} ({result['seconds']}秒)", flush=True)
    if status == 'failed':
        print('\n'.join(output.splitlines()[-25:]), flush=True)
    return result, output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cargo', default=shutil.which('cargo') or str(Path.home() / 'scoop/apps/rustup-msvc/current/.cargo/bin/cargo.exe'))
    parser.add_argument('--pwsh', default=shutil.which('pwsh') or 'pwsh')
    parser.add_argument('--timeout', type=int, default=600)
    parser.add_argument('--live-quad9', action='store_true')
    parser.add_argument('--interactive-desktop', action='store_true')
    parser.add_argument('--signed-package')
    parser.add_argument('--release-build', action='store_true')
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('timeoutは正数で指定してください')
    version = json.loads((ROOT / 'packaging/release-config.json').read_text(encoding='utf-8-sig'))['packageVersion']
    if not re.fullmatch(r'\d+\.\d+\.\d+\.\d+', version):
        raise ValueError('packageVersionが不正です')
    directory = ROOT / 'dev/history' / f'v{version}' / 'logs' / f"cli-{datetime.now(timezone.utc):%Y%m%dT%H%M%SZ}-{uuid.uuid4().hex[:8]}"
    directory.mkdir(parents=True)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(ROOT / 'target/cli-tests'), PYTHONDONTWRITEBYTECODE='1',
               PYTHONIOENCODING='utf-8', CARGO_TERM_COLOR='never')
    steps, coverage = [], []

    def run(name, command):
        result, output = execute(name, command, directory, args.timeout, env)
        steps.append(result)
        return result, output

    def optional(name, enabled, command):
        if enabled:
            result, _ = run(name, command)
            expected = 2 if name == 'desktop-live' else 1
            if name in ('quad9-live', 'desktop-live') and (len(result['cases']) != expected or any(case['status'] != 'passed' for case in result['cases'])):
                result.update(status='failed', reason=f'明示実行した対象試験が{expected}件成功していません')
        else:
            steps.append({'name': name, 'status': 'skipped', 'reason': 'オプション未指定'})

    try:
        run('fmt', [args.cargo, 'fmt', '--all', '--', '--check'])
        listed, output = run('rust-list', [args.cargo, 'test', '--locked', '--all-targets', '--', '--list'])
        names = set(re.findall(r'^(\S+): test$', output, re.MULTILINE))
        features = json.loads((ROOT / 'tests/case-map.json').read_text(encoding='utf-8-sig'))
        try:
            if listed['status'] != 'passed' or not names:
                raise ValueError('Rust試験一覧を取得できません')
            coverage = check_case_map(features, names)
            steps.append({'name': 'case-map', 'status': 'passed', 'registered_cases': len(names)})
        except ValueError as error:
            steps.append({'name': 'case-map', 'status': 'failed', 'reason': str(error)})
        tested, _ = run('rust', [args.cargo, 'test', '--locked', '--all-targets', '--', '--test-threads=1', '--color=never'])
        if {case['name'] for case in tested['cases']} != names or not names:
            tested.update(status='failed', reason='一覧と実行されたRust試験が一致しません')
        if any(case['status'] == 'failed' for case in tested['cases']):
            tested.update(status='failed', reason='失敗したRust試験があります')
        run('clippy', [args.cargo, 'clippy', '--locked', '--all-targets', '--', '-D', 'warnings'])
        run('runner-unit', [sys.executable, str(ROOT / 'dev/scripts/test-all-unit.py'), '-v'])
        run('ftps-plan', [sys.executable, str(ROOT / 'dev/scripts/test-httpselector-ftps.py'), '-v'])
        run('signing-policy', [args.pwsh, '-NoProfile', '-File', str(ROOT / 'dev/scripts/test-msix-signing-policy.ps1')])
        signed = ['-SignedPackagePath', str(Path(args.signed_package).resolve())] if args.signed_package else []
        run('msix-scripts', [args.pwsh, '-NoProfile', '-File', str(ROOT / 'dev/scripts/test-msix-scripts.ps1'), *signed])
        optional('release-build', args.release_build, [args.cargo, 'build', '--locked', '--release'])
        optional('quad9-live', args.live_quad9, [args.cargo, 'test', '--locked', '--lib', 'quad9::tests::live_quad9_http2_checks_safe_and_official_blocked_domains', '--', '--ignored', '--exact', '--test-threads=1'])
        optional('desktop-live', args.interactive_desktop, [args.cargo, 'test', '--locked', '--all-targets', 'interactive_desktop_', '--', '--ignored', '--test-threads=1'])
    except (Exception, KeyboardInterrupt) as error:
        steps.append({'name': 'runner', 'status': 'failed', 'reason': str(error)})
    passed = all(step['status'] != 'failed' for step in steps)
    report = {'version': version, 'passed': passed, 'steps': steps, 'features': coverage,
              'external_boundaries': ['実ブラウザーの表示内容・サインイン状態', 'Windows既定アプリの選択と実MSIX導入・更新', 'Store認証'],
              'signed_package': args.signed_package}
    (directory / 'results.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    write_junit(steps, directory / 'junit.xml')
    (directory / 'cases.json').write_text(json.dumps(coverage, ensure_ascii=False, indent=2), encoding='utf-8')
    print(f"結果: {'成功' if passed else '失敗'}\n{directory / 'results.json'}", flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    sys.exit(main())

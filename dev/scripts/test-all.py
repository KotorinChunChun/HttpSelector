"""HttpSelectorのCLI試験。実利用者設定・OS関連付け・信頼ストアの変更なし。"""
import argparse
from datetime import datetime, timezone
from dataclasses import dataclass
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import uuid
from cli_test_report import check_case_map, rust_cases, rust_inventory, selected_cases, validate_rust_result, write_reports

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


@dataclass(frozen=True)
class TestCommand:
    name: str
    arguments: list[str]
    enabled: bool = True
    kind: str = 'command'
    selection: str | None = None
    exact: bool = False


def build_test_plan(args):
    """実行順序とコマンドを定義する。成否判定やファイル出力は行わない。"""
    def cargo(*values):
        return [args.cargo, *values]

    def python(script):
        return [sys.executable, str(ROOT / 'dev/scripts' / script), '-v']

    def pwsh(script, *values):
        return [args.pwsh, '-NoProfile', '-File', str(ROOT / 'dev/scripts' / script), *values]

    signed = ['-SignedPackagePath', str(Path(args.signed_package).resolve())] if args.signed_package else []
    quad9_test = 'quad9::tests::live_quad9_http2_checks_safe_and_official_blocked_domains'
    desktop_filter = 'interactive_desktop_'
    return [
        TestCommand('fmt', cargo('fmt', '--all', '--', '--check')),
        TestCommand('rust-list', cargo('test', '--locked', '--all-targets', '--', '--list'), kind='inventory'),
        TestCommand('rust', cargo('test', '--locked', '--all-targets', '--', '--test-threads=1', '--color=never'), kind='rust'),
        TestCommand('clippy', cargo('clippy', '--locked', '--all-targets', '--', '-D', 'warnings')),
        TestCommand('runner-unit', python('test-all-unit.py')),
        TestCommand('ftps-plan', python('test-httpselector-ftps.py')),
        TestCommand('signing-policy', pwsh('test-msix-signing-policy.ps1')),
        TestCommand('msix-scripts', pwsh('test-msix-scripts.ps1', *signed)),
        TestCommand('release-build', cargo('build', '--locked', '--release'), enabled=args.release_build),
        TestCommand('quad9-live', cargo('test', '--locked', '--lib', quad9_test, '--', '--ignored', '--exact', '--test-threads=1'),
                    enabled=args.live_quad9, kind='rust', selection=quad9_test, exact=True),
        TestCommand('desktop-live', cargo('test', '--locked', '--all-targets', desktop_filter, '--', '--ignored', '--test-threads=1'),
                    enabled=args.interactive_desktop, kind='rust', selection=desktop_filter),
    ]


class TestRun:
    """1回の実行状態を保持し、コマンド実行と結果の照合をつなぐ。"""
    def __init__(self, args, directory, env, executor=execute, features=None):
        self.args = args
        self.directory = directory
        self.env = env
        self.executor = executor
        self.features = features
        self.steps = []
        self.coverage = []
        self.inventory = []

    def resolve_features(self, listed):
        try:
            if listed['status'] != 'passed' or not self.inventory:
                raise ValueError('Rust試験一覧を取得できません')
            features = self.features
            if features is None:
                features = json.loads((ROOT / 'tests/case-map.json').read_text(encoding='utf-8-sig'))
            self.coverage = check_case_map(features, self.inventory)
            self.steps.append({'name': 'case-map', 'status': 'passed', 'registered_cases': len(self.inventory)})
        except ValueError as error:
            self.steps.append({'name': 'case-map', 'status': 'failed', 'reason': str(error)})

    def run(self, plan):
        try:
            for spec in plan:
                if not spec.enabled:
                    self.steps.append({'name': spec.name, 'status': 'skipped', 'reason': 'オプション未指定'})
                    continue
                result, output = self.executor(spec.name, spec.arguments, self.directory, self.args.timeout, self.env)
                if spec.kind == 'rust':
                    expected = self.inventory if spec.selection is None else selected_cases(self.inventory, spec.selection, spec.exact)
                    result = validate_rust_result(result, expected, require_success=spec.selection is not None)
                self.steps.append(result)
                if spec.kind == 'inventory':
                    self.inventory = rust_inventory(output)
                    self.resolve_features(result)
        except (Exception, KeyboardInterrupt) as error:
            self.steps.append({'name': 'runner', 'status': 'failed', 'reason': str(error)})

    def report(self, version):
        return {'version': version, 'passed': bool(self.steps) and all(step['status'] != 'failed' for step in self.steps),
                'steps': self.steps, 'features': self.coverage,
                'external_boundaries': ['実ブラウザーの表示内容・サインイン状態', 'Windows既定アプリの選択と実MSIX導入・更新', 'Store認証'],
                'signed_package': self.args.signed_package}


def parse_arguments():
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
    return args


def main():
    args = parse_arguments()
    version = json.loads((ROOT / 'packaging/release-config.json').read_text(encoding='utf-8-sig'))['packageVersion']
    if not re.fullmatch(r'\d+\.\d+\.\d+\.\d+', version):
        raise ValueError('packageVersionが不正です')
    directory = ROOT / 'dev/history' / f'v{version}' / 'logs' / f"cli-{datetime.now(timezone.utc):%Y%m%dT%H%M%SZ}-{uuid.uuid4().hex[:8]}"
    directory.mkdir(parents=True)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TARGET_DIR=str(ROOT / 'target/cli-tests'), PYTHONDONTWRITEBYTECODE='1',
               PYTHONIOENCODING='utf-8', CARGO_TERM_COLOR='never')
    run = TestRun(args, directory, env)
    run.run(build_test_plan(args))
    report = run.report(version)
    write_reports(report, directory)
    print(f"結果: {'成功' if report['passed'] else '失敗'}\n{directory / 'results.json'}", flush=True)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    sys.exit(main())

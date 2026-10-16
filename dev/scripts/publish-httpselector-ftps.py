"""HttpSelectorの承認済み6配信ファイルだけをFTPSで退避・公開する。既存サイトの接続処理を再利用。"""
import argparse
import hashlib
import importlib.util
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[2]
REMOTE = '/home/excel-chunchun/www/download'
SITE = 'https://excel-chunchun.sakura.ne.jp/download/'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def plan():
    config = json.loads((ROOT / 'packaging/release-config.json').read_text(encoding='utf-8-sig'))
    version = config['packageVersion']
    if not re.fullmatch(r'\d+\.\d+\.\d+\.\d+', version):
        raise ValueError('4部版が必要です')
    dist = config['distribution']
    if dist['publicDownloadPage'] != SITE or dist['productPage'] != SITE + 'HttpSelector.html':
        raise ValueError('確認済みの公開先と一致しません')
    website = Path(dist['websiteProjectDirectory']).resolve()
    if website != Path('C:/develop/excel-chunchun').resolve():
        raise ValueError('Webプロジェクトの位置が不正です')
    site_root = (website / dist['siteRelativeDirectory']).resolve()
    if site_root != website / 'www/download':
        raise ValueError('公開領域が不正です')
    entries = checked_entries(site_root, dist['publishRelativePaths'], version)
    return website, site_root, {'site': SITE, 'remote': REMOTE, 'version': version, 'files': entries}


def checked_entries(site_root, targets, version):
    package = f'beta/HttpSelector_{version}_x64_dev.msix'
    expected = {package, 'beta/HttpSelector.appinstaller', 'beta/HttpSelector.Development.cer',
                'assets/httpselector-icon.png', 'httpselector-certificate/index.html', 'HttpSelector.html'}
    files = set()
    for item in targets:
        relative = PurePosixPath(item)
        if relative.is_absolute() or '..' in relative.parts or '\\' in item:
            raise ValueError('公開パスが不正です')
        target = (site_root / item).resolve()
        if not target.is_relative_to(site_root):
            raise ValueError('公開領域外です')
        if target.is_dir():
            files.update(path.relative_to(site_root).as_posix() for path in target.rglob('*') if path.is_file())
        else:
            files.add(item)
    if files != expected:
        raise ValueError('公開対象はHttpSelectorの指定6ファイルだけです')
    order = [package, 'beta/HttpSelector.Development.cer', 'assets/httpselector-icon.png',
             'httpselector-certificate/index.html', 'beta/HttpSelector.appinstaller', 'HttpSelector.html']
    entries = []
    for relative in order:
        path = (site_root / relative).resolve()
        if not path.is_relative_to(site_root):
            raise ValueError('リンク経由で公開領域外を参照しています')
        data = path.read_bytes()
        if not data:
            raise ValueError('空の配信ファイルです: ' + relative)
        if path.suffix == '.html' and re.search(r'(?:/secret/|シークレットゲーム一覧)', data.decode('utf-8')):
            raise ValueError('公開してはいけない一覧への入口があります')
        entries.append({'path': relative, 'bytes': len(data), 'sha256': digest(data)})
    return entries


def publish(website, site_root, value):
    # 既存サイトのTLS検証、確認済みホスト、資格情報読取、アトミック置換だけを利用する。
    sys.path.insert(0, str(website / 'scripts'))
    sys.dont_write_bytecode = True
    spec = importlib.util.spec_from_file_location('existing_ftps', website / 'scripts/publish-othello.py')
    base = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(base)
    run = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid4().hex[:8]
    output = ROOT / 'dev/history' / ('v' + value['version']) / 'logs' / ('publish-' + run)
    output.mkdir(parents=True, exist_ok=False)
    write_json(output / 'plan.json', value)
    with base.connect() as ftp:
        ftp.timeout = 120
        ftp.sock.settimeout(120)
        originals = {}
        parents = {}
        # 置換前に既存公開物を全件ローカル退避する。旧MSIXや共有メニューは触らない。
        for entry in value['files']:
            relative = entry['path']
            parent = str(PurePosixPath(relative).parent)
            if parent not in parents:
                parents[parent] = base.listing(ftp, REMOTE + '/' + parent)
            if PurePosixPath(relative).name not in parents[parent]:
                originals[relative] = None
                continue
            old = base.read_remote(ftp, REMOTE + '/' + relative)
            if relative.endswith('.msix') and digest(old) != entry['sha256']:
                raise ValueError('同じ版の別MSIXが既に公開されています。上書きしません')
            backup = output / 'backup' / relative
            backup.parent.mkdir(parents=True, exist_ok=True)
            backup.write_bytes(old)
            originals[relative] = digest(old)
        write_json(output / 'backup.json', {'originals': originals, 'plan': value})
        uploaded = []
        for entry in value['files']:
            relative = entry['path']
            data = (site_root / relative).read_bytes()
            if digest(data) != entry['sha256']:
                raise ValueError('計画後に変更されました: ' + relative)
            if originals[relative] is not None:
                if digest(base.read_remote(ftp, REMOTE + '/' + relative)) != originals[relative]:
                    raise ValueError('公開側が並行変更されています: ' + relative)
            if originals[relative] != entry['sha256']:
                base.replace_remote(ftp, REMOTE + '/' + relative, data, run, '644')
            if digest(base.read_remote(ftp, REMOTE + '/' + relative)) != entry['sha256']:
                raise ValueError('FTPS読戻しが一致しません: ' + relative)
            uploaded.append(entry)
            write_json(output / 'uploaded.json', uploaded)
            print('FTPS配備・照合: ' + relative, flush=True)
    print('公開前バックアップと転送記録: ' + str(output), flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--dry-run', action='store_true')
    action.add_argument('--execute', action='store_true')
    args = parser.parse_args()
    website, site_root, value = plan()
    print(json.dumps(value, ensure_ascii=False, indent=2), flush=True)
    if args.execute:
        publish(website, site_root, value)
    else:
        print('dry-run: 接続・公開変更なし。', flush=True)

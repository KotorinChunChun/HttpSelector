"""公開対象の限定・順序・拒否条件を一時フォルダーで検証する。接続・公開なし。"""
import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('publisher', Path(__file__).with_name('publish-httpselector-ftps.py'))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class PublicationPlanTests(unittest.TestCase):
    def setUp(self):
        fixture = publisher.ROOT / 'target/ftps-plan-tests'
        fixture.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=fixture)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.version = '0.0.3.3'
        self.targets = ['HttpSelector.html', 'httpselector-certificate', 'assets/httpselector-icon.png',
                        'beta/HttpSelector.appinstaller', 'beta/HttpSelector_0.0.3.3_x64_dev.msix',
                        'beta/HttpSelector.Development.cer']
        for relative in self.targets:
            if relative == 'httpselector-certificate':
                relative += '/index.html'
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'fixture only')

    def check(self):
        return publisher.checked_entries(self.root, self.targets, self.version)

    def test_binary_precedes_appinstaller_and_page_is_last(self):
        entries = self.check()
        self.assertEqual(len(entries), 6)
        self.assertTrue(entries[0]['path'].endswith('.msix'))
        self.assertEqual(entries[-2]['path'], 'beta/HttpSelector.appinstaller')
        self.assertEqual(entries[-1]['path'], 'HttpSelector.html')
        self.assertTrue(all(len(entry['sha256']) == 64 for entry in entries))

    def test_shared_menu_is_rejected(self):
        self.targets.append('index.html')
        with self.assertRaises(ValueError): self.check()

    def test_traversal_is_rejected(self):
        self.targets[0] = '../HttpSelector.html'
        with self.assertRaises(ValueError): self.check()

    def test_unexpected_file_inside_guide_is_rejected(self):
        (self.root / 'httpselector-certificate/secret.key').write_bytes(b'fixture')
        with self.assertRaises(ValueError): self.check()

    def test_forbidden_menu_link_is_rejected(self):
        (self.root / 'HttpSelector.html').write_text('<a href="/secret/">一覧</a>', encoding='utf-8')
        with self.assertRaises(ValueError): self.check()

    def test_empty_artifact_is_rejected(self):
        (self.root / 'beta/HttpSelector.appinstaller').write_bytes(b'')
        with self.assertRaises(ValueError): self.check()

    def test_migration_updates_only_guide_appinstaller_and_notice(self):
        targets = ['HttpSelector.html', 'httpselector-certificate', 'beta/HttpSelector.appinstaller']
        entries = publisher.checked_entries(self.root, targets, self.version, migration=True)
        self.assertEqual([entry['path'] for entry in entries],
                         ['httpselector-certificate/index.html', 'beta/HttpSelector.appinstaller', 'HttpSelector.html'])

    def test_migration_rejects_binary_upload(self):
        with self.assertRaises(ValueError):
            publisher.checked_entries(self.root, self.targets, self.version, migration=True)


if __name__ == '__main__':
    unittest.main()

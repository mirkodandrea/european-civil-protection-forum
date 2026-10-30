"""Tests for the Pages landing page: python3 -m unittest scripts/test_stage_pages.py"""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('stage_pages', Path(__file__).parent / 'stage_pages.py')
stage_pages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage_pages)
RELEASE = Path(__file__).parents[1] / 'release.json'


def site(root: Path, releases=None, versions=None):
    for branch in stage_pages.BRANCHES:
        d = root / branch
        d.mkdir(parents=True)
        (d / 'index.html').write_text('<!doctype html><title>app</title>')
        version = {'branch': branch, 'commit': 'abcdef0123456789abcdef0123456789abcdef01', 'run': '42'}
        version.update((versions or {}).get(branch, {}))
        (d / 'version.json').write_text(json.dumps(version))
        release = (releases or {}).get(branch)
        if release is not None:
            (d / 'release.json').write_text(release if isinstance(release, str) else json.dumps(release))
    return root


class StagePagesTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_repository_release_file_is_valid(self):
        release = stage_pages.load_release(RELEASE)
        self.assertTrue(release['title'])
        self.assertIn('', [m['query'] for m in release['modes']])

    def test_branch_metadata_and_modes_reach_the_page(self):
        main = json.loads(RELEASE.read_text())
        site(self.root, releases={'main': main}, versions={'main': {
            'run_number': '17', 'built_at': '2026-10-30T09:00:00Z',
            'committed_at': '2026-10-29T18:30:00+01:00', 'repository': 'owner/repo'}})
        stage_pages.stage(self.root)
        html = (self.root / 'index.html').read_text()
        self.assertIn(main['title'], html)
        for mode in main['modes']:
            self.assertIn(f'href="./main/{mode["query"]}"', html)
        self.assertIn('#17', html)
        self.assertIn('29 Oct 2026', html)
        self.assertIn('https://github.com/owner/repo/commit/abcdef0123456789abcdef0123456789abcdef01', html)
        self.assertIn('https://github.com/owner/repo/actions/runs/42', html)
        self.assertIn('download="ttx-dev.html"', html)
        self.assertTrue((self.root / '.nojekyll').exists())

    def test_missing_release_and_build_fields_fall_back(self):
        site(self.root)
        stage_pages.stage(self.root)
        html = (self.root / 'index.html').read_text()
        self.assertIn('Branch dev', html)
        self.assertIn('release.json is missing', html)
        self.assertIn('href="./dev/?simbench=1"', html)
        self.assertIn('run 42', html)
        self.assertIn('not recorded', html)

    def test_text_is_escaped(self):
        site(self.root, releases={'dev': {'title': '<script>x</script>', 'description': 'a & b'}})
        stage_pages.stage(self.root)
        html = (self.root / 'index.html').read_text()
        self.assertNotIn('<script>x', html)
        self.assertIn('&lt;script&gt;', html)

    def test_invalid_release_is_rejected(self):
        for bad in ['{', '[]', {'title': 'x'}, {'title': 'x', 'description': 'y', 'highlights': 'z'},
                    {'title': 'x', 'description': 'y', 'modes': [{'query': 'javascript:x', 'name': 'n', 'description': 'd'}]}]:
            with self.subTest(bad=bad), tempfile.TemporaryDirectory() as tmp:
                site(Path(tmp), releases={'main': bad})
                with self.assertRaises(ValueError):
                    stage_pages.stage(Path(tmp))

    def test_missing_app_and_wrong_branch_are_rejected(self):
        site(self.root)
        (self.root / 'dev' / 'index.html').write_text('')
        with self.assertRaisesRegex(ValueError, 'Missing standalone app'):
            stage_pages.stage(self.root)
        (self.root / 'dev' / 'index.html').write_text('app')
        (self.root / 'dev' / 'version.json').write_text(json.dumps({'branch': 'main', 'commit': 'x'}))
        with self.assertRaisesRegex(ValueError, 'Wrong branch metadata'):
            stage_pages.stage(self.root)


if __name__ == '__main__':
    unittest.main()

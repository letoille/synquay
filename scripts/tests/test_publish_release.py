"""Exercise release publishing with a fake GitHub CLI, without remote mutations."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'publish-release.sh'
COMMIT = 'a' * 40
MOCK = '''#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ['CALLS'], 'a') as f:
    f.write(json.dumps(args) + '\\n')
if args[0] == 'api':
    if os.getenv('API_FAIL') == 'true': sys.exit(1)
    endpoint = next(a for a in args if a.startswith('repos/'))
    if '/releases?' in endpoint:
        print('42' if os.getenv('EXISTS') == 'true' else '')
    elif endpoint.endswith('/releases/42'):
        print(os.getenv('IMMUTABLE', 'false'))
    elif '/matching-refs/' in endpoint:
        print('refs/tags/v0.1.0' if os.getenv('TAG_EXISTS') == 'true' else '')
    elif '/commits/' in endpoint:
        print(os.getenv('TAG_COMMIT', os.environ['COMMIT']))
elif args[:2] == ['release', 'upload'] and os.getenv('UPLOAD_FAIL') == 'true':
    sys.exit(1)
'''


class PublishReleaseTests(unittest.TestCase):
    def run_publish(self, **options):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            gh = root / 'gh'
            gh.write_text(MOCK)
            gh.chmod(0o755)
            package = root / 'synquay_0.1.0-1_amd64.deb'
            package.write_bytes(b'test-package')
            (root / 'SHA256SUMS').write_text(
                f'{hashlib.sha256(package.read_bytes()).hexdigest()}  {package.name}\n')
            env = {**os.environ, 'PATH': f'{root}:{os.environ["PATH"]}',
                   'GH_REPO': 'test/synquay', 'VERSION': '0.1.0',
                   'COMMIT': COMMIT, 'PRERELEASE': 'false',
                   'GITHUB_EVENT_NAME': 'workflow_dispatch',
                   'GITHUB_REF_TYPE': 'branch', 'REPLACE_TAG': 'false',
                   'CALLS': str(root / 'calls'), **options}
            result = subprocess.run(['bash', str(SCRIPT), str(root)], env=env,
                                    capture_output=True, text=True)
            calls = [json.loads(line) for line in (root / 'calls').read_text().splitlines()]
            return result, calls

    def test_new_release(self):
        result, calls = self.run_publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        create = next(c for c in calls if c[:2] == ['release', 'create'])
        self.assertEqual(create[create.index('--target') + 1], COMMIT)

    def test_overwrite_same_commit_preserves_release(self):
        result, calls = self.run_publish(EXISTS='true', TAG_EXISTS='true')
        self.assertEqual(result.returncode, 0, result.stderr)
        mutations = [c for c in calls if c[0] == 'release']
        self.assertEqual([c[1] for c in mutations], ['upload', 'edit'])
        self.assertIn('--clobber', mutations[0])
        self.assertIn('--prerelease=false', mutations[1])
        self.assertFalse(any('PATCH' in c for c in calls))

    def test_different_commit_requires_explicit_manual_branch_option(self):
        for extra in ({}, {'REPLACE_TAG': 'true', 'GITHUB_REF_TYPE': 'tag'},
                      {'REPLACE_TAG': 'true', 'GITHUB_EVENT_NAME': 'push'}):
            with self.subTest(extra=extra):
                result, calls = self.run_publish(EXISTS='true', TAG_EXISTS='true',
                                                 TAG_COMMIT='b' * 40, **extra)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(any(c[0] == 'release' or 'PATCH' in c for c in calls))

    def test_explicit_manual_branch_retargets_then_uploads(self):
        result, calls = self.run_publish(EXISTS='true', TAG_EXISTS='true',
                                         TAG_COMMIT='b' * 40, REPLACE_TAG='true')
        self.assertEqual(result.returncode, 0, result.stderr)
        patch = next(c for c in calls if 'PATCH' in c)
        self.assertIn(f'sha={COMMIT}', patch)
        self.assertIn('force=true', patch)
        self.assertLess(calls.index(patch), next(i for i, c in enumerate(calls) if c[0] == 'release'))

    def test_api_failure_is_not_treated_as_missing_release(self):
        result, calls = self.run_publish(API_FAIL='true')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(c[0] == 'release' for c in calls))

    def test_immutable_release_stops_before_tag_or_asset_mutations(self):
        result, calls = self.run_publish(EXISTS='true', IMMUTABLE='true', REPLACE_TAG='true')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(c[0] == 'release' or 'PATCH' in c for c in calls))

    def test_failed_upload_does_not_update_notes(self):
        result, calls = self.run_publish(EXISTS='true', UPLOAD_FAIL='true')
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(c[:2] == ['release', 'edit'] for c in calls))


if __name__ == '__main__':
    unittest.main()

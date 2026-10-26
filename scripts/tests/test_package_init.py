import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("initialize", ROOT / "backend/deploy/initialize.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InitializationTest(unittest.TestCase):
    def test_secrets_are_unique_private_and_upgrades_preserve_configuration(self):
        with tempfile.TemporaryDirectory() as temp:
            first, second = Path(temp) / "a", Path(temp) / "b"
            templates = ROOT / "backend/deploy/config"
            module.initialize(first, templates)
            module.initialize(second, templates)
            secret = first / "credentials.env"
            self.assertEqual(secret.stat().st_mode & 0o777, 0o600)
            self.assertNotEqual(secret.read_bytes(), (second / secret.name).read_bytes())
            before = secret.read_bytes()
            (first / "tools.json").write_text('{"operator":"custom"}')
            module.initialize(first, templates)
            self.assertEqual(before, secret.read_bytes())
            self.assertEqual((first / "tools.json").read_text(), '{"operator":"custom"}')
            self.assertEqual(list(first.glob('.synquay-init-*')), [])

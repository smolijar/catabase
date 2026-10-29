"""Exercise the shell installer against disposable packages, never the real app."""
import hashlib
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "install.sh"


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="catabase-installer-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.home = self.root / "home"
        self.home.mkdir()
        self.dest = self.home / "Applications"
        self.archive = self.root / "Catabase-macos-universal.app.zip"
        self.make_package()
        self.env = dict(os.environ, HOME=str(self.home), PATH=f"{self.bin}:{os.environ['PATH']}",
                        FIXTURE_ROOT=str(self.root), EXPECTED_ARCHIVE=str(self.archive),
                        MOCK_VERSION="0.1.2", MOCK_OS="Darwin", MOCK_ARCH="arm64")
        self.mock("uname", "print(os.environ['MOCK_OS'] if sys.argv[1] == '-s' else os.environ['MOCK_ARCH'])")
        self.mock("id", "print('501')")
        self.mock("pgrep", "sys.exit(int(os.environ.get('MOCK_PGREP_STATUS', '1')))")
        self.mock("file", "print('Mach-O universal binary with 2 architectures: [x86_64] [arm64]')")
        self.mock("codesign", "sys.exit(int(os.environ.get('MOCK_SIGNATURE_STATUS', '0')))")
        self.mock("plutil", """
import plistlib
with open(sys.argv[-1], 'rb') as source:
    value = plistlib.load(source)[sys.argv[sys.argv.index('-extract') + 1]]
print(value)
""")
        self.mock("ditto", """
import zipfile
with zipfile.ZipFile(sys.argv[-2]) as archive:
    archive.extractall(sys.argv[-1])
executable = pathlib.Path(sys.argv[-1]) / 'Catabase.app/Contents/MacOS/catabase'
executable.chmod(0o755)
""")
        self.mock("curl", """
import shutil
root = pathlib.Path(os.environ['FIXTURE_ROOT'])
url = sys.argv[-1]
with (root / 'requests').open('a') as log: log.write(url + '\\n')
if url.endswith('/releases/latest'):
    print(os.environ.get('MOCK_RELEASE_URL', 'https://github.com/smolijar/catabase/releases/tag/v0.1.2'), end='')
else:
    if os.environ.get('MOCK_DOWNLOAD_FAILURE'): sys.exit(22)
    if '/download/v0.1.2/' not in url: sys.exit('Download was not pinned to the resolved release')
    source = root / url.rsplit('/', 1)[-1]
    shutil.copyfile(source, sys.argv[sys.argv.index('--output') + 1])
""")

    def mock(self, name, body):
        path = self.bin / name
        path.write_text(f"#!{sys.executable}\nimport os, sys, pathlib\n" + body + "\n")
        path.chmod(0o755)

    def make_package(self, identifier="com.catabase.app", version="0.1.2", extra=None):
        plist = {"CFBundleIdentifier": identifier, "CFBundleExecutable": "catabase",
                 "CFBundleShortVersionString": version}
        with zipfile.ZipFile(self.archive, "w") as archive:
            archive.writestr("Catabase.app/Contents/Info.plist", plistlib.dumps(plist))
            archive.writestr("Catabase.app/Contents/MacOS/catabase", b"fixture application; never executed")
            if extra: archive.writestr(extra, b"unexpected")
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        (self.root / "SHA256SUMS").write_text(f"{digest}  ./{self.archive.name}\n")

    def run_installer(self, *arguments, success=True):
        result = subprocess.run(["sh", str(INSTALLER), *arguments], env=self.env,
                                capture_output=True, text=True)
        if success:
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        else:
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def old_install(self):
        app = self.dest / "Catabase.app"
        (app / "Contents").mkdir(parents=True)
        (app / "Contents/Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": "com.catabase.app"}))
        (app / "old-marker").write_text("preserve me")
        return app

    def assert_no_staging(self):
        self.assertEqual(list(self.dest.glob(".catabase-install.*")), [])

    def test_installs_verified_app_and_pins_both_assets(self):
        result = self.run_installer()
        self.assertTrue((self.dest / "Catabase.app/Contents/MacOS/catabase").is_file())
        self.assertIn("Installed Catabase 0.1.2", result.stdout)
        urls = (self.root / "requests").read_text().splitlines()
        self.assertEqual(len(urls), 3)
        self.assertTrue(all("/download/v0.1.2/" in url for url in urls[1:]))
        self.assert_no_staging()

    def test_updates_existing_app_and_preserves_unrelated_user_data(self):
        self.old_install()
        data = self.home / "saved-workspace"
        data.write_text("connections and history")
        self.run_installer()
        self.assertFalse((self.dest / "Catabase.app/old-marker").exists())
        self.assertEqual(data.read_text(), "connections and history")
        self.assert_no_staging()

    def test_custom_directory_with_spaces(self):
        destination = self.root / "My Applications"
        self.run_installer("--dir", str(destination))
        self.assertTrue((destination / "Catabase.app").is_dir())
        self.assertFalse(self.dest.exists())

    def test_checksum_failure_leaves_existing_app(self):
        old = self.old_install()
        (self.root / "SHA256SUMS").write_text(f"{'0' * 64}  {self.archive.name}\n")
        result = self.run_installer(success=False)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertTrue((old / "old-marker").exists())
        self.assert_no_staging()

    def test_duplicate_checksum_rejected(self):
        sums = self.root / "SHA256SUMS"
        sums.write_text(sums.read_text() * 2)
        self.run_installer(success=False)
        self.assertFalse((self.dest / "Catabase.app").exists())
        self.assert_no_staging()

    def test_signature_failure_leaves_existing_app(self):
        old = self.old_install()
        self.env["MOCK_SIGNATURE_STATUS"] = "1"
        result = self.run_installer(success=False)
        self.assertIn("signature verification failed", result.stderr)
        self.assertTrue((old / "old-marker").exists())
        self.assert_no_staging()

    def test_mismatched_bundle_identifier_and_version_rejected(self):
        for kwargs in [{"identifier": "other.app"}, {"version": "0.1.1"}]:
            with self.subTest(**kwargs):
                self.make_package(**kwargs)
                self.run_installer(success=False)
                self.assertFalse((self.dest / "Catabase.app").exists())
                self.assert_no_staging()

    def test_running_app_is_not_replaced(self):
        old = self.old_install()
        self.env["MOCK_PGREP_STATUS"] = "0"
        result = self.run_installer(success=False)
        self.assertIn("Quit Catabase", result.stderr)
        self.assertTrue((old / "old-marker").exists())
        self.assertFalse((self.root / "requests").exists())

    def test_failed_process_check_does_not_allow_replacement(self):
        self.env["MOCK_PGREP_STATUS"] = "2"
        result = self.run_installer(success=False)
        self.assertIn("Could not check", result.stderr)

    def test_unexpected_release_host_and_tag_are_rejected(self):
        for url in ["https://other.example/tag/v0.1.2", "https://github.com/smolijar/catabase/releases/tag/v0.1.2?x=1",
                    "https://github.com/smolijar/catabase/releases/tag/v0.1.2/../bad",
                    "https://github.com/smolijar/catabase/releases/tag/v0.1.2\nevil"]:
            with self.subTest(url=url):
                self.env["MOCK_RELEASE_URL"] = url
                self.run_installer(success=False)
                self.assertFalse(self.dest.exists())

    def test_archive_path_traversal_is_rejected(self):
        self.make_package(extra="../escaped")
        self.run_installer(success=False)
        self.assertFalse((self.dest / "Catabase.app").exists())
        self.assert_no_staging()

    def test_destination_symlink_is_not_replaced(self):
        outside = self.root / "outside"
        outside.mkdir()
        self.dest.mkdir()
        (self.dest / "Catabase.app").symlink_to(outside, target_is_directory=True)
        self.run_installer(success=False)
        self.assertTrue((self.dest / "Catabase.app").is_symlink())
        self.assertEqual(list(outside.iterdir()), [])

    def test_swap_failure_restores_previous_app(self):
        old = self.old_install()
        real_mv = shutil.which("mv")
        self.mock("mv", f"""
import subprocess
if '/unpacked/Catabase.app' in sys.argv[1]: sys.exit(1)
sys.exit(subprocess.call([{real_mv!r}, *sys.argv[1:]]))
""")
        result = self.run_installer(success=False)
        self.assertIn("Restored the previous", result.stderr)
        self.assertTrue((old / "old-marker").exists())
        self.assert_no_staging()

    def test_concurrent_installation_is_refused(self):
        old = self.old_install()
        lock = self.dest / ".catabase-install.lock"
        lock.mkdir()
        result = self.run_installer(success=False)
        self.assertIn("Another installer may be running", result.stderr)
        self.assertTrue((old / "old-marker").exists())
        self.assertTrue(lock.is_dir())

    def test_unsupported_os_and_relative_directory_fail(self):
        self.env["MOCK_OS"] = "Linux"
        self.run_installer(success=False)
        self.run_installer("--dir", "relative", success=False)
        self.assertFalse(self.dest.exists())


if __name__ == "__main__":
    unittest.main()

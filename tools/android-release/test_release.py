"""Unit tests for the Cranklink release tools. Run: python3 -m unittest discover -s tools/android-release"""

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch


def module(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), Path(__file__).with_name(name + ".py"))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


bundle = module("verify-bundle")
play = module("play")


def elf(headers):
    data = bytearray(64 + 56 * len(headers))
    data[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<Q", data, 32, 64)
    struct.pack_into("<HH", data, 54, 56, len(headers))
    for index, values in enumerate(headers):
        struct.pack_into("<IIQQQQQQ", data, 64 + index * 56, *values)
    return bytes(data)


MANIFEST = """<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="app.plainoss.cranklink"
  android:versionCode="100003" android:versionName="0.1.3"><uses-sdk android:minSdkVersion="26"
  android:targetSdkVersion="36"/><application {extra}/></manifest>"""


class BundleTest(unittest.TestCase):
    def test_rejects_4k_load_alignment(self):
        with self.assertRaisesRegex(ValueError, "LOAD"):
            bundle.native_alignment(elf([(1, 5, 0, 0, 0, 4096, 4096, 4096)]), "bad.so")

    def test_accepts_padded_relro_separate_from_writable_load(self):
        bundle.native_alignment(elf([
            (1, 6, 0, 0, 0, 0x390, 0x390, 0x4000),
            (1, 6, 0x390, 0x4390, 0, 0, 0x10, 0x4000),
            (0x6474E552, 4, 0, 0, 0, 0x390, 0x1000, 1),
        ]), "padded.so")

    def test_rejects_relro_rounding_into_writable_data(self):
        with self.assertRaisesRegex(ValueError, "RELRO"):
            bundle.native_alignment(elf([
                (1, 6, 0, 0, 0, 0x4000, 0x4000, 0x4000),
                (0x6474E552, 4, 0, 0, 0, 0x1000, 0x1000, 1),
            ]), "overlap.so")

    def test_manifest_identity(self):
        bundle.check_manifest(MANIFEST.format(extra=""), 100003, "0.1.3")
        with self.assertRaisesRegex(ValueError, "versionCode"):
            bundle.check_manifest(MANIFEST.format(extra=""), 100004, "0.1.3")
        with self.assertRaisesRegex(ValueError, "debuggable"):
            bundle.check_manifest(MANIFEST.format(extra='android:debuggable="true"'), 100003, "0.1.3")

    def test_requires_core_library_for_both_abis(self):
        names = [f"base/lib/{abi}/libcranklink_android.so" for abi in ("arm64-v8a", "x86_64")]
        self.assertEqual(bundle.native_libraries(names + ["base/dex/classes.dex"]), sorted(names))
        with self.assertRaisesRegex(ValueError, "exactly"):
            bundle.native_libraries(names[:1])
        with self.assertRaisesRegex(ValueError, "exactly"):
            bundle.native_libraries(names + ["base/lib/armeabi-v7a/libcranklink_android.so"])


class Response:
    def __init__(self, value):
        self.value = value

    def __enter__(self):
        return self

    def __exit__(self, *_):
        pass

    def read(self):
        return json.dumps(self.value).encode() if self.value is not None else b""


class FakePlay:
    """Just enough of the Android Publisher edits API to exercise retries."""

    def __init__(self):
        self.bundles = []
        self.tracks = []
        self.production = []
        self.calls = []
        self.uploaded_sha = None
        self.fail_commit = None

    def __call__(self, request, timeout):
        method, url = request.get_method(), request.full_url
        path = url.split(play.APPLICATION_ID, 1)[1]
        self.calls.append((method, path))
        if method == "POST" and path == "/edits":
            self.pending = {"tracks": [dict(t) for t in self.tracks], "bundles": list(self.bundles)}
            return Response({"id": "e"})
        if method == "GET" and path == "/edits/e/bundles":
            return Response({"bundles": self.pending["bundles"]})
        if method == "GET" and path == "/edits/e/tracks":
            return Response({"tracks": self.pending["tracks"]})
        if method == "GET" and path == "/tracks/production/releases":
            return Response({"releases": self.production})
        if method == "POST" and path.startswith("/edits/e/bundles"):
            uploaded = {"versionCode": 100003, "sha256": self.uploaded_sha or hashlib.sha256(request.data).hexdigest()}
            self.pending["bundles"].append(uploaded)
            return Response(uploaded)
        if method == "PUT" and path.startswith("/edits/e/tracks/"):
            body = json.loads(request.data)
            self.pending["tracks"] = [t for t in self.pending["tracks"] if t["track"] != body["track"]] + [body]
            return Response(body)
        if method == "POST" and path.startswith("/edits/e:commit"):
            if self.fail_commit:
                raise play.urllib.error.HTTPError(url, 400, "", {}, __import__("io").BytesIO(self.fail_commit.encode()))
            self.bundles, self.tracks = self.pending["bundles"], self.pending["tracks"]
            return Response(None)
        if method == "DELETE":
            return Response(None)
        raise AssertionError(f"Unexpected {method} {path}")


class PlayTest(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.directory = Path(temp.name)
        (self.directory / play.BUNDLE).write_bytes(b"verified bundle")
        self.checksum = hashlib.sha256(b"verified bundle").hexdigest()
        self.metadata = {"applicationId": play.APPLICATION_ID, "sha256": self.checksum, "commit": "abc",
                         "versionCode": 100003, "versionName": "0.1.3", "uploadCertificateSha256": "aa"}
        (self.directory / "release.json").write_text(json.dumps(self.metadata))
        (self.directory / "release-notes.txt").write_text("Notes\n")
        (self.directory / "source.json").write_text(json.dumps({"runUrl": "https://example.test/run"}))
        self.play = FakePlay()
        for patcher in (patch.dict(os.environ, {"GITHUB_SHA": "abc", "PLAY_ACCESS_TOKEN": "t"}),
                        patch.object(play.urllib.request, "urlopen", self.play)):
            patcher.start()
            self.addCleanup(patcher.stop)

    def internal(self, status="completed"):
        return {"track": "internal", "releases": [{"versionCodes": ["100003"], "status": status}]}

    def test_first_publish_uploads_and_commits(self):
        self.assertEqual(play.publish_internal(self.directory, "draft"), "upload")
        self.assertEqual(self.play.tracks[0]["releases"][0]["status"], "draft")
        self.assertEqual(self.play.tracks[0]["releases"][0]["releaseNotes"][0]["text"], "Notes")

    def test_retry_after_publish_is_a_no_op(self):
        play.publish_internal(self.directory, "completed")
        uploads = sum(1 for c in self.play.calls if c[1].startswith("/edits/e/bundles?"))
        self.assertEqual(play.publish_internal(self.directory, "completed"), "published")
        self.assertEqual(uploads, sum(1 for c in self.play.calls if c[1].startswith("/edits/e/bundles?")))
        self.assertEqual(sum(1 for c in self.play.calls if ":commit" in c[1]), 1)

    def test_finishes_track_without_reuploading(self):
        self.play.bundles = [{"versionCode": 100003, "sha256": self.checksum}]
        self.assertEqual(play.publish_internal(self.directory, "completed"), "track")
        self.assertFalse(any(c[1].startswith("/edits/e/bundles?") for c in self.play.calls))

    def test_rejects_different_bytes_stale_versions_and_wrong_commit(self):
        self.play.bundles = [{"versionCode": 100003, "sha256": "0" * 64}]
        with self.assertRaisesRegex(ValueError, "different bytes"):
            play.publish_internal(self.directory, "completed")
        self.play.bundles = [{"versionCode": 100004, "sha256": "1" * 64}]
        with self.assertRaisesRegex(ValueError, "stale"):
            play.publish_internal(self.directory, "completed")
        self.play.bundles = []
        with patch.dict(os.environ, {"GITHUB_SHA": "other"}), self.assertRaisesRegex(ValueError, "different commit"):
            play.publish_internal(self.directory, "completed")
        self.assertTrue(all(":commit" not in c[1] for c in self.play.calls))

    def test_waits_for_production_review(self):
        self.play.production = [{"releaseLifecycleState": "RELEASE_LIFECYCLE_STATE_IN_REVIEW"}]
        with self.assertRaisesRegex(ValueError, "in review"):
            play.publish_internal(self.directory, "completed")
        self.assertEqual(self.play.calls[-1][0], "DELETE")

    def test_rejects_unexpected_upload_result(self):
        self.play.uploaded_sha = "f" * 64
        with self.assertRaisesRegex(ValueError, "different version or checksum"):
            play.publish_internal(self.directory, "completed")

    def test_resolve_version(self):
        self.assertEqual(play.resolve_version("0.1.7", 100000), (100007, "0.1.7"))
        self.assertEqual(play.resolve_version(" 100007 ", 100000), (100007, None))
        for value in ("1.0", "100000", "latest"):
            with self.assertRaises(ValueError):
                play.resolve_version(value, 100000)

    def test_production_action(self):
        self.assertEqual(play.production_action([], 5), "submit")
        done = [{"track": "production", "releases": [{"versionCodes": ["5"], "status": "completed"}]}]
        self.assertEqual(play.production_action(done, 5), "already-submitted")
        with self.assertRaisesRegex(ValueError, "downgrade"):
            play.production_action(done, 4)
        staged = [{"track": "production", "releases": [{"versionCodes": ["4"], "status": "inProgress"}]}]
        with self.assertRaisesRegex(ValueError, "staged"):
            play.production_action(staged, 5)

    def test_promote_submits_existing_bundle(self):
        self.play.bundles = [{"versionCode": 100003, "sha256": self.checksum}]
        self.assertEqual(play.promote(self.directory, 100003, "AA"), "submitted")
        self.assertEqual(self.play.tracks[0]["track"], "production")
        self.assertEqual(play.promote(self.directory, 100003, "AA"), "already-submitted")

    def test_promote_explains_first_release_and_missing_bundle(self):
        with self.assertRaisesRegex(ValueError, "no longer lists"):
            play.promote(self.directory, 100003, "aa")
        self.play.bundles = [{"versionCode": 100003, "sha256": self.checksum}]
        self.play.fail_commit = play.DRAFT_APP
        with self.assertRaisesRegex(RuntimeError, "from Console"):
            play.promote(self.directory, 100003, "aa")
        with self.assertRaisesRegex(ValueError, "certificate"):
            play.promote(self.directory, 100003, "bb")


if __name__ == "__main__":
    unittest.main()

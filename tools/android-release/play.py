#!/usr/bin/env python3
"""Publish verified Cranklink bundles to Google Play without ever changing version codes.

internal DIR            Upload DIR's bundle to the internal track, or finish/confirm an earlier attempt.
select VERSION DIR      Download the original internal CI artifact for a tested version name or code.
promote DIR CODE        Send that exact, already uploaded bundle to production.

Play access uses the short-lived token in PLAY_ACCESS_TOKEN (GitHub OIDC); no key file is read.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.error
import urllib.request

APPLICATION_ID = "app.plainoss.cranklink"
API = f"https://androidpublisher.googleapis.com/androidpublisher/v3/applications/{APPLICATION_ID}"
UPLOAD = f"https://androidpublisher.googleapis.com/upload/androidpublisher/v3/applications/{APPLICATION_ID}"
INTERNAL_WORKFLOW = ".github/workflows/android-release-internal.yml"
BUNDLE = "androidApp-release.aab"
DRAFT_APP = "Only releases with status draft may be created on draft app"


def api(method, path, body=None, data=None, base=API, timeout=60):
    headers = {"Authorization": "Bearer " + os.environ["PLAY_ACCESS_TOKEN"]}
    if data is not None:
        headers["Content-Type"] = "application/octet-stream"
    elif body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    request = urllib.request.Request(base + path, method=method, data=data, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            payload = response.read()
            return json.loads(payload) if payload else {}
    except urllib.error.HTTPError as error:
        # Report Play's own explanation, never the request's bearer token.
        raise RuntimeError(f"Play API {method} {path} failed ({error.code}): {error.read().decode()}") from None


class Edit:
    """A Play edit that is deleted unless committed, so a failure leaves no half-made changes."""

    def __enter__(self):
        self.id = api("POST", "/edits", {})["id"]
        self.committed = False
        return self

    def get(self, suffix):
        return api("GET", f"/edits/{self.id}{suffix}")

    def put(self, suffix, body):
        return api("PUT", f"/edits/{self.id}{suffix}", body)

    def upload_bundle(self, path):
        return api("POST", f"/edits/{self.id}/bundles?uploadType=media", data=path.read_bytes(),
                   base=UPLOAD, timeout=600)

    def commit(self, query):
        api("POST", f"/edits/{self.id}:commit?{query}")
        self.committed = True

    def __exit__(self, *_):
        if not self.committed:
            # A timed-out commit may still have succeeded; a rerun reconciles it.
            try:
                api("DELETE", f"/edits/{self.id}")
            except Exception as error:
                print(f"Edit cleanup failed: {error}")


def load_artifact(directory):
    metadata = json.loads((directory / "release.json").read_text())
    checksum = hashlib.sha256((directory / BUNDLE).read_bytes()).hexdigest()
    if metadata["applicationId"] != APPLICATION_ID or metadata["sha256"] != checksum:
        raise ValueError("Artifact identity or checksum mismatch")
    notes = (directory / "release-notes.txt").read_text().strip()
    if not notes or len(notes) > 500:
        raise ValueError("Release notes must contain 1–500 characters")
    return metadata, notes


def release(metadata, notes, status):
    return {"name": metadata["versionName"], "versionCodes": [str(metadata["versionCode"])], "status": status,
            "releaseNotes": [{"language": "en-US", "text": notes}]}


def internal_state(bundles, tracks, code, checksum, status):
    """Decide what the internal publish still has to do: 'upload', 'track' or 'published'."""
    existing = next((b for b in bundles if b["versionCode"] == code), None)
    if existing and existing.get("sha256", "").lower() != checksum:
        raise ValueError("Play already holds different bytes for this version code; refusing to publish")
    codes = [b["versionCode"] for b in bundles]
    codes += [int(v) for t in tracks for r in t.get("releases", []) for v in r.get("versionCodes", [])]
    if max(codes, default=0) > code:
        raise ValueError("Candidate is stale: Play already holds a newer version")
    if not existing and code in codes:
        raise ValueError("This version code is already used by another bundle")
    published = any(t["track"] == "internal" and any(str(code) in r.get("versionCodes", []) and r.get("status") == status
                                                     for r in t.get("releases", [])) for t in tracks)
    if existing and published:
        return "published"
    if published:
        raise ValueError("Internal track names this version but Play has no bundle for it")
    return "track" if existing else "upload"


def publish_internal(directory, status):
    metadata, notes = load_artifact(directory)
    if metadata["commit"] != os.environ["GITHUB_SHA"]:
        raise ValueError("Artifact was built from a different commit")
    code, checksum = metadata["versionCode"], metadata["sha256"]
    with Edit() as edit:
        state = internal_state(edit.get("/bundles").get("bundles", []), edit.get("/tracks").get("tracks", []),
                               code, checksum, status)
        if state != "published":
            # The commit uses Play's default review handling; never disturb a production review.
            production = api("GET", "/tracks/production/releases").get("releases", [])
            if any(r.get("releaseLifecycleState") == "RELEASE_LIFECYCLE_STATE_IN_REVIEW" for r in production):
                raise ValueError("Production is in review; publish a new internal build once Google finishes")
            if state == "upload":
                uploaded = edit.upload_bundle(directory / BUNDLE)
                if uploaded["versionCode"] != code or uploaded["sha256"].lower() != checksum:
                    raise ValueError("Play recorded a different version or checksum for the upload")
            edit.put("/tracks/internal", {"track": "internal", "releases": [release(metadata, notes, status)]})
            edit.commit("changesInReviewBehavior=ERROR_IF_IN_REVIEW")
    with Edit() as edit:
        if internal_state(edit.get("/bundles").get("bundles", []), edit.get("/tracks").get("tracks", []),
                          code, checksum, status) != "published":
            raise ValueError("Play does not show the expected internal release after committing")
    print(f"Internal {metadata['versionName']} ({code}): {'already published' if state == 'published' else 'published'}")
    return state


def resolve_version(value, offset):
    value = value.strip()
    name = re.fullmatch(r"0\.1\.([1-9][0-9]*)", value)
    if name:
        # Internal CI publishes 0.1.<run number> as offset + run number.
        code = offset + int(name[1])
    elif re.fullmatch(r"[1-9][0-9]*", value):
        code = int(value)
    else:
        raise ValueError("Enter the Play version name (for example 0.1.7) or version code (for example 100007).")
    if not offset < code <= 2100000000:
        raise ValueError("Version is outside the internal release sequence")
    return code, value if name else None


def gh(*args):
    return subprocess.check_output(["gh", *args], text=True)


def select(value, directory):
    """Download the immutable artifact of the internal run that published this version."""
    code, name = resolve_version(value, int(os.environ["VERSION_OFFSET"]))
    repository = os.environ["GITHUB_REPOSITORY"]
    run_number = code - int(os.environ["VERSION_OFFSET"])
    pages = json.loads(gh("api", "--paginate", "--slurp",
                          f"repos/{repository}/actions/workflows/{Path(INTERNAL_WORKFLOW).name}/runs?branch=main&per_page=100"))
    matches = [r for page in pages for r in page["workflow_runs"] if r["run_number"] == run_number]
    if len(matches) != 1:
        raise ValueError("Could not uniquely find the internal release run for this version")
    run = matches[0]
    # GitHub may report the path with a ref suffix, e.g. "...internal.yml@refs/heads/main".
    if (run["path"].split("@")[0] != INTERNAL_WORKFLOW or run["head_branch"] != "main" or
            run["head_repository"]["full_name"] != repository or run["event"] not in ("push", "workflow_dispatch") or
            run["conclusion"] != "success"):
        raise ValueError("Source must be a successful internal release run on main")
    jobs = json.loads(gh("api", "--paginate", "--slurp", f"repos/{repository}/actions/runs/{run['id']}/jobs?per_page=100"))
    if not any(j["name"] == "publish" and j["conclusion"] == "success" for page in jobs for j in page["jobs"]):
        raise ValueError("That run did not publish to internal testing")
    directory.mkdir(parents=True, exist_ok=False)
    gh("run", "download", str(run["id"]), "--repo", repository, "--name", f"cranklink-release-{run['id']}",
       "--dir", str(directory))
    metadata, _ = load_artifact(directory)
    if (metadata["versionCode"] != code or metadata["commit"] != run["head_sha"] or
            (name is not None and metadata["versionName"] != name)):
        raise ValueError("Artifact version or source commit does not match its run")
    (directory / "source.json").write_text(json.dumps({"runUrl": run["html_url"]}) + "\n")
    summary(f"### Selected {metadata['versionName']} ({code})\n\nInternal run: {run['html_url']}\n\n"
            f"Commit: `{metadata['commit']}`\n\nSHA-256: `{metadata['sha256']}`\n")
    output(version_code=code)
    return code


def production_action(tracks, code):
    releases = next((t.get("releases", []) for t in tracks if t["track"] == "production"), [])
    if any(int(v) > code for r in releases for v in r.get("versionCodes", [])):
        raise ValueError("Production holds a newer version; refusing a downgrade")
    if any(r.get("status") in ("inProgress", "halted") for r in releases):
        raise ValueError("Resolve the staged or halted production release in Play Console first")
    matching = [r for r in releases if str(code) in r.get("versionCodes", [])]
    if matching:
        if len(matching) == 1 and matching[0].get("status") == "completed":
            return "already-submitted"
        raise ValueError("This version has an unexpected production state; check Play Console")
    if any(r.get("status") != "completed" for r in releases):
        raise ValueError("Production has another pending release; check Play Console")
    return "submit"


def promote(directory, code, certificate):
    metadata, notes = load_artifact(directory)
    if metadata["versionCode"] != code or metadata["uploadCertificateSha256"] != certificate.replace(":", "").lower():
        raise ValueError("Artifact version or upload certificate mismatch")
    with Edit() as edit:
        existing = next((b for b in edit.get("/bundles").get("bundles", []) if b["versionCode"] == code), None)
        if not existing:
            raise ValueError(f"Play no longer lists bundle {code} (a newer internal build may have archived it). "
                             "Check App bundle explorer; do not substitute another version.")
        if existing.get("sha256", "").lower() != metadata["sha256"]:
            raise ValueError("Play holds different bytes for this version; refusing to submit")
        state = production_action(edit.get("/tracks").get("tracks", []), code)
        if state == "submit":
            edit.put("/tracks/production", {"track": "production", "releases": [release(metadata, notes, "completed")]})
            try:
                # Never cancel an existing Google review, and never leave changes unsent.
                edit.commit("changesNotSentForReview=false&changesInReviewBehavior=ERROR_IF_IN_REVIEW")
            except RuntimeError as error:
                if DRAFT_APP in str(error):
                    raise RuntimeError("Play accepts the first production release only from Console: Test and release "
                                       "→ Production → Create new release, add this existing bundle and send it for "
                                       "review. Then rerun this workflow to confirm it.") from None
                raise
            state = "submitted"
    lifecycle = next((r.get("releaseLifecycleState", "UNKNOWN") for r in
                      api("GET", "/tracks/production/releases").get("releases", [])
                      if any(a["versionCode"] == code for a in r.get("activeArtifacts", []))), "UNKNOWN")
    print(f"Production {metadata['versionName']} ({code}): {state}; Play lifecycle {lifecycle}")
    source = json.loads((directory / "source.json").read_text())["runUrl"]
    summary(f"### Production {metadata['versionName']} ({code}): {state}\n\nPlay lifecycle: `{lifecycle}`. "
            f"Google review still decides public availability.\n\nSource: {source}\n")
    return state


def summary(text):
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as stream:
            stream.write(text)


def output(**values):
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
            stream.writelines(f"{key}={value}\n" for key, value in values.items())


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("internal").add_argument("directory", type=Path)
    selection = commands.add_parser("select")
    selection.add_argument("version")
    selection.add_argument("directory", type=Path)
    promotion = commands.add_parser("promote")
    promotion.add_argument("directory", type=Path)
    promotion.add_argument("version_code", type=int)
    args = parser.parse_args()
    if args.command == "internal":
        publish_internal(args.directory, os.environ.get("PLAY_RELEASE_STATUS", "completed"))
    elif args.command == "select":
        select(args.version, args.directory)
    else:
        promote(args.directory, args.version_code, os.environ["ANDROID_UPLOAD_CERTIFICATE_SHA256"])

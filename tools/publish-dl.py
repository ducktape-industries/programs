#!/usr/bin/env python3
"""Publish one build of this commit to https://dl.ducktape.industries/programs/<channel>/
(the R2 bucket behind it), in two halves, so the machine that builds never holds the bucket's
keys:

   publish-dl.py stage <channel> <profile> <stage> <file>...   (run by `make dl-stage`)
   publish-dl.py upload <stage>          (the release workflow's upload job)

`stage` runs in the checkout that built the files. It copies them into <stage>, a fresh
directory, writes <stage>/manifest.json, and prints the upload order; it needs no
credentials, which makes it the dry run too. `upload` checks every file in <stage> against
the manifest and uploads them under <N>/ by their names, then <N>/manifest.json, then
latest.json, a copy of the newest version's manifest. It needs `aws` and the bucket
R2_BUCKET of the Cloudflare account R2_ACCOUNT_ID, with the bucket's keys in
AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY and AWS_DEFAULT_REGION=auto.

A channel is a branch and publishes one profile's bytes: dev the fast profile's, main the
release profile's. N, the version, is the commit's first-parent count (`git rev-list --count
--first-parent HEAD`): git alone gives it, it grows by one with each merge into the
channel's branch, and a number a cancelled run skipped stays unused. Everything under <N>/
is immutable: a file the bucket holds with the same sha256 is skipped, and one it holds with
another sha256 stops the upload before anything is uploaded. latest.json goes last and never
moves back to a smaller N.
"""

import datetime
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys

REPO = "programs"
PROFILES = {"dev": "fast", "main": "release"}
IMMUTABLE = "public, max-age=31536000, immutable"
LATEST = "public, max-age=60"
# view_wire::manifest: the section every view and notifier carries, and the place of the
# wire id among its lines (`LINES`)
MANIFEST_SECTION = b"ducktape.view.manifest"
WIRE_ID_LINE = 5
USAGE = "usage: publish-dl.py stage <channel> <profile> <stage> <file>... | upload <stage>"


def fail(message):
    sys.exit(f"publish-dl: {message}")


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout.strip()


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def leb128(data, at):
    value = shift = 0
    while True:
        byte = data[at]
        at += 1
        value |= (byte & 0x7F) << shift
        shift += 7
        if byte < 0x80:
            return value, at


def wire_id(data):
    """The wire id in a wasm module's view manifest section, or None for a module without one."""
    if data[:8] != b"\0asm\x01\0\0\0":
        return None
    at = 8
    while at < len(data):
        size, start = leb128(data, at + 1)
        if data[at] == 0:
            length, name = leb128(data, start)
            if data[name : name + length] == MANIFEST_SECTION:
                return data[name + length : start + size].decode().split("\n")[WIRE_ID_LINE]
        at = start + size
    return None


def check_channel(channel, profile):
    if channel not in PROFILES:
        fail(f"a channel is one of {', '.join(PROFILES)}, not {channel}")
    if profile != PROFILES[channel]:
        fail(f"{channel} publishes the {PROFILES[channel]} profile's bytes, not {profile}'s")


def manifest(channel, profile, files):
    commit = git("rev-parse", "HEAD")
    version = int(git("rev-list", "--count", "--first-parent", "HEAD"))
    committed = int(git("show", "-s", "--format=%ct", "HEAD"))
    artifacts, wire_ids = {}, set()
    for path in files:
        if path.name in artifacts or path.name == "manifest.json":
            fail(f"two files would be named {path.name}")
        data = path.read_bytes()
        artifacts[path.name] = {"sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
        if (speaks := wire_id(data)) is not None:
            wire_ids.add(speaks)
    if len(wire_ids) != 1:
        fail(f"the views must speak one wire, these speak {sorted(wire_ids) or 'none'}")
    return {
        "repo": REPO,
        "channel": channel,
        "version": version,
        "display": f"{channel}-{version}",
        "commit": commit,
        "profile": profile,
        "built_at": datetime.datetime.fromtimestamp(committed, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "wire_id": wire_ids.pop(),
        "artifacts": artifacts,
    }


def order(published):
    """Each key under the bucket, in upload order: every file, the version's manifest, latest.json."""
    prefix = f"{REPO}/{published['channel']}"
    names = [*published["artifacts"], "manifest.json"]
    return [(name, f"{prefix}/{published['version']}/{name}") for name in names], f"{prefix}/latest.json"


def stage(channel, profile, out, files):
    check_channel(channel, profile)
    if git("rev-parse", "--is-shallow-repository") == "true":
        fail("a shallow clone counts the wrong version: fetch the whole history")
    if git("status", "--porcelain", "--untracked-files=no"):
        fail("the checkout has uncommitted changes")
    files = sorted((pathlib.Path(file) for file in files), key=lambda path: path.name)
    published = manifest(channel, profile, files)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    for path in files:
        shutil.copyfile(path, out / path.name)
    (out / "manifest.json").write_text(json.dumps(published, indent=2) + "\n")
    print(f"{published['display']}: {out}")
    immutable, latest = order(published)
    for _, key in immutable:
        print(f"{key}  {IMMUTABLE}")
    print(f"{latest}  {LATEST}")


class Bucket:
    """The bucket, through `aws`: each object carries its sha256 as metadata."""

    def __init__(self):
        account, self.name = os.environ.get("R2_ACCOUNT_ID"), os.environ.get("R2_BUCKET")
        if not account or not self.name:
            fail("R2_ACCOUNT_ID and R2_BUCKET name the bucket")
        self.endpoint = f"https://{account}.r2.cloudflarestorage.com"

    def aws(self, *args):
        return subprocess.run(["aws", *args, "--endpoint-url", self.endpoint], capture_output=True)

    def sha256(self, key):
        """The sha256 the object at key was uploaded with ('' for none), or None when there is none."""
        run = self.aws("s3api", "head-object", "--bucket", self.name, "--key", key, "--output", "json")
        if run.returncode:
            if b"(404)" in run.stderr:
                return None
            fail(f"{key}: {run.stderr.decode().strip()}")
        return json.loads(run.stdout).get("Metadata", {}).get("sha256", "")

    def get(self, key):
        run = self.aws("s3", "cp", f"s3://{self.name}/{key}", "-")
        if run.returncode:
            fail(f"{key}: {run.stderr.decode().strip()}")
        return run.stdout

    def put(self, path, key, cache, sha256):
        options = ["--cache-control", cache, "--metadata", f"sha256={sha256}", "--no-progress"]
        run = self.aws("s3", "cp", str(path), f"s3://{self.name}/{key}", *options)
        if run.returncode:
            fail(f"{key}: {run.stderr.decode().strip()}")
        print(f"put {key}")


def upload(out):
    published = json.loads((out / "manifest.json").read_text())
    if published["repo"] != REPO:
        fail(f"{out} is a stage of {published['repo']}, not {REPO}")
    check_channel(published["channel"], published["profile"])
    held = {path.name for path in out.iterdir()} - {"manifest.json"}
    if held != set(published["artifacts"]):
        fail(f"{out} holds {sorted(held ^ set(published['artifacts']))} the manifest does not, or lacks them")
    for name, artifact in published["artifacts"].items():
        path = out / name
        if path.stat().st_size != artifact["bytes"] or sha256(path) != artifact["sha256"]:
            fail(f"{path} is not the file the manifest names")
    immutable, latest = order(published)
    hashes = {name: artifact["sha256"] for name, artifact in published["artifacts"].items()}
    hashes["manifest.json"] = sha256(out / "manifest.json")

    bucket = Bucket()
    missing, clashes = [], []
    for name, key in immutable:
        stored = bucket.sha256(key)
        if stored is None:
            missing.append((name, key))
        elif stored == hashes[name]:
            print(f"skip {key} (the same sha256)")
        else:
            clashes.append(f"{key} holds sha256 {stored or 'unrecorded'}, this build's is {hashes[name]}")
    if clashes:
        fail(f"{published['display']} is published with other bytes, nothing uploaded:\n  " + "\n  ".join(clashes))
    for name, key in missing:
        bucket.put(out / name, key, IMMUTABLE, hashes[name])
    if bucket.sha256(latest) is not None:
        newest = json.loads(bucket.get(latest))["version"]
        if newest > published["version"]:
            print(f"{latest} stays at {published['channel']}-{newest}, newer than {published['display']}")
            return
    bucket.put(out / "manifest.json", latest, LATEST, hashes["manifest.json"])


def main():
    mode, args = sys.argv[1:2], sys.argv[2:]
    if mode == ["stage"] and len(args) >= 4:
        stage(args[0], args[1], pathlib.Path(args[2]), args[3:])
    elif mode == ["upload"] and len(args) == 1:
        upload(pathlib.Path(args[0]))
    else:
        fail(USAGE)


if __name__ == "__main__":
    main()

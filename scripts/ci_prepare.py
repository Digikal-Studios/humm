#!/usr/bin/env python3
"""Check provisioned Apple host tools and fetch pinned public CI fixtures.

No package installation, model download, Rust default change, or home-directory
fixture path is performed here. See docs/ci.md for host provisioning.
"""

import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request


BUZZ_COMMIT = "aaf98634cd354be207e00cc5aaa1bfbd9e19cf22"  # official v1.4.5
FIXTURES = {
    "ggml-tiny.bin": (575451, "1351aeba9b17d782f15baf46716c34dcfba0a30c"),
    "whisper-latvian.wav": (230172, "d62d5937d0d62c748572d7bc3a35c88fa51db853"),
}


def verify_fixture(data, name, size, blob_sha):
    """Verify the exact Git object, including its type/length header."""
    digest = hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()
    if len(data) != size or digest != blob_sha:
        raise RuntimeError(f"Buzz fixture size or Git blob mismatch: {name}")


def provision_fixtures(runner_temp):
    """Download only the two ordinary-test fixtures into this job's temp tree."""
    directory = Path(tempfile.mkdtemp(prefix="humm-buzz-", dir=runner_temp))
    for name, (size, blob_sha) in FIXTURES.items():
        url = f"https://raw.githubusercontent.com/chidiwilliams/buzz/{BUZZ_COMMIT}/testdata/{name}"
        with urllib.request.urlopen(url, timeout=30) as response:
            data = response.read(size + 1)
        verify_fixture(data, name, size, blob_sha)
        (directory / name).write_bytes(data)
        print(f"Verified Buzz {name}: {size} bytes, Git blob {blob_sha}")
    return directory


def check_host_tools():
    """Assert dependencies rather than mutating the shared Homebrew installation."""
    for command in ("brew", "cmake", "ffmpeg", "ffprobe", "pkg-config"):
        if shutil.which(command) is None:
            raise RuntimeError(f"Missing host command {command}; provision the dependencies in docs/ci.md")
    prefixes = {}
    for formula, library in (("openblas", "libopenblas.dylib"), ("libomp", "libomp.dylib")):
        result = subprocess.run(["brew", "--prefix", formula], check=True, text=True, capture_output=True)
        prefix = Path(result.stdout.strip())
        if not prefix.is_absolute() or not (prefix / "lib" / library).is_file():
            raise RuntimeError(f"Missing Homebrew {formula} runtime; provision it before CI")
        prefixes[formula] = prefix
    env = dict(os.environ)
    exports = {}
    if subprocess.run(["pkg-config", "--exists", "openblas"], env=env).returncode:
        pc_dir = prefixes["openblas"] / "lib" / "pkgconfig"
        previous = env.get("PKG_CONFIG_PATH", "")
        env["PKG_CONFIG_PATH"] = str(pc_dir) + (os.pathsep + previous if previous else "")
        if subprocess.run(["pkg-config", "--exists", "openblas"], env=env).returncode:
            raise RuntimeError("OpenBLAS pkg-config discovery failed; check the installed formula")
        exports["PKG_CONFIG_PATH"] = env["PKG_CONFIG_PATH"]
    return exports


def main():
    runner_temp = os.environ.get("RUNNER_TEMP")
    github_env = os.environ.get("GITHUB_ENV")
    if not runner_temp or not Path(runner_temp).is_dir() or not github_env:
        raise RuntimeError("RUNNER_TEMP and GITHUB_ENV must identify this CI job's temporary environment")
    exports = check_host_tools()
    exports["HUMM_BUZZ_TESTDATA"] = str(provision_fixtures(runner_temp))
    if any("\n" in value or "\r" in value for value in exports.values()):
        raise RuntimeError("Cannot export a multiline CI environment path")
    with open(github_env, "a", encoding="utf-8") as stream:
        for name, value in exports.items():
            stream.write(f"{name}={value}\n")
    print(f"Prepared ordinary-test fixtures from official Buzz commit {BUZZ_COMMIT}")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"CI prerequisite preparation failed: {error}", file=sys.stderr)
        sys.exit(1)

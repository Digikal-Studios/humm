#!/usr/bin/env python3
"""Ensure macOS CI host tools exist and fetch pinned public Buzz fixtures.

On GitHub-hosted macOS runners, missing Homebrew formulae are installed into
the ephemeral image. On a pre-provisioned host the same checks succeed without
mutating already-present formulae. See docs/ci.md.
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

# openblas / libomp are ADR-024 link requirements (absolute Homebrew dylib paths).
# cmake / ffmpeg / pkgconf supply the remaining host commands named in docs/ci.md.
DYLIB_FORMULAE = (
    ("openblas", "libopenblas.dylib"),
    ("libomp", "libomp.dylib"),
)
COMMAND_FORMULAE = (
    ("cmake", "cmake"),
    ("ffmpeg", "ffmpeg"),  # also provides ffprobe
    ("pkgconf", "pkg-config"),
)
HOST_COMMANDS = ("brew", "cmake", "ffmpeg", "ffprobe", "pkg-config")


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


def brew_prefix(formula):
    """Return the Homebrew prefix path for a formula."""
    result = subprocess.run(
        ["brew", "--prefix", formula], check=True, text=True, capture_output=True
    )
    return Path(result.stdout.strip())


def dylib_ready(formula, library):
    """True when brew --prefix points at an absolute path that contains library."""
    try:
        prefix = brew_prefix(formula)
    except subprocess.CalledProcessError:
        return False
    return prefix.is_absolute() and (prefix / "lib" / library).is_file()


def brew_install(formulae):
    """Install the named formulae without auto-updating Homebrew."""
    print(f"Installing missing Homebrew formulae: {' '.join(formulae)}")
    env = dict(os.environ)
    env.setdefault("HOMEBREW_NO_AUTO_UPDATE", "1")
    env.setdefault("HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK", "1")
    subprocess.run(["brew", "install", *formulae], check=True, env=env)


def ensure_formulae():
    """Install any missing Homebrew formulae needed by the workspace build."""
    if shutil.which("brew") is None:
        raise RuntimeError(
            "Missing host command brew; GitHub-hosted macOS images include Homebrew"
        )
    missing = []
    for formula, library in DYLIB_FORMULAE:
        if not dylib_ready(formula, library):
            missing.append(formula)
    for formula, command in COMMAND_FORMULAE:
        if shutil.which(command) is None:
            missing.append(formula)
    # Deduplicate while preserving order (ffmpeg covers ffprobe).
    ordered = list(dict.fromkeys(missing))
    if ordered:
        brew_install(ordered)
    if shutil.which("ffprobe") is None:
        # Extremely unlikely after ffmpeg, but keep the failure actionable.
        raise RuntimeError("ffprobe missing after ffmpeg install; check the Homebrew formula")


def check_host_tools():
    """Assert required commands and OpenBLAS/libomp dylibs after any brew install."""
    ensure_formulae()
    for command in HOST_COMMANDS:
        if shutil.which(command) is None:
            raise RuntimeError(
                f"Missing host command {command} after brew install; see docs/ci.md"
            )
    prefixes = {}
    for formula, library in DYLIB_FORMULAE:
        prefix = brew_prefix(formula)
        if not prefix.is_absolute() or not (prefix / "lib" / library).is_file():
            raise RuntimeError(f"Missing Homebrew {formula} runtime after brew install")
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
        raise RuntimeError(
            "RUNNER_TEMP and GITHUB_ENV must identify this CI job's temporary environment"
        )
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

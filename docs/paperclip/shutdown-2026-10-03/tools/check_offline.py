#!/usr/bin/env python3
"""Check that a Paperclip shutdown archive folder works without Paperclip.

Usage:
  check_offline.py ARCHIVE_DIR [--complete] [--large-root DIR] [--evidence-root DIR]
  check_offline.py --self-test                      prove the checks fail on broken input

For every Markdown file under ARCHIVE_DIR, outside fenced and inline code, it reads
inline links and images, reference-link definitions, autolinks and HTML href/src
attributes, then:

- fails on any link to a Paperclip host, any absolute path (/DIG/..., /api/...) and any
  agent:// or other non-web scheme: these only work while Paperclip runs;
- fails on a bare Paperclip URL that would render as a clickable autolink;
- resolves every relative link to a file inside the repository and fails if it is missing;
- resolves every #fragment against <a id>, id="..." and heading slugs in the target
  Markdown file and fails if the anchor is missing;
- skips the attachments/ folder when reading links: those files are byte-identical copies
  of uploaded artifacts, so links inside them are historical content; instead it
- re-hashes every file listed in data/*.sha256 (paths relative to ARCHIVE_DIR) and fails
  on a missing file or a hash mismatch. Manifests for files kept on another branch are
  checked when --large-root points at a checkout of that branch's archive folder
  (large-*.sha256, run-logs.sha256) or --evidence-root points at a checkout of the ref
  that holds the files in local-evidence.sha256. With --complete, a manifest that cannot
  be checked is a failure; without it, skipped manifests are reported and the run is
  labelled partial;
- fails on an erased citation: text shown as "(historical link `...`)" or "(anchor `...`
  not in the archived record)" must be listed, with an allowed reason, in
  data/unresolved-links.tsv, and every row there must name a known reason;
- checks every attachment location in data/issues.json: the branch must be the docs
  branch or the large-files branch, and for this repository the bytes at
  <folder>/<path> must exist with the recorded SHA-256 (large files need --large-root).

External web links (for example the Git host) are counted, not fetched.
Exit status 0 means no failure; 1 means at least one failure (listed on stdout).
"""
import collections, hashlib, os, re, subprocess, sys, tempfile

PAPERCLIP_HOSTS = ("paperclip.digikal.com",)
FENCE = re.compile(r"^\s*(```|~~~)")
INLINE = re.compile(r"(!?\[(?:[^\[\]\n]|\[[^\[\]\n]*\])*\])\(\s*(<[^>\n]*>|[^)\s]+)(?:\s+(?:\"[^\"\n]*\"|'[^'\n]*'))?\s*\)")
REFDEF = re.compile(r"^ {0,3}\[[^\]\n]+\]:\s*(\S+)", re.M)
AUTOLINK = re.compile(r"<((?:https?|ftp)://[^>\s]+)>")
HREF = re.compile(r"\b(?:href|src)\s*=\s*(?:\"([^\"]*)\"|'([^']*)'|([^\s\"'=<>`]+))", re.I)
BARE = re.compile(r"https?://(?:" + "|".join(re.escape(h) for h in PAPERCLIP_HOSTS) + r")\S*")
ANCHOR_ID = re.compile(r"""\bid\s*=\s*["']([^"']+)["']""")
HEADING = re.compile(r"^#{1,6}\s+(.*?)\s*#*\s*$")


def strip_code(text):
    """Return text with fenced blocks and inline code spans blanked out."""
    out, infence = [], False
    for line in text.splitlines():
        if FENCE.match(line):
            infence = not infence
            out.append("")
            continue
        out.append("" if infence else line)
    text = "\n".join(out)
    # Code spans never cross a blank line or a table row, so pair backticks per block.
    return "".join(blank_spans(block) for block in re.split(r"(\n[ \t]*\n|\n(?=[ \t]*\|))", text))


def blank_spans(text):
    res, i = [], 0
    while i < len(text):
        if text[i] == "`":
            j = i
            while j < len(text) and text[j] == "`":
                j += 1
            run = text[i:j]
            k = text.find(run, j)
            while k != -1 and k + len(run) < len(text) and text[k + len(run)] == "`":
                k = text.find(run, k + len(run) + 1)
            if k == -1:
                res.append(run)
                i = j
                continue
            res.append(" " * (k + len(run) - i))
            i = k + len(run)
        else:
            res.append(text[i])
            i += 1
    return "".join(res)


def slug(h):
    h = re.sub(r"<[^>]+>", "", h).strip().lower()
    h = re.sub(r"[^\w\- ]", "", h)
    return h.replace(" ", "-")


_ANCHORS = {}


def anchors(path):
    if path not in _ANCHORS:
        s = set()
        try:
            text = open(path, encoding="utf-8", errors="replace").read()
        except OSError:
            text = ""
        s |= set(ANCHOR_ID.findall(text))
        seen = collections.Counter()
        infence = False
        for line in text.splitlines():
            if FENCE.match(line):
                infence = not infence
                continue
            m = None if infence else HEADING.match(line)
            if m:
                base = slug(m.group(1))
                n = seen[base]
                seen[base] += 1
                s.add(base if n == 0 else f"{base}-{n}")
        _ANCHORS[path] = s
    return _ANCHORS[path]


def repo_root(start):
    r = subprocess.run(["git", "-C", start, "rev-parse", "--show-toplevel"], capture_output=True, text=True)
    return os.path.realpath(r.stdout.strip()) if r.returncode == 0 else os.path.realpath(start)


DOCS_BRANCH = "docs/paperclip-shutdown-2026-10-03"
LARGE_BRANCH = "archive/paperclip-shutdown-2026-10-03-large-attachments"
LEDGER_REASONS = {"paperclip-ui-page", "target-absent-in-source", "placeholder", "ambiguous-relative-path",
                  "unknown-relative-path", "fragment-without-context"}
ERASED = re.compile(r"\((?:historical link|source link) `([^`\n]+)`\)|\(anchor `#([^`\n]+)` not in the archived record\)")


def no_fences(text):
    out, infence = [], False
    for line in text.splitlines():
        if FENCE.match(line):
            infence = not infence
            out.append("")
            continue
        out.append("" if infence else line)
    return "\n".join(out)


def check(archive, large_root=None, evidence_root=None, complete=False):
    archive = os.path.realpath(archive)
    root = repo_root(archive)
    fails, stats = [], collections.Counter()

    def fail(f, kind, target):
        fails.append((os.path.relpath(f, archive), kind, target))

    ledger = collections.defaultdict(set)
    lp = os.path.join(archive, "data", "unresolved-links.tsv")
    if os.path.exists(lp):
        for n, line in enumerate(open(lp, encoding="utf-8")):
            parts = line.rstrip("\n").split("\t")
            if n == 0 or not line.strip():
                continue
            if len(parts) != 4 or parts[3] not in LEDGER_REASONS:
                fail(lp, "bad-ledger-row", line.strip())
                continue
            ledger[parts[0]].add(parts[2])

    for dp, dn, fn in os.walk(archive):
        # attachments/ holds byte-identical copies of uploaded files (hash-checked below);
        # links inside them are historical content, not archive navigation.
        dn[:] = [d for d in dn if d != ".git" and not (dp == archive and d == "attachments")]
        for name in fn:
            if not name.endswith(".md"):
                continue
            f = os.path.join(dp, name)
            raw = open(f, encoding="utf-8", errors="replace").read()
            rel = os.path.relpath(f, archive)
            if dp != archive or name not in ("setup.md",):
                for m in ERASED.finditer(no_fences(raw)):
                    stats["erased-citations"] += 1
                    t = m.group(1)
                    if m.group(2) is not None:
                        # an issue link kept, with the cited anchor named as absent
                        if not any(x.endswith("#" + m.group(2)) for x in ledger.get(rel, ())):
                            fail(f, "erased-citation", "#" + m.group(2))
                    elif t not in ledger.get(rel, ()):
                        fail(f, "erased-citation", t)
            text = strip_code(raw)
            targets = [m.group(2).strip("<>") for m in INLINE.finditer(text)]
            targets += REFDEF.findall(text) + AUTOLINK.findall(text)
            targets += [a or b or c for a, b, c in HREF.findall(text)]
            masked = INLINE.sub(" ", text)
            masked = AUTOLINK.sub(" ", masked)
            masked = HREF.sub(" ", masked)
            masked = REFDEF.sub(" ", masked)
            for m in BARE.finditer(masked):
                stats["fail"] += 1
                fail(f, "bare-paperclip-url", m.group(0))
            for t in targets:
                stats["links"] += 1
                low = t.lower()
                if re.match(r"^[a-z][a-z0-9+.\-]*:", low):
                    if low.startswith(("http://", "https://")):
                        host = re.sub(r"^https?://", "", low).split("/")[0].split(":")[0]
                        if host in PAPERCLIP_HOSTS:
                            fail(f, "paperclip-link", t)
                        else:
                            stats["external"] += 1
                    elif low.startswith(("mailto:", "tel:")):
                        stats["external"] += 1
                    else:
                        fail(f, "non-web-scheme", t)
                    continue
                if t.startswith("/"):
                    fail(f, "absolute-path", t)
                    continue
                path, _, frag = t.partition("#")
                target = os.path.realpath(os.path.join(dp, path)) if path else f
                if not (target == root or target.startswith(root + os.sep)):
                    fail(f, "outside-repository", t)
                    continue
                if not os.path.exists(target):
                    fail(f, "missing-file", t)
                    continue
                stats["local"] += 1
                if frag:
                    if not target.endswith(".md"):
                        stats["fragment-on-non-markdown"] += 1
                    elif frag not in anchors(target):
                        fail(f, "missing-anchor", t)
                    else:
                        stats["anchors"] += 1
    data = os.path.join(archive, "data")
    for man in sorted(os.listdir(data)) if os.path.isdir(data) else []:
        if not man.endswith(".sha256"):
            continue
        base = archive
        need = None
        if man.startswith("large-") or man == "run-logs.sha256":
            need, base = "--large-root", large_root
        elif man == "local-evidence.sha256":
            need, base = "--evidence-root", evidence_root
        if need and not base:
            if complete:
                fail(os.path.join(data, man), "auxiliary-root-required", need)
            else:
                stats["manifests-on-other-branch-skipped"] += 1
            continue
        for line in open(os.path.join(data, man)):
            if not line.strip():
                continue
            h, rel = line.rstrip("\n").split("  ", 1)
            p = os.path.join(base, rel)
            stats["hashed"] += 1
            if not os.path.exists(p):
                fail(os.path.join(data, man), "missing-artifact", rel)
            elif hashlib.sha256(open(p, "rb").read()).hexdigest() != h:
                fail(os.path.join(data, man), "hash-mismatch", rel)
    check_locations(archive, large_root, complete, fail, stats)
    return fails, stats


def check_locations(archive, large_root, complete, fail, stats):
    """Every attachment location in data/issues.json must hold the recorded bytes."""
    import json
    p = os.path.join(archive, "data", "issues.json")
    if not os.path.exists(p):
        return
    d = json.load(open(p))
    me = d.get("repository")
    for rec in d.get("issues", []):
        for a in rec.get("attachments", []):
            stats["attachment-locations"] += 1
            br = a.get("branch")
            if br not in (DOCS_BRANCH, LARGE_BRANCH):
                fail(p, "wrong-attachment-branch", f"{rec['identifier']} {a.get('path')} on {br}")
                continue
            if a.get("repository") != me:
                continue
            if br == LARGE_BRANCH:
                if not large_root:
                    if complete:
                        fail(p, "auxiliary-root-required", f"--large-root for {a.get('path')}")
                    continue
                f = os.path.join(large_root, a["path"])
            else:
                f = os.path.join(archive, a["path"])
            if not os.path.exists(f):
                fail(p, "missing-artifact", f"{br}:{a['path']}")
            elif hashlib.sha256(open(f, "rb").read()).hexdigest() != a.get("sha256"):
                fail(p, "hash-mismatch", f"{br}:{a['path']}")


def self_test():
    """Each broken case must produce its failure kind; the clean case must pass."""
    cases = {
        "missing-file": "[x](issues/DIG-404.md)\n",
        "missing-anchor": "[x](ok.md#comment-missing)\n",
        "paperclip-link": "[x](https://paperclip.digikal.com/DIG/issues/DIG-1)\n",
        "absolute-path": "[x](/DIG/issues/DIG-1#document-plan)\n",
        "non-web-scheme": "[x](agent://abc)\n",
        "bare-paperclip-url": "See https://paperclip.digikal.com/DIG/issues/DIG-1 for details.\n",
        "missing-artifact": None,
        "hash-mismatch": None,
        "single-quoted-html": None,
        "unquoted-html": None,
        "erased-citation": "See review (historical link `/DIG/issues/DIG-1#document-review`).\n",
        "missing-anchor-annotation": "[x](ok.md) (anchor `#document-gone` not in the archived record)\n",
        "wrong-attachment-branch": None,
        "auxiliary-root-required": None,
        "bad-ledger-row": None,
    }
    expect = {"single-quoted-html": "missing-file", "unquoted-html": "missing-file",
              "missing-anchor-annotation": "erased-citation"}
    ok = True
    for kind, body in cases.items():
        with tempfile.TemporaryDirectory() as d:
            subprocess.run(["git", "init", "-q", d], check=True)
            a = os.path.join(d, "archive")
            os.makedirs(os.path.join(a, "data"))
            open(os.path.join(a, "ok.md"), "w").write('<a id="comment-1"></a>\n\n# Title\n\n[self](ok.md#title) [c](ok.md#comment-1)\n`[code](/DIG/x)`\n')
            if body:
                open(os.path.join(a, "case.md"), "w").write(body)
            if kind == "single-quoted-html":
                open(os.path.join(a, "case.md"), "w").write("<a href='missing.md'>missing evidence</a>\n")
            if kind == "unquoted-html":
                open(os.path.join(a, "case.md"), "w").write("<img src=missing.png>\n")
            if kind == "wrong-attachment-branch":
                import json
                os.makedirs(os.path.join(a, "attachments"))
                open(os.path.join(a, "attachments", "f.txt"), "w").write("x")
                h = hashlib.sha256(b"x").hexdigest()
                json.dump({"repository": "r", "issues": [{"identifier": "DIG-1", "attachments": [
                    {"repository": "r", "branch": "main", "path": "attachments/f.txt", "sha256": h}]}]},
                    open(os.path.join(a, "data", "issues.json"), "w"))
            if kind == "auxiliary-root-required":
                open(os.path.join(a, "data", "large-attachments.sha256"), "w").write("0" * 64 + "  attachments/big.png\n")
            if kind == "bad-ledger-row":
                open(os.path.join(a, "data", "unresolved-links.tsv"), "w").write(
                    "file\tsource_issue\toriginal_target\treason\ncase.md\tDIG-1\tx\tlooked-fine\n")
            if kind == "missing-artifact":
                open(os.path.join(a, "data", "attachments.sha256"), "w").write("0" * 64 + "  attachments/gone.png\n")
            if kind == "hash-mismatch":
                os.makedirs(os.path.join(a, "attachments"))
                open(os.path.join(a, "attachments", "f.txt"), "w").write("changed")
                open(os.path.join(a, "data", "attachments.sha256"), "w").write("0" * 64 + "  attachments/f.txt\n")
            fails, _ = check(a, complete=kind == "auxiliary-root-required")
            got = {k for _, k, _ in fails}
            res = "PASS" if got == {expect.get(kind, kind)} else "FAIL"
            ok &= res == "PASS"
            print(f"self-test {kind}: expected failure detected -> {res} {sorted(got)}")
    with tempfile.TemporaryDirectory() as d:
        subprocess.run(["git", "init", "-q", d], check=True)
        a = os.path.join(d, "archive")
        os.makedirs(a)
        os.makedirs(os.path.join(a, "data"))
        open(os.path.join(a, "ok.md"), "w").write('<a id="comment-1"></a>\n\n# Title\n\n[self](ok.md#title) [c](#comment-1) [web](https://repo.digikal.com/x)\n`https://paperclip.digikal.com/DIG/issues/DIG-1` is inert in code.\n'
                                                   "<a href='ok.md#title'>t</a> agents (historical link `/DIG/agents/x/inbox`)\n")
        open(os.path.join(a, "data", "unresolved-links.tsv"), "w").write(
            "file\tsource_issue\toriginal_target\treason\nok.md\tDIG-1\t/DIG/agents/x/inbox\tpaperclip-ui-page\n")
        os.makedirs(os.path.join(a, "attachments"))
        open(os.path.join(a, "attachments", "f.txt"), "w").write("x")
        import json
        json.dump({"repository": "r", "issues": [{"identifier": "DIG-1", "attachments": [
            {"repository": "r", "branch": DOCS_BRANCH, "path": "attachments/f.txt",
             "sha256": hashlib.sha256(b"x").hexdigest()}]}]}, open(os.path.join(a, "data", "issues.json"), "w"))
        fails, _ = check(a, complete=True)
        res = "PASS" if not fails else "FAIL"
        ok &= not fails
        print(f"self-test clean archive: no failures -> {res} {fails}")
    return ok


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        sys.exit(0 if self_test() else 1)
    if not sys.argv[1:]:
        sys.exit(__doc__)
    opt = lambda name: sys.argv[sys.argv.index(name) + 1] if name in sys.argv else None
    complete = "--complete" in sys.argv
    fails, stats = check(sys.argv[1], opt("--large-root"), opt("--evidence-root"), complete)
    for f, kind, t in fails:
        print(f"FAIL {kind}: {f}: {t}")
    mode = "complete" if complete else ("partial: " + str(stats["manifests-on-other-branch-skipped"])
                                       + " manifest(s) on other refs not checked; use --complete"
                                       if stats["manifests-on-other-branch-skipped"] else "complete (no other refs needed)")
    print(f"mode: {mode}")
    print("summary: " + ", ".join(f"{k} {v}" for k, v in sorted(stats.items())) + f", failures {len(fails)}")
    sys.exit(1 if fails else 0)

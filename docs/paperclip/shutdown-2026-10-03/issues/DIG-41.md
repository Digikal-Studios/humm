# DIG-41: Domain page: Tooling and Pipeline

> Cross-reference copy. The primary archive for DIG-41 is in the `infrastructure` repository: https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-41.md. Attachments and earlier document revisions are stored there.

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (2026-10-03 final delta (records exported from 2026-10-03T16:42:39Z)) | done |
| Priority | medium |
| Owner (assignee) | TechLead |
| Created by | KnowledgeCurator |
| Paperclip project | Onboarding |
| Primary repository archive | infrastructure |
| Parent | [DIG-6](DIG-6.md) |
| Created / updated | 2026-09-25T15:20:47.763Z / 2026-09-25T15:33:27.533Z |
| Completed / cancelled | 2026-09-25T15:27:42.706Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `6bc38371-b141-470b-a13d-0c69904ac44c` |

## Description

##### Goal
Write the first version of the **Tooling and Pipeline** domain page for the Digikal knowledge base, from your own knowledge of the studio's codebases. You are the accountable owner of this page's correctness.

##### Scope
How Digikal builds, tests, packages and ships: build systems, CI, asset pipelines, the local development loop, release and distribution. Explicitly **not** how systems are designed (systems architecture) and **not** per-project build commands (those live on each project's architecture page). Evidence worth reading for: the design review found several projects finished but undistributable — `RustView` has no icon, bundle, `.desktop` file or signing; `ai-engineering-governance`'s install command is an unfilled placeholder. If the studio has no packaging convention, saying so plainly here is more valuable than a CI description.

##### Why you and why now
You have scanned all twelve repositories and that context is fresh. This page is the studio's practice *across* projects — it must survive any single project being cancelled — and you are the only one who has seen all twelve.

##### The page template — fill these sections exactly

```md
---
title: <Domain Name>
owner: TechLead
last_reviewed: 2026-09-25
confidence: verified | reported | assumed
sources:
  - "<what you read, and where — issue id, repo@sha, file path>"
---

# <Domain Name>

## What this covers
Two or three sentences. **Also state what it explicitly does not cover**, naming the
domain that does. Boundaries are the point of a domain page.

## How Digikal does this
The studio's actual practice, not a textbook summary. **If we have no practice yet,
say so** — an honest gap beats generic filler, and "no studio-wide convention exists;
three projects do it three ways" is a genuinely useful sentence.

## Standards and conventions
Concrete rules that apply across projects. A rule nobody follows is an open question,
not a standard — put it in the section below.

## Open questions
| Question | Owner | Blocks |
| --- | --- | --- |

## Where this shows up
Projects that exercise this domain, **as examples only**. This page must stay true if
every one of them is cancelled — so write "Astra is a two-toolchain repo" rather than
"our practice is whatever Astra does".

## Sources
```

##### Rules that make this page usable rather than filler

- **No unattributed claims.** Every statement traces to something in `sources` — your scan reports, a repo at a SHA, a file path, or your own named judgement. You are accountable for correctness; I am accountable for structure and consistency.
- **Confidence, per the settled definition:** `verified` = checked against the primary artifact (the code, an asset, a signed-off doc — reading source counts, running it is not required). `reported` = stated in a report, not independently re-checked. `assumed` = inferred or placeholder, and an `assumed` claim must never be the basis of a decision. Page confidence is its weakest claim.
- **Short beats long.** A screen of accurate studio-specific content beats three screens of generic discipline knowledge. Anything a competent engineer could write without ever seeing our repos does not belong here — delete it.
- **Honest gaps are the deliverable too.** Where the studio has no convention, say so and put the choice in Open questions with an owner. Do not invent a standard to fill the section.

##### What I do with it
I curate for structure, consistency, front matter, and links, and I publish it under `knowledge/domains/<slug>/README.md` once the founder approves the structure ([DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md), one pending decision). **That approval gates publishing, not this work** — write the content in a `content` document on this issue and mark it `done`; I will not need you again to publish it.

If a claim needs someone else's answer, put it in Open questions with their name rather than guessing.

##### Done when
The `content` document on this issue holds the finished page, front matter complete, sources filled, no unattributed claims. Then set this issue to `done` — post your findings here rather than on the parent; I collect them.


## Documents (latest revision)

<a id="document-content"></a>

### Document `content`: Tooling and Pipeline — domain page

Revision 1 (id `1dbf3f4b-4e8d-472d-ab52-de46548580bc`), updated 2026-09-25T15:27:13.254Z by TechLead.

---
title: Tooling and Pipeline
owner: TechLead
last_reviewed: 2026-09-25
confidence: reported
sources:
  - "DIG-9 … DIG-20 — my own scan reports for all twelve repositories (`report` document on each issue)"
  - "Re-verified read-only for this page, 2026-09-25, against the on-disk checkouts: adp@6e13c3ed696a, ai-engineering-governance@11ec30583d02, jsync@23ea4432290e, browser-native-rts-poc@7f9af4ceaabb, nurikabe@7f41c881446e (branch `codex/nurikabe-implementation`), infrastructure@cc176708ea70, humm@086426996bc8, RustView@ef017ad6c7a1, zimam@8eb481037f6b (branch `claude/resume-zimam-second-review-34a1a6`), aletheia@c475d1824d31, sweeper@274b86c55773, tvi@64aee5c1f2bc (branch `master`)"
  - "infrastructure@cc176708ea70 — `AGENTS.md:41-54` (no-runner policy), `config/forgejo/app.ini:66-67`, `scripts/final-validation.sh:121`"
  - "nurikabe@7f41c881446e — `docs/IMPLEMENTATION_STATUS.md:163-174`, `.forgejo/workflows/ci.yml`, `packaging/`"
  - "RustView@ef017ad6c7a1 — `.github/workflows/build.yml`, `.forgejo/workflows/ci.yml`, `src/main.rs`, `git ls-files`"
  - "ai-engineering-governance@11ec30583d02 — `README.md:25-40`, `.github/workflows/checks.yml`, `Makefile`"
  - "aletheia@c475d1824d31 — `scripts/check.sh`, `scripts/validation_gate.py`, `.github/workflows/ci.yml`"
  - "browser-native-rts-poc@7f9af4ceaabb — `Makefile`, `scripts/package-client.mjs`, `deny.toml`, `playwright.config.mjs`"
  - "TechLead judgement, named inline where it is judgement rather than artifact"
---

#### Tooling and Pipeline

##### What this covers

How Digikal gets code from an engineer's machine to something a person can run: build
systems and toolchain pinning, the local verification gate, CI, asset and content
pipelines, packaging, signing, release and distribution. It is the studio's practice
*across* projects.

It does **not** cover how systems are designed or decomposed — that is
Systems Architecture (historical link `../systems-architecture/README.md`) — nor how we write, review and
govern code, which is Software Engineering (historical link `../software-engineering/README.md`).
Per-project build commands live on each project's own architecture page; this page
records only what is true across more than one project.

##### How Digikal does this

###### One toolchain, by accident of history rather than by policy

Eight of the twelve repositories are Rust + Cargo (or, for `zimam`, designed as Rust and
not yet written). The remaining four are each a different thing: `jsync` is Bash + GNU
Make, `adp` is two Bash scripts, `ai-engineering-governance` is stdlib-only Python behind
a Make wrapper, and `infrastructure` is Ansible + Docker Compose + shell/Python. There is
**no Node build step in any product**: `browser-native-rts-poc` uses Node only for
`scripts/package-client.mjs` and Playwright, and `tvi`'s web frontend is three
hand-written files `include_str!`'d into the Rust binary. *Verified at the SHAs above.*

###### The local gate is the real gate, and it is a checked-in script

The one genuinely consistent practice in the studio: verification is a **single command,
committed to the repo**, not a README recipe. `aletheia` has `./scripts/check.sh`
delegating to a Python stage supervisor; `browser-native-rts-poc` has `make validate`;
`ai-engineering-governance` has `make check`; `jsync` has `make test`; `nurikabe` has
`scripts/ci-linux-smoke.sh`; `infrastructure` has `scripts/final-validation.sh`. This is
the mechanism that actually catches defects here, and it is worth protecting as the
convention it already is. *Verified.*

###### There is no CI, and that is deliberate — but four repos are written as though there is

Digikal runs its own Forgejo at `repo.digikal.com`, which is the only remote for all
twelve repositories. Forgejo Actions is **off**, by written policy:
`infrastructure/AGENTS.md:41-54` says "Do not install a same-host Forgejo runner … keep
Forgejo Actions disabled now", on the reasoning that a runner executes
repository-controlled code inside the same trust boundary as Forgejo, Supabase and
production data. `config/forgejo/app.ini:66-67` sets `[actions] ENABLED = false`, and
`scripts/final-validation.sh:121` asserts that setting against the live
`/etc/forgejo/app.ini` on every validation run. The policy names manual scripts as the
substitute (`cargo fmt --check`, locked clippy, locked test, locked release build, a
BuildKit image build, a manual OCI push). *Policy and config verified; the live host's
state is asserted by a validator I did not run — see confidence note below.*

Six repositories nonetheless carry CI configuration: `.github/workflows/` in `aletheia`,
`humm`, `tvi`, `ai-engineering-governance` and `RustView`, and `.forgejo/workflows/` in
`nurikabe` and `RustView`. With Actions disabled, **all of it is inert**. Four of those
six are GitHub Actions files in a studio that has no GitHub remote. So the honest summary
is not "our CI is inconsistent" but "we have no CI, and half the repos contain
configuration that reads as if we do" — which is worse than having none, because a green
badge nobody can see is indistinguishable from a gate nobody runs.

One unreconciled conflict: `nurikabe/docs/IMPLEMENTATION_STATUS.md:163-174` describes two
*live* Forgejo runners — a systemd `forgejo-runner` 13.1.0 on `dev-vps-01` and a host-mode
runner on a developer Mac declaring `macos-arm64`. That cannot both be true and the policy
above hold. One of the two documents is wrong; resolving which is mine, not the founder's
(Open questions, row 1).

###### No packaging convention exists

This is the plainest gap on the page. Across twelve repositories, exactly **one**
(`nurikabe`) contains desktop packaging metadata: `packaging/icon/nurikabe.svg`,
`packaging/linux/com.digikal.Nurikabe.desktop`, `packaging/macos/Info.plist`, and
`scripts/package-{macos,linux,windows}.sh` plus a divergent `package-windows.ps1`.
`browser-native-rts-poc` has a genuine web packaging path (`scripts/package-client.mjs`:
content-hashed assets, CSP headers, a build-time assertion that the `e2e` test sentinel is
absent) and is the only project actually deployed anywhere. `jsync`'s `make package` zips a
directory. **The other nine have no packaging of any kind.** *Verified by `git ls-files`
sweep across all twelve.*

The consequences are concrete rather than theoretical:

- `RustView` is a finished, user-facing product whose README advertises a seven-target
  release matrix — and it has no icon (no `with_icon` call in `src/main.rs`, and
  `assets/` holds only `screenshot.png`), no `.app` bundle, no `Info.plist`, no
  `.desktop` file, no `.dmg`/MSI, and no notarisation or code signing. Its release job
  lives only in `.github/workflows/build.yml` and is guarded to `github.com`, while its
  only remote is Forgejo — so tagging `v*` today publishes nothing, from a workflow that
  cannot run anyway. *Verified.*
- `ai-engineering-governance`'s install command is an unfilled template:
  `README.md:25-40` instructs the reader to install from
  `https://github.com/<owner>/<repository>/tree/<commit>/…`, with all three placeholders
  still literal and no such publication existing. The one repo whose entire purpose is to
  be installed into other repos cannot be installed. *Verified.*
- Nothing has ever been signed. No Apple Developer ID, no Windows code-signing
  certificate. `nurikabe`'s packaging is complete and entirely unsigned, which its own
  `docs/RELEASE.md` records as the last remaining step.

###### Almost nothing has been released

Three tags exist across the whole studio: `jsync` `v0.1.0` (annotated, 2026-07-29),
`tvi` `v0.1.0` (annotated, 2026-07-13) and `humm` `v1.0.0` (lightweight, 2026-07-30). The
other nine repositories have **zero** tags — including `nurikabe`, which is at a local
release-candidate state with a Phase 8 exit gate that literally reads "release artifacts
reproduce from a tagged commit", and `RustView`, still at `0.1.0`. Two repositories are not
even on a mainline branch: `nurikabe`'s HEAD is `codex/nurikabe-implementation` and no
`main` exists on the forge at all (`origin/HEAD` points at the codex branch), and `zimam`'s
is `claude/resume-zimam-second-review-34a1a6`. *Verified.*

###### Asset pipelines are absent, mostly on purpose

No project has an asset build, importer, or baking stage. `nurikabe` and `sweeper` draw
everything procedurally in WGSL with no textures or fonts to import; `aletheia` authors RON
by hand or through its in-process editor and hot-reloads it, with glTF `.glb` files dropped
into a folder and referenced by path; `browser-native-rts-poc` is the closest thing to a
real content pipeline, with a `build.rs` that globs `assets/maps/*.ron` into a compile-time
registry and an `rtsdata validate` lint gate over designer content; `humm` downloads its ML
models at runtime rather than shipping them. For the current portfolio this is the right
call and not debt — but it means the studio has **no experience** of a content pipeline,
which is the first thing a texture- or audio-heavy project would need. *Judgement, TechLead.*

###### Reproducibility is good; toolchain pinning is not

A clean-machine `cargo build --release` genuinely works in most Rust repos, because of one
consistently applied habit: **vendor or bundle native dependencies instead of requiring
system packages.** `tvi` bundles SQLite and uses rustls rather than OpenSSL; `RustView`
compiles Cisco's openh264 C sources in; `humm` vendors whisper.cpp; `sweeper` vendors a
patched `block` crate. `Cargo.lock` is tracked in git in **all seven** Rust repositories
that contain code (`zimam` has no code yet).

Toolchain pinning is the counter-example: only `aletheia` (1.95.0) and `nurikabe` (1.97.1)
have a `rust-toolchain.toml`. `tvi`, `humm`, `RustView`, `sweeper` and
`browser-native-rts-poc` float on whatever `stable` is installed, and `sweeper`'s real
floor (≥1.88, for let-chains) is undeclared anywhere. *Verified.*

##### Standards and conventions

These are the rules that are actually followed in more than one repository. Everything
else is in Open questions.

1. **The verification gate is a committed script or Make target, invoked by one command.**
   Not README prose, not a list of steps. Six repos do this.
2. **Vendor or bundle native dependencies; do not require system packages.** Prefer
   rustls over OpenSSL, bundled SQLite over a system one, compiled-in C sources over a
   discovered library. This is why clean-machine builds work here.
3. **Commit the lockfile.** All seven Rust repos with code track `Cargo.lock`.
4. **Pin dependency versions and audit the supply chain where the project warrants it.**
   `nurikabe` pins every dependency with `=`; `nurikabe`, `tvi` and
   `browser-native-rts-poc` carry a `deny.toml`; `infrastructure` pins every container
   image by `@sha256:` digest.
5. **CI configuration must match the forge it will run on.** Newly stated here rather
   than observed — see Open questions row 4. Today four repos violate it.

Deliberately **not** listed as standards, because they are not practised: toolchain
pinning (2 of 12), packaging (1 of 12), signing (0 of 12), release tagging (3 of 12),
and CI (0 of 12 executing).

##### Open questions

| Question | Owner | Blocks |
| --- | --- | --- |
| Do the two Forgejo runners described in `nurikabe/docs/IMPLEMENTATION_STATUS.md:163-174` exist, or is that document stale against `infrastructure/AGENTS.md:41-54`? | TechLead | Any claim that a CI gate executes anywhere in the studio |
| Is public distribution a goal for any project, or is everything internal/portfolio? A one-line answer per project would do. | CEO | Whether a packaging convention is worth writing at all — and the `RustView` H.264 patent question |
| Should we acquire an Apple Developer ID and a Windows code-signing certificate? Both are spend decisions. | CEO | Signed artifacts for `nurikabe` (packaging otherwise complete) and any macOS/Windows release |
| For the six repos with inert CI config: port to `.forgejo/workflows/`, or delete the files and rely on the documented manual script list? | TechLead | Standards row 5; removing the false impression that gates run |
| Is the no-runner policy permanent, or should the separate-trust-boundary runner in `infrastructure/docs/future-runner.md` be built? | CEO | Whether "no CI" is a standard to document or debt to retire |
| Which `nurikabe` Windows artifact is canonical — the MSVC `package-windows.ps1` or the cross-compiled GNU `package-windows.sh`? | CEO | A first Windows release from any project |

##### Where this shows up

Examples only. This page stays true if every project below is cancelled.

- **`nurikabe`** is the studio's only project with desktop packaging metadata for three
  platforms, and the only one with `.forgejo/workflows/` as its sole CI config — so it is
  where a packaging convention would be reverse-engineered from, if we wrote one.
- **`browser-native-rts-poc`** is a two-toolchain repo (Cargo plus Node for packaging and
  Playwright) and the only project with a working release-packaging path that has actually
  been deployed.
- **`RustView`** is the worked example of finished-but-undistributable: two CI systems, a
  release job guarded to a forge the studio does not use, and no icon, bundle or signing.
- **`aletheia`** is the deepest local gate — a Python stage supervisor with per-stage
  timeouts wrapping fmt, two clippy matrices, tests, a packaged-player build and content
  validation.
- **`infrastructure`** is where the studio's CI policy is written down, and is itself the
  least reproducible repo: nothing in it performs a first deployment, so rebuilding the
  host means re-running 562 lines of English prose.
- **`ai-engineering-governance`** and **`jsync`** are the two non-Rust repos with a Make
  wrapper as their gate, and between them show both packaging outcomes: an unfilled install
  placeholder, and a `make package` that zips a directory.

##### Sources

Listed in the front matter. Every claim above traces to one of: a named file and line in a
repository at the SHA recorded there, one of my twelve scan reports
([DIG-9](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-9.md) … [DIG-20](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-20.md)), or TechLead judgement labelled
as such inline.

**Confidence note.** Nearly every claim here was re-checked this session against the primary
artifact — the repository files at the SHAs listed — so it is `verified`. The page is marked
`reported` because of one load-bearing claim that is not: that Forgejo Actions is disabled
*on the live host*. I verified the studio's written policy, the committed
`config/forgejo/app.ini`, and the fact that `scripts/final-validation.sh:121` asserts the
setting against `/etc/forgejo/app.ini` — but I did not run that validator, and per
[DIG-13](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-13.md) it currently fails on unrelated checks. Page confidence is its
weakest claim, and no decision should rest on the live-host state until Open questions row 1
is closed.


## Comments (2)

<a id="comment-78b50328-6b7a-4662-99cc-49ecaa6d5524"></a>

### 2026-09-25T15:27:42.734Z — TechLead

Comment id `78b50328-6b7a-4662-99cc-49ecaa6d5524`; written by run `611b787f-c364-4feb-938d-e43c17f9a8b6`.

##### Done — `content` document holds the finished page

Page written to [`content`](DIG-41.md#document-content) (~14.9 KB). Front matter complete, sources filled, no unattributed claims.

**I did not write this from the scan reports alone.** All twelve checkouts were still on disk, so I re-verified every load-bearing claim read-only against the repositories at pinned SHAs (recorded in `sources`) rather than restating [DIG-9](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-9.md)…[DIG-20](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-20.md). That changed three things, one of them the headline.

###### Findings

- **There is no CI in this studio, and it is deliberate policy — not drift.** `infrastructure/AGENTS.md:41-54` says "Do not install a same-host Forgejo runner … keep Forgejo Actions disabled now"; `config/forgejo/app.ini:66-67` sets `[actions] ENABLED = false`; and `scripts/final-validation.sh:121` asserts that setting against the live `/etc/forgejo/app.ini`. Six repos nonetheless carry CI config, four of them GitHub Actions files in a studio with no GitHub remote. **All of it is inert.** That is worse than having none — a gate nobody can run looks identical to a gate that passes.
- **No packaging convention exists.** Exactly **one** repo of twelve (`nurikabe`) has any desktop packaging metadata. Nine have none at all. Verified both items the task flagged: `RustView` has no icon (no `with_icon`, `assets/` holds only `screenshot.png`), no bundle, no `.desktop`, no `Info.plist`, no signing — and its release job is guarded to `github.com` while its only remote is Forgejo; `ai-engineering-governance`'s install command is still the literal `https://github.com/<owner>/<repository>/tree/<commit>/…` template. I stated the gap plainly instead of describing a CI pipeline, per your scope note.
- **Two corrections to my own earlier reports**, which is why the re-check was worth it:
  1. Tags: **three** exist studio-wide (`jsync` v0.1.0, `tvi` v0.1.0 annotated; `humm` **v1.0.0** lightweight) — my [DIG-14](DIG-14.md) report implied `humm` was never versioned.
  2. [DIG-12](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-12.md) said `nurikabe` CI "triggers on a `main` that does not exist". Half right: it triggers on `main` **and** `codex/**`, which does match the live branch. Fixed in the page.
- **Four real cross-project standards** do exist and I recorded them as such: the gate is a committed script invoked by one command (6 repos); vendor/bundle native deps rather than requiring system packages (which is *why* clean-machine builds work here); commit `Cargo.lock` (7/7 Rust repos with code); pin deps and audit supply chain where warranted. Toolchain pinning (2/12), packaging (1/12), signing (0/12) and CI (0/12 executing) are explicitly listed as **not** standards.

###### Page confidence: `reported`, deliberately

Nearly every claim is `verified` against the primary artifact. The page is `reported` because of one load-bearing claim that is not: that Actions is disabled *on the live host*. I verified the policy, the committed config, and the assertion in the validator — but I did not run that validator, and per [DIG-13](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-13.md) it currently fails on unrelated checks. Weakest claim sets the page.

###### Open questions — owners assigned, two are mine

Six rows. I kept the ones an agent can close: reconciling `nurikabe/docs/IMPLEMENTATION_STATUS.md:163-174` (which describes two *live* runners, contradicting the policy above) and deciding whether the six inert CI configs get ported to `.forgejo/` or deleted. Four are genuine founder calls for CEO (historical link `/DIG/agents/ceo`) — whether public distribution is a goal at all, Apple/Windows signing credentials (spend), whether the no-runner policy is permanent, and which `nurikabe` Windows artifact is canonical.

No follow-up needed from me to publish — the page is self-contained under your template and the structure approval on [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) gates publishing, not this work.

<a id="comment-8ad4f55c-2aa0-44e1-88f8-db28fec6518c"></a>

### 2026-09-25T15:33:27.526Z — KnowledgeCurator

Comment id `8ad4f55c-2aa0-44e1-88f8-db28fec6518c`; written by run `2e7d0364-58d1-4ee6-a464-5a7500523e10`.

##### Curated and staged — Tooling & Pipeline domain page

Picked this up alongside [DIG-39](DIG-39.md) so all three domain pages are staged together. It passes curation with the lightest touch of the three — you were the only one who linked your sibling domains without being asked.

**Conformance ([DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) `design`, revision 4 corrected):** five required front-matter fields ✅, single H1 identical to `title:` ✅, seven H3 sub-sections — allowed, §4 bans H4 and deeper, not H3 ✅, all six §9.7 sections in template order ✅, boundary links already relative and correctly pointed ✅.

**Two curator edits (§6, Curator-alone, both reversible)**

1. **Name normalised to "Tooling & Pipeline"** in `title:` and the H1. The design doc's own worked example (§11) uses the ampersand, and the other two domain pages had to refer to you by name; one form beats three. The slug is unaffected — `tooling-pipeline` stands, and §4 forbids ever renaming it.
2. **Up-link footer** added — `[← Knowledge base](../../README.md)`, per §5.4.

**`confidence: reported` is right, and I am not upgrading it.** Under the corrected §3 most of your page is structural and would read `verified` on its own, but the live-host and Actions-disabled state is not settled by any artifact you read, and page confidence is the weakest claim. You already say this and point at Open questions row 1. That is exactly the disclosure §3 asks for.

**Single source of truth (§5.3).** Diffed against [DIG-39](DIG-39.md) and [DIG-40](DIG-40.md). One touchpoint: `./scripts/check.sh` appears on Software Engineering inside aletheia's definition of done, and here as the committed-single-command-gate claim. This page owns the mechanism; I resolved it with the boundary link rather than by cutting it there, since the gate is load-bearing for that page's argument. No other claim is duplicated.

**Staged, not published.** Publishing waits on the founder confirming the structure — [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) has a pending `request_confirmation`. The file is held at [DIG-6 → `domain-tooling-pipeline`](DIG-6.md#document-domain-tooling-pipeline), ready to write to `knowledge/domains/tooling-pipeline/README.md` unchanged. Nothing is waiting on you.


## Agent runs (3)

Every Paperclip heartbeat run recorded against this issue. A run's results are the comments, documents, attachments and commits it wrote (comment headers above name the run). Raw run transcripts are not archived, except the runs listed with a transcript copy below; see [inventory.md](../inventory.md#exclusions).

| Run | Agent | Status | Started | Finished | Log bytes | Transcript copy |
|---|---|---|---|---|---|---|
| `6fc692b5-592b-4e4b-9a86-b7753e333e17` | KnowledgeCurator | succeeded | 2026-09-25T15:19:06.209Z | 2026-09-25T15:22:49.678Z | 175279 | — |
| `611b787f-c364-4feb-938d-e43c17f9a8b6` | TechLead | succeeded | 2026-09-25T15:20:48.136Z | 2026-09-25T15:32:58.037Z | 242964 | — |
| `2e7d0364-58d1-4ee6-a464-5a7500523e10` | KnowledgeCurator | succeeded | 2026-09-25T15:27:49.098Z | 2026-09-25T15:34:30.398Z | 259883 | — |

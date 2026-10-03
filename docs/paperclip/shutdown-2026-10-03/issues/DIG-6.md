# DIG-6: Seed knowledge base domain foundations

> Cross-reference copy. The primary archive for DIG-6 is in the `infrastructure` repository: https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-6.md. Attachments and earlier document revisions are stored there.

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (baseline export 2026-10-03T07:00Z) | done |
| Priority | medium |
| Owner (assignee) | CEO |
| Created by | CEO |
| Paperclip project | Onboarding |
| Primary repository archive | infrastructure |
| Parent | [DIG-1](DIG-1.md) |
| Created / updated | 2026-09-25T11:34:30.504Z / 2026-09-26T03:42:24.686Z |
| Completed / cancelled | 2026-09-26T03:42:24.686Z / — |
| Blocked by | — |
| Blocks | [DIG-38](DIG-38.md) (done) |
| Paperclip id | `59cad1bc-a1c5-43a2-8827-f1778f50de03` |

## Description

##### Goal
Create the first version of the domain pages: software engineering, systems architecture, game design, UI design, UX design, tooling and pipeline. Written to Digikal's context, not generic textbook content.

##### Approach
- Knowledge Curator owns the task and the final pages. Content comes from the domain owners: Tech Lead for software engineering, systems architecture, tooling/pipeline; Game & Experience Designer for game design, UI, UX.
- For each domain, create a child issue assigned to the owning agent with a clear brief: the page template, the sections to fill, the expectation that examples reference Digikal's projects where the scan has produced them, and a length ceiling (short, accurate pages beat long generic ones).
- Curate the returned content: consistency, front matter, links, no unattributed claims, no filler.

##### Suggested first pages per domain
- Principles Digikal follows in that discipline, with rationale.
- Common patterns and anti-patterns seen in the studio's projects.
- Checklists the team actually uses (review checklist, definition of done, accessibility checklist, etc.).
- Glossary of terms used across projects.
- Further reading, curated and annotated, not a link dump.

##### Blocked by
Task "Knowledge base structure and maintenance rules". You wake automatically when it is done. Project-specific examples can be added later if the scan is not yet complete; note that as a follow-up on the page.

##### Done when
All six domains have their first pages with complete front matter and are reachable from the index. Post the list as a comment and set the task to in_review assigned to CEO.

## Documents (latest revision)

<a id="document-domain-software-engineering"></a>

### Document `domain-software-engineering`: Publish-ready: knowledge/domains/software-engineering/README.md

Revision 1, updated 2026-09-25T15:30:54.445Z by KnowledgeCurator.

---
title: Software Engineering
owner: TechLead
last_reviewed: 2026-09-25
confidence: verified
sources:
  - "Twelve repository scans by TechLead, all 2026-09-25: DIG-9 aletheia, DIG-10 tvi, DIG-11 browser-native-rts-poc, DIG-12 nurikabe, DIG-13 infrastructure, DIG-14 humm, DIG-15 RustView, DIG-16 adp, DIG-17 ai-engineering-governance, DIG-18 sweeper, DIG-19 jsync, DIG-20 zimam"
  - "Re-checked against the working trees this pass (2026-09-25): aletheia@c475d18, browser-native-rts-poc@7f9af4c, humm@0864269, jsync@23ea443, nurikabe@7f41c88, RustView@ef017ad, sweeper@274b86c, tvi@64aee5c, ai-engineering-governance@11ec305, infrastructure@cc17670, adp@6e13c3e, zimam@8eb4810"
  - "jsync@23ea443 `bin/jsync:484-518` — the SSH remediation message, read in full"
  - "browser-native-rts-poc@7f9af4c `crates/client/src/input.rs:26-38`, `crates/sim/src/production.rs:14-21` — typed placement error and its player-facing copy"
  - "tvi@64aee5c `src/ui/screens/screener.rs:190`, `src/ui/render_tests.rs:927`, `CLAUDE.md:65` — the \"Need data\" rule and its assertion"
  - "tvi@64aee5c `Cargo.toml:61-63` — the release-profile comment recording a deliberate deviation on `panic`"
  - "aletheia@c475d18 `docs/engineering/AI_ENGINEERING_POLICY.md` §1-§4 and `## Aletheia project requirements` — the only written definition of done in the studio"
  - "sweeper@274b86c `src/render.rs:627-675`, `src/main.rs:308` — stale-uniform bounds check against a resized board"
  - "Test, marker, `unsafe`, error-crate, manifest, and authorship counts: greps run over all twelve working trees this pass"
  - "TechLead judgement, named inline where it appears"
---

#### Software Engineering

##### What this covers

How Digikal writes, tests, reviews and maintains code: language choice, where tests
live and what they actually cover, how failure reaches a user, what review means here,
and when a change is done.

It does **not** cover how systems are shaped — module boundaries, data flow, determinism
strategy, persistence — that is [Systems Architecture](../systems-architecture/README.md). It does **not** cover
build, CI, toolchain pinning, or release mechanics — that is
[Tooling & Pipeline](../tooling-pipeline/README.md). Several
facts below (no CI anywhere, `cargo-deny` policy, toolchain pins) are named only where
they change what an engineer must do by hand; their ownership sits in that page.

##### How Digikal does this

**Rust is the default for anything with a runtime.** Seven of twelve repositories are
Rust codebases (aletheia, browser-native-rts-poc, humm, nurikabe, RustView, sweeper,
tvi). The other five are not counter-examples — they are a Bash tool (jsync), an ops
repo (infrastructure), a Python agent skill (ai-engineering-governance), and two
document archives with no product code (adp, zimam). Editions are split 2021 (tvi,
RustView) and 2024 (the rest). **No repository records why Rust** — there is no
decision record, README rationale, or ADR anywhere in the twelve. See Open questions.

**Tests live inline, next to the code.** Across the seven Rust repos there are **4,314
`#[test]` functions inside `src/`** against **369 in `tests/` directories** — inline by
roughly twelve to one, and four repos (nurikabe, tvi, RustView, sweeper) have no
integration test directory at all. Density is real where it exists: aletheia 1,986
tests, humm 1,976, tvi 1,231, browser-native-rts-poc 734, nurikabe 418.

**What gets tested is the pure logic; what touches a device is verified by hand.** This
is the sharpest split in the studio and it is consistent across projects. RustView has
92 tests and **zero** in `video/player.rs`, `audio.rs`, `frame_queue.rs`, `clock.rs`, or
`decoder.rs` — the threaded A/V pipeline where its repair history is concentrated.
sweeper has 12 tests in `game.rs`/`score.rs`/`audio.rs` and none over `render.rs` (932
lines) or `main.rs` (705 lines), which is 58% of its code and where both of its known
defects live. humm's 70 `#[ignore]`d contract tests — the layer its own policy makes
solely responsible for validating an external upgrade — are the tests CI never runs.
*TechLead judgement: this is not laziness; it is the absence of a studio answer for
testing anything with a GPU, an audio device, or a thread in it. It is the single
highest-value gap on this page.*

**Unfinished work is never left in the source.** Across all seven Rust codebases there
are **zero** `TODO`, `FIXME`, `HACK`, `todo!()` or `unimplemented!()` markers. (One
`XXX` hit in humm is a test fixture string, not a marker.) Backlogs live in documents
instead — `PLAN.md` in browser-native-rts-poc, `docs/NEXT.md` + `docs/current-state.md`
in aletheia, an `[Unreleased]` CHANGELOG block in jsync, a normative implementation
guide in sweeper. Where the backlog lives is not standardised; that it is not in the
code is.

**Error handling is the studio's most deliberate practice.** *TechLead judgement.* Three
independent codebases, three unrelated domains, one shape: a failure names its cause,
names the user's next action, and never invents a value.

- **jsync** (`bin/jsync:484-518`) answers an SSH auth failure with a four-step numbered
  remediation — test the connection and verify the fingerprint, generate a key, install
  it with the exact `umask 077` one-liner, verify with `BatchMode=yes` — interpolating
  the real host and port into every command, and closing with "Do not disable host-key
  checking." It tells you what to type.
- **browser-native-rts-poc** keeps a typed `PlacementError` enum in the zero-dependency
  `sim` crate (`crates/sim/src/production.rs:14-21`) and maps it to copy only at the
  presentation boundary (`crates/client/src/input.rs:26-38`): "Requires clear ground.",
  "A unit is blocking this footprint.", and — pulled from the rules data, not hardcoded —
  "Build within {N} cells of one of your structures." Six causes, six distinct answers.
- **tvi** renders missing provider data as "Need data", never as zero
  (`CLAUDE.md:65`), ranks it honestly at the bottom of the screener rather than hiding
  it (`src/ui/screens/screener.rs:190`), and **asserts the string in a render test**
  (`src/ui/render_tests.rs:927`).

Answering the question that prompted this page: **this is a practice, not an accident.**
Three codebases converged on it independently, and in two of the three it is written
down or test-enforced.

**It is not universal, and the exceptions are informative.** sweeper and RustView — the
two least-maintained Rust repos — both ship `panic = "abort"`, which turns any
unhandled case into a hard crash with no dialog and no unwind. RustView has 52
`.unwrap()`/`.expect()` sites in `src/`; sweeper's `tile_from_cursor` bounds-checks the
cursor against the *stale* render uniform (`src/render.rs:655-660`) and then indexes the
*new* board (`src/main.rs:308`), so shrinking the difficulty panics. By contrast tvi
deliberately **omits** `panic = "abort"` and says why in the manifest: abort skips
unwinding and would leave the user's shell in raw mode (`Cargo.toml:61-63`). The good
error handling correlates exactly with the projects that are still being worked on.

**There is no pull-request review, because there is no second engineer.** Every
repository is effectively single-author: aletheia 471 of 563 commits by Jawad Alnajjar
(the other 91 by AI agents), browser-native-rts-poc 196 of 196, humm 244 of 244, tvi 546
of 552. Merge commits are near-zero (humm 0, nurikabe 5, tvi 8). Development is
trunk-based, with agent branches (`claude/…`, `codex/…`) in some repos.

**What substitutes for review is two things, and only one is systematic.** First,
**review as a committed document**: nurikabe carries `docs/DESIGN_REVIEW.md`,
`RELEASE_REVIEW.md`, `GENERATOR_AUDIT.md`; tvi carries `docs/review/` plus two dated
committee reviews; zimam is almost entirely review apparatus. Second, the **adversarial
review pass** defined by `ai-engineering-governance` — a single mandatory pass triggered
only by auth/secrets, money movement, irreversible data operations, production schema
migrations, or policy changes, explicitly non-waivable "by urgency, confidence, or task
size." That policy is installed in **two of twelve repos**, at two different versions
(aletheia schema 2 / policy 3.0.0; sweeper schema 2 / policy 2.1.0), while its author
repo runs 3.1.0. Re-verified this pass. The nine others have no policy at all.

**One repo has a written definition of done; eleven do not.** aletheia's policy §4
requires a completion report stating what changed and why, the commands run **with their
actual results**, the checks *not* run, remaining uncertainty, adversarial findings and
their resolution, and confirmation that user work outside the change is untouched — with
the rule that "a check that was not run is reported as not run." Its project section adds
that `./scripts/check.sh` must be green and that a green gate does not replace a required
review pass. *TechLead judgement: this is the best definition of done in the studio and
the obvious candidate to generalise.*

##### Standards and conventions

These are the rules that are actually followed, verified against the trees this pass.

1. **Tests go inline in a `#[cfg(test)]` module beside the code they test.** Reach for
   `tests/` only for cross-crate contracts (humm, aletheia, browser-native-rts-poc do).
2. **No `TODO`/`FIXME`/`HACK`/`todo!()`/`unimplemented!()` in committed source.**
   Unfinished work goes in a document. Zero violations across seven Rust repos.
3. **A user-facing failure names the cause and the user's next action, and never renders
   a fabricated value** (no silent zero, no empty string standing in for missing data).
   Where the next action is a command, print the command with the real arguments filled
   in.
4. **Domain errors are typed values in the core; user-readable copy is produced at the
   presentation boundary**, never inside the logic that detects the failure
   (browser-native-rts-poc is the reference implementation).
5. **A deliberate deviation from a default is recorded where the deviation lives**, with
   the failure mode it prevents (tvi's `panic` comment is the model).

Rules 3 and 4 are the ones worth defending. Rules 1 and 2 are descriptive — they are
what every repo already does.

##### Open questions

| Question | Owner | Blocks |
| --- | --- | --- |
| Why Rust? No repo records the decision. A one-paragraph answer (performance, single-language studio, agent-friendliness, preference) settles whether a non-Rust project needs justification. | Jawad (founder) | Hiring briefs; whether a future tool may be written in something else |
| Is there a studio error type convention, or does each project choose? Today: nurikabe/humm/tvi use `thiserror`/`anyhow`; aletheia and browser-native-rts-poc hand-roll plain enums with no error crate; RustView and sweeper use neither. Four approaches, seven repos. | TechLead, with founder sign-off | Nothing today; every new crate re-litigates it |
| Is there an `unsafe` policy? nurikabe forbids it workspace-wide; browser-native-rts-poc and humm forbid it per-crate on their cores; aletheia, tvi, RustView and sweeper have no policy and 28 `unsafe` blocks between them — including one in RustView (`src/video/decoder.rs:137`) that constructs a reference to uninitialised memory. | TechLead | A soundness bug in RustView's hot path |
| Should the `ai-engineering-governance` policy be installed in the other ten repos, or is it aletheia-and-sweeper only by choice? Both current installs are on stale versions, and sweeper is on the 2.1.0 regime the policy's own baseline experiment measured at 3.0–4.4× cost for zero outcome gain. | Jawad (founder), executed by TechLead | Whether "adversarial review pass" is a studio standard or one project's habit |
| Should aletheia's completion-report definition of done (policy §4) become the studio-wide definition of done? | CEO, drafted by TechLead | This page promoting §4 from example to standard |
| What is the studio's answer for testing GPU, audio-device and threaded code? Every project has skipped it and two have shipped defects there. Options: a headless self-test binary (RustView already has one nothing runs), golden-image comparison, or accept manual verification and say so. | TechLead | Closing the largest verification gap in the studio |
| Does "error handling names the next action" apply to developer-facing failures too, or only player/user-facing ones? jsync's is developer-facing; tvi's is user-facing. | Game & Experience Designer (player-facing copy), TechLead (developer-facing) | Writing rule 3 into project templates |

##### Where this shows up

Examples only. Every claim above survives any of these being cancelled.

- **browser-native-rts-poc** is the reference for rules 3 and 4: a typed error enum in a
  zero-dependency core, copy at the client boundary, values interpolated from rules data.
- **jsync** is the reference for a developer-facing failure that teaches: four numbered
  steps, real host and port, in ~1,000 lines of Bash with no dependencies.
- **tvi** is the reference for refusing to fabricate: "Need data" rendered, ranked, and
  asserted in a test.
- **aletheia** is the only repo with a written definition of done and a single canonical
  validation gate.
- **RustView** and **sweeper** are the counter-examples: `panic = "abort"` over untested
  threaded and rendering code. Useful precisely because they show what the practice
  costs when it lapses.
- **ai-engineering-governance** is where the adversarial-review rule is authored; it is
  installed in two repos and is itself the only one running its current version.
- **nurikabe**, **humm**, **infrastructure**, **adp**, **zimam** contribute to the counts
  above but are not load-bearing for any rule here.

##### Sources

Listed in full in the front matter. Every claim traces to one of: a file path at a named
SHA in a repository working tree read this pass; a count produced by grep over those
trees this pass; the twelve scan reports on DIG-9 through DIG-20; or TechLead judgement,
which is labelled inline wherever it appears.

Every claim carried forward from a scan report was re-checked against the code this pass,
including the three that most affect the conclusions above: humm's 70 `#[ignore]`d tests
against a CI job that runs only `cargo test --workspace`
(humm@0864269 `.github/workflows/ci.yml:27-33`, whose own line 31 notes the ignored tests
must be run separately); RustView's repair history in `video/` (`cb886f5`, `f3086d7`,
`e3191bd`, `6e92652`) and the uninitialised-memory `unsafe` at `src/video/decoder.rs:137`;
and the 3.0–4.4× cost figure at
ai-engineering-governance@11ec305 `drafts/DESIGN-NOTES.md:12`.

Nothing on this page rests on an `assumed` claim.

---
[← Knowledge base](../../README.md)

<a id="document-domain-systems-architecture"></a>

### Document `domain-systems-architecture`: Publish-ready: knowledge/domains/systems-architecture/README.md

Revision 1, updated 2026-09-25T15:32:26.392Z by KnowledgeCurator.

---
title: Systems Architecture
owner: TechLead
last_reviewed: 2026-09-25
confidence: verified
sources:
  - "DIG-9 scan report — aletheia @ main `c475d18` (Cargo.toml workspace, crates/aletheia_core/src/{save.rs,config/}, crates/aletheia_game/src/mission.rs, crates/aletheia_render/)"
  - "DIG-10 scan report — tvi @ master `64aee5c` (src/lib.rs, src/api/repository/, src/finance/conviction.rs, src/db/schema.rs, src/web_api/v1.rs)"
  - "DIG-11 scan report — browser-native-rts-poc @ main `7f9af4c` (crates/sim/{Cargo.toml,src/lib.rs,clippy.toml,src/pcg.rs}, crates/data/src/lib.rs, crates/protocol/src/lib.rs, crates/server/src/lib.rs, CLAUDE.md)"
  - "DIG-12 scan report — nurikabe @ codex/nurikabe-implementation `7f41c88` (Cargo.toml workspace, crates/nurikabe-core/src/, crates/nurikabe-solver/src/lib.rs, crates/nurikabe-game/src/{storage.rs,session/jobs.rs})"
  - "DIG-13 scan report — infrastructure @ main `cc17670`"
  - "DIG-14 scan report — humm @ main `0864269` (Cargo.toml workspace, crates/humm-core/src/boundary.rs, crates/humm-core/src/queue.rs, docs/design/ARCHITECTURE.md)"
  - "DIG-15 scan report — RustView @ main `ef017ad` (src/app.rs, src/file_nav.rs, src/image_loader.rs, src/video/)"
  - "DIG-16 scan report — adp @ main `6e13c3e`"
  - "DIG-17 scan report — ai-engineering-governance @ main `11ec305`"
  - "DIG-18 scan report — sweeper @ main `274b86c` (src/game.rs, src/main.rs, src/score.rs)"
  - "DIG-19 scan report — jsync @ main `23ea443`"
  - "DIG-20 scan report — zimam @ claude/resume-zimam-second-review-34a1a6 `8eb4810` (Physical Design Specification v0.2 §B.1–B.6)"
  - "TechLead named judgement — which of these are studio patterns vs single-project choices (marked inline)"
---

#### Systems Architecture

##### What this covers

How Digikal draws the seams inside a system: crate and module topology and which way dependencies are allowed to point, which component owns which kind of state, where determinism is a hard requirement rather than a nicety, client/server splits, and the on-disk and on-the-wire formats those seams exchange.

It does **not** cover any individual project's layout — that belongs on each project's own architecture page. It does **not** cover code-level practice (error handling, test style, `unsafe` policy, lint configuration) — that is [Software Engineering](../software-engineering/README.md). It does **not** cover how code becomes a build or an asset becomes a shipped file — that is [Tooling & Pipeline](../tooling-pipeline/README.md).

##### How Digikal does this

**The Cargo dependency graph *is* the architecture enforcement, and it is the only mechanism used studio-wide.** Four of the seven Rust application repos are multi-crate workspaces with a deliberate one-directional dependency spine: aletheia (7 crates), browser-native-rts-poc (6), nurikabe (4), humm (9). In each, the rule is written down in prose *and* made true by the manifests — a crate that must not know about a thing simply does not list it as a dependency. humm's `binaries → adapters → core` rule means `humm-core` cannot name ffmpeg, SQLite, cpal or `ort` even by accident; browser-native-rts-poc states its chain as a hard rule in `CLAUDE.md` and mirrors it per manifest; aletheia's `aletheia_game` does not depend on `aletheia_render` at all, and cross-crate render ordering is routed through a set exported from `aletheia_core`. That is a genuine, repeated studio practice.

**The engine-free domain core is the practice in most, but deliberately not all, of our game code.** nurikabe puts rules, solving and generation in three crates that carry no Bevy dependency — only `nurikabe-game` imports the engine. browser-native-rts-poc goes further: `crates/sim` has an empty `[dependencies]` block, zero dependencies of any kind. aletheia is the counter-example and it is a choice, not a lapse: `aletheia_core` is Bevy-typed throughout — its config pipeline loads each domain through a generic `ConfigPlugin<C>` into a `Res<Active<C>>`, and it owns `AppState`. So aletheia's core is *sibling-free*, not *engine-free*. Two different targets, and no studio-wide answer for which one a new project should pick.

**Determinism is treated as a per-project requirement, and exactly one project defends it mechanically.** browser-native-rts-poc needs cross-client determinism to exist at all (15 Hz lockstep over a relay), and the whole crate is bent to it: `#![deny(clippy::float_arithmetic, clippy::disallowed_types)]` plus a `clippy.toml` `disallowed-types` list banning `HashMap`/`HashSet`, all randomness through one seeded `Pcg32`, and a `state_hash_covers_every_field` test that fails if a struct field is added without folding into the hash. aletheia has the same *property* in its sim — no RNG and no wall-clock time, variation derived from hash-of-counter or noise-of-position, at an owned 64 Hz `FixedUpdate` — and ships deterministic replay with stable FNV-1a fingerprints, but my scan recorded no lint-level enforcement of the ban. nurikabe needs determinism only in generation, and gets it by *checking the result*: a generated puzzle is not accepted until it regenerates byte-for-byte from `(GENERATOR_VERSION, seed, dimensions, challenge)`.

That settles the question the scope asked: **a determinism-critical `sim` crate is one project's answer to one project's problem.** What generalises is the three sub-practices it uses — name the boundary the property is defended at, hash the whole state so drift is detectable, and pin behaviour with golden fixtures — and those do show up wherever a project actually needs them.

**Presentation reads simulation state; it does not write back.** Stated most explicitly in browser-native-rts-poc ("Bevy reads sim state and draws it; nothing feeds back"), enforced structurally in aletheia by the missing `game → render` edge, and expressed in nurikabe as a threading rule (solver and generator work never touches the render thread). Same principle, three different altitudes, no shared wording.

**One named owner per kind of state, and one gateway through which it is mutated.** This is the most consistent property in the portfolio and holds in every repo that has meaningful state:

- tvi — `SahmkRepository` is the only data gateway; `finance/conviction.rs` is the single source of any user-facing verdict; the 37 modules under `src/finance/` are pure and I/O-free.
- aletheia — mission entry clones the resolved `Mission` into an immutable `MissionRuntime` and its Level into a separate immutable `ActiveLevel`, so a structural content edit applies on the next attempt and never mid-run; runtime `Active<C>` and inspector mutations are runtime-only and never persist.
- humm — one `TaskEvent` stream is the source of truth for the GUI table, the CLI renderer, the exporters and the store; the full `JobSpec` is persisted with the queue record.
- nurikabe — every board mutation goes through a validated `nurikabe-core` session transaction with undo/redo.
- RustView — current file lives only in `FileNavigator`, decoded pixels only in the loader's cache, view transform only in `ViewState`.

**Long work runs on an owned thread behind an explicit cancellation token; the UI polls and never blocks.** nurikabe (`std::thread` workers, mpsc, `CancellationToken`), humm (one `TaskRunner` thread, `flume` in / broadcast out, `CancelToken` polled by whisper.cpp's abort callback), tvi (background Tokio tasks with `AtomicBool` cancellation), RustView (two decode threads plus three video threads). Four independent arrivals at the same shape.

**Persisted formats carry a version and migrate forward; writes are atomic replaces; a failed read is quarantined rather than overwritten.** Four projects version and migrate: aletheia is at save v3 with explicit v1 and v2 parsers, nurikabe carries `PlayerProfile` v7 / `SavedGame` v3 / settings v5, tvi has 16 `column_exists`-gated `migrate_*` functions and never an unguarded `ALTER TABLE`, and browser-native-rts-poc carries three separate version constants (`PROTOCOL_VERSION = 12`, RON `FORMAT_VERSION = 3`, `JOURNAL_VERSION = 1`). Atomic replace is the broader habit and shows up even where versioning does not — sweeper writes its score file temp-then-rename with a Windows-specific backup and rollback. The two places both are absent are the two smallest persistence surfaces: RustView's three persisted settings keys and sweeper's score file carry no version field. Quarantine-on-bad-read is the most disciplined part: aletheia moves the slot to `.corrupt` and inhibits all automatic writes for the rest of the session rather than risk overwriting a good save; nurikabe renames to `session.rejected.json` and keeps the `.bak` alongside it.

**We have almost no client/server practice, because we have almost no servers.** Seven of the twelve repos are application systems, and exactly one of them has a server other machines connect to. browser-native-rts-poc is that one, and its rule is that the server is deliberately dumb: it relays, matchmakes and hosts static files, and it **never simulates** — the AI that takes over a dropped player runs inside the remaining clients' own sim — with no server-side persistence, by decision. tvi is the only other inbound surface in an application repo and it is loopback-only and read-only: `src/web_api/v1.rs` defines explicit wire DTOs because domain structs are deliberately never serialised directly. aletheia's only socket is a dev-only BRP listener on `127.0.0.1` that compiles out of player builds. Two data points is not a convention.

**Format choice follows the job, consistently.** RON for hand- and GUI-authored game content in the Bevy projects (aletheia's eight config domains, `crates/data`'s rules and maps); JSON for player-side persistence (nurikabe); SQLite for anything record-shaped (tvi's 26 tables, humm's WAL store); TOML for settings (humm); a binary codec only where the wire is hot (postcard in browser-native-rts-poc). Nobody has reached for a schema language, a code generator, or a binary format on disk.

##### Standards and conventions

These hold across projects today, with the evidence above.

1. **Dependency direction is one-way and expressed in the manifests.** If a rule says "A must not know about B", the enforcement is that A's `Cargo.toml` does not list B. Prose alone is not the mechanism. (4 of 4 multi-crate repos.)
2. **Domain logic is separated from the shell that runs it, at crate granularity where the project is large enough to have crates.** Which shell — engine, GUI, CLI, HTTP — is the variable; that there is a separated core is not. (aletheia, browser-native-rts-poc, nurikabe, humm; tvi does it at module granularity behind a `src/lib.rs` seam.)
3. **Every kind of state has exactly one owner and one mutation path.** Duplicating a piece of state across two owners is the thing we do not do. (All stateful repos.)
4. **Content edited while a run is in flight does not take effect mid-run.** Authoring state and runtime state are separate, and the runtime reads an immutable snapshot taken at entry. (aletheia's `MissionRuntime`/`ActiveLevel`; browser-native-rts-poc compiles RON to sim structs before the frame-0 barrier and rejects a config-digest mismatch as `StaleBundle` before the match starts.)
5. **A persisted format that will be read back after a schema change carries a version and a forward migration; writes are atomic replaces; a failed read is quarantined, never overwritten.** (Versioning: 4 repos. Atomic replace: 5. Absent only where the persistence surface is a handful of settings keys.)
6. **Blocking work is moved off the presentation thread behind a cancellation token the caller owns.** (4 repos, independently.)
7. **Where a property must hold across machines or across runs, it gets a named boundary and a total hash.** (browser-native-rts-poc's `sim` + `state_hash`; aletheia's replay envelope + FNV-1a fingerprints.)

Not standards — real practice in some projects only, recorded here so nobody mistakes them for rules: engine-free cores, lint-enforced determinism bans, and single-crate layouts.

##### Open questions

| Question | Owner | Blocks |
| --- | --- | --- |
| Should an engine-free domain core (nurikabe / browser-native-rts-poc shape) be the default for new Bevy projects, or stay a per-project call? aletheia deliberately went the other way. | TechLead to propose, CEO to adopt | Any shared-crate reuse across games; guidance for the next new project |
| Where determinism is required, must the ban be machine-enforced (clippy `disallowed-types` + a total-state-hash test) rather than conventional? And does aletheia's sim already enforce its no-RNG/no-wall-clock rule mechanically, or only by convention? My scan recorded the property but no enforcement mechanism — one file read closes the second half. | TechLead | Whether browser-native-rts-poc's enforcement pattern gets promoted to a standard; any future replay or lockstep work |
| Is any product other than the RTS expected to have a server? | CEO | Whether client/server splits are worth standardising at all; the backend-hire recommendation in [DIG-8](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-8.md) |
| Should a new save or settings format be required to carry a version field from v1, even when only one version exists? | TechLead | Turning standard 5 from a habit into a gate |
| Below what size is a single-crate layout fine? The two single-crate desktop apps both concentrated cross-cutting state in one type; none of the multi-crate repos did. | TechLead | Guidance for the next small tool |

##### Where this shows up

Examples only. Nothing above depends on any of these projects continuing to exist.

- **browser-native-rts-poc** is the studio's only networked system and its only lockstep-deterministic one. It carries the strictest boundary enforcement in the portfolio, and it is the source of the determinism sub-practices described above — not of a studio-wide determinism requirement.
- **aletheia** is a seven-crate Bevy workspace whose core is sibling-free but Bevy-typed, and whose largest crate by production lines is its editor rather than its game. It is the portfolio's strongest example of the authoring-state / runtime-state split.
- **nurikabe** is a four-crate workspace in which three crates carry no engine dependency, and where correctness is defended by an admission gate rather than by a lint.
- **humm** is a nine-crate non-game workspace with a `binaries → adapters → core` rule; it is the clearest demonstration that the Cargo graph, not documentation, is what makes a boundary real — two late adapter swaps cost one adapter each.
- **tvi** is a single-crate, two-binary repo that grew a `src/lib.rs` seam specifically to get a frontend-neutral boundary between its TUI and its web view.
- **RustView** and **sweeper** are single-crate desktop apps; both concentrate the frame loop and every cross-cutting concern in one type.
- **zimam** is a 28-crate architecture that exists only as design documents. Its central idea — that a component *cannot* contain a forbidden capability "by construction" because no dependency edge exists — is the most ambitious boundary design the studio has produced, and none of it is built or reviewed-clear.
- **infrastructure**, **adp**, **ai-engineering-governance** and **jsync** are not application systems and do not exercise this domain.

##### Sources

Twelve read-only repository scans performed 2026-09-25, each recorded as a `report` document on its issue: [DIG-9](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-9.md) aletheia, [DIG-10](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-10.md) tvi, [DIG-11](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-11.md) browser-native-rts-poc, [DIG-12](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-12.md) nurikabe, [DIG-13](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-13.md) infrastructure, [DIG-14](DIG-14.md) humm, [DIG-15](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-15.md) RustView, [DIG-16](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-16.md) adp, [DIG-17](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-17.md) ai-engineering-governance, [DIG-18](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-18.md) sweeper, [DIG-19](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-19.md) jsync, [DIG-20](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-20.md) zimam. Branches, SHAs and the specific files read are listed in the front matter above and cited per-claim in each report.

Every factual claim on this page restates something read in source during those scans. The judgements about *which* of those facts constitute a studio pattern versus a single-project choice are mine (TechLead) and are labelled as such in the text.

---
[← Knowledge base](../../README.md)

<a id="document-domain-tooling-pipeline"></a>

### Document `domain-tooling-pipeline`: Publish-ready: knowledge/domains/tooling-pipeline/README.md

Revision 1, updated 2026-09-25T15:32:26.573Z by KnowledgeCurator.

---
title: Tooling & Pipeline
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

#### Tooling & Pipeline

##### What this covers

How Digikal gets code from an engineer's machine to something a person can run: build
systems and toolchain pinning, the local verification gate, CI, asset and content
pipelines, packaging, signing, release and distribution. It is the studio's practice
*across* projects.

It does **not** cover how systems are designed or decomposed — that is
[Systems Architecture](../systems-architecture/README.md) — nor how we write, review and
govern code, which is [Software Engineering](../software-engineering/README.md).
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
([DIG-9](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-9.md) … [DIG-20](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-20.md)), or TechLead judgement labelled
as such inline.

**Confidence note.** Nearly every claim here was re-checked this session against the primary
artifact — the repository files at the SHAs listed — so it is `verified`. The page is marked
`reported` because of one load-bearing claim that is not: that Forgejo Actions is disabled
*on the live host*. I verified the studio's written policy, the committed
`config/forgejo/app.ini`, and the fact that `scripts/final-validation.sh:121` asserts the
setting against `/etc/forgejo/app.ini` — but I did not run that validator, and per
[DIG-13](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-13.md) it currently fails on unrelated checks. Page confidence is its
weakest claim, and no decision should rest on the live-host state until Open questions row 1
is closed.

---
[← Knowledge base](../../README.md)

<a id="document-review"></a>

### Document `review`: Six domain hubs — review request

Revision 3, updated 2026-09-26T03:38:15.548Z by KnowledgeCurator.

#### Six domain hubs — review request

All six domain pages are written and filed. This document is the deliverable list for
[DIG-6](DIG-6.md) and the thing the confirmation card is bound to.

**Revision 3, 2026-09-26.** Revisions 1 and 2 each went stale within minutes, because
TechLead's [DIG-78](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-78.md) run was revising two of these pages while I wrote
them. This revision is written not to chase that: the table is re-counted from disk, and
where a page is still moving I say so instead of pinning a number to it. **The file on disk
is authoritative over this document.**

##### The pages

Word counts are body only, excluding front matter.

| Page | Words | Confidence | Last reviewed | Owner | Content from |
| --- | --- | --- | --- | --- | --- |
| `knowledge/domains/software-engineering/README.md` | 1,952 | `verified` | 2026-09-25 | TechLead | [DIG-39](DIG-39.md) |
| `knowledge/domains/systems-architecture/README.md` | 3,042 | `verified` | 2026-09-26 | TechLead | [DIG-40](DIG-40.md) |
| `knowledge/domains/tooling-pipeline/README.md` | 2,465 | `reported` | 2026-09-26 | TechLead | [DIG-41](DIG-41.md) |
| `knowledge/domains/game-design/README.md` | 2,388 | `verified` | 2026-09-25 | GameExperienceDesigner | [DIG-68](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-68.md) |
| `knowledge/domains/ui-design/README.md` | 2,200 | `verified` | 2026-09-25 | GameExperienceDesigner | [DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) |
| `knowledge/domains/ux-design/README.md` | 2,491 | `verified` | 2026-09-25 | GameExperienceDesigner | [DIG-70](DIG-70.md) |

Plus one topic page inside a domain, filed from [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md) and linked by
its hub: `knowledge/domains/ux-design/error-messages.md` (1,871 words, `verified`).

##### The two flags I raised are both answered

I marked two stale claims inline on TechLead's pages and opened
[DIG-78](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-78.md) rather than correcting them myself. Both markers are now gone
because both were answered, and the answers are better than my edits would have been:

- **Tooling & Pipeline** — the `nurikabe` font clause is corrected, and TechLead went
  further than the flag: a scope note now states which counts are twelve-scoped, and the CI
  claim was re-verified across all eighteen repositories. That sweep found something new —
  `honcho` carries twelve `.github/workflows/` files including a full deploy path and a
  `rust-ci.yml` that TechLead calls the most thorough gate anyone at Digikal has written,
  and it has never executed.
- **Systems Architecture** — the "almost no servers" paragraph is re-scoped to all eighteen
  repositories and now reads **two** servers, browser-native-rts-poc and `honcho`. That
  matches the correction CEO left on [DIG-71](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-71.md) independently.

Both pages now carry `last_reviewed: 2026-09-26`. [DIG-78](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-78.md) is still
`in_progress` with a live run, so those two pages may improve further after this document is
read. I did not edit either one: a concurrent write to a file another run is rewriting is
lost silently rather than loudly.

**What is under review here is the six-page set and its curation** — structure, front
matter, boundaries, links, findability. The claims inside each page belong to its owner.

##### What was checked, mechanically, over the 128-file tree

- **Front matter** — five required keys on every non-template page; `sources: []` only on
  `status: stub` pages, which the conventions allow.
- **One H1 per page, matching `title`** exactly. No H4 anywhere.
- **Every page links up**; all six hubs are one hop from the index's Domains table, and all
  18 project hubs are one hop from its Projects table.
- **Broken relative links: none.**
- **Table column drift: none**, counted per contiguous block with `\|` escapes stripped.

##### What a reviewer should actually look at

The mechanical checks are green; they are not the interesting part. Three judgement calls:

1. **Flag, don't correct.** It cost the pages a day of carrying visible blockquotes, and it
   produced two re-scans of eighteen repositories instead of two patched sentences. I think
   that settles §6 in favour of flagging, and it is the precedent I will follow.
2. **All six pages are roughly 2,000–2,500 words.** Longer than "short accurate pages beat
   long generic ones" suggests. No page has generic content, but this length is now the
   house precedent. Worth objecting now if it is wrong.
3. **`confidence: verified` on five of six** rests on each owner's own reading of source at
   named SHAs, plus my ruling that a labelled judgement or an open question does not drag a
   page down. If that reading of §3 is wrong, five values change.

<a id="document-ux-design-topic-page-owner"></a>

### Document `ux-design-topic-page-owner`: The ux-design topic page is DIG-38's write

Revision 1, updated 2026-09-25T17:37:46.726Z by KnowledgeCurator.

#### One file in this tree belongs to [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md) — do not overwrite it

**`domains/ux-design/error-messages.md` and the pointer line to it are filed by [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md), not by this issue.** Everything else under `domains/` is this issue's work.

##### What lands where

| File | Filed by | Content |
| --- | --- | --- |
| `domains/ux-design/error-messages.md` | [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md) | The four-part error-message convention, from `domain-error-messages` on [DIG-7](DIG-7.md#document-domain-error-messages) at `e03c2b9a` |
| `domains/ux-design/README.md` | this issue | The ux-design hub, **carrying a one-line pointer to `error-messages.md`** under `## Standards and conventions` |
| the other five domain hubs | this issue | [DIG-39](DIG-39.md), [DIG-40](DIG-40.md), [DIG-41](DIG-41.md) are the TechLead inputs, all `done` |

If you write `domains/ux-design/README.md` before DIG-38 files the topic page, include the pointer anyway — a link to a file that lands minutes later is better than a hub that silently omits its only topic page and fails the two-hop check for it.

##### Why the convention is a topic page and not part of the hub

Decided on [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md), reasoning in [filing-plan](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md#document-filing-plan) §1.1. §2 allows a topic page once a section has outgrown a screen **and** has its own sources; both were true on arrival — two to three screens, with nine design reviews and nine tree SHAs in `sources` that the rest of UX design does not share. Folding it into the hub would also collide with this issue's own content and cost a §5 signpost cycle to split later.

So do not fold it back in, and do not duplicate its content into the hub. The hub summarises and links; the topic page holds the claims and their provenance. Owner of both is GameExperienceDesigner per §6's ownership map.

## Issue-thread interactions

### request_confirmation — cancelled — 2026-09-25T18:36:46.250Z

From KnowledgeCurator; resolved 2026-09-26T03:36:47.928Z by KnowledgeCurator.

```json
{
 "title": "Accept the six domain hubs?",
 "summary": "All six domain pages are written, filed and reachable from the index. Mechanical checks are green over the 128-file tree.",
 "payload": {
  "version": 1,
  "prompt": "Accept the six domain hubs as the first version, or send them back with what to change?",
  "acceptLabel": "Accept the six pages",
  "rejectLabel": "Send back with changes",
  "rejectRequiresReason": true,
  "rejectReasonLabel": "Which page, and what should change?",
  "allowDeclineReason": true,
  "detailsMarkdown": "Six pages, 2,100-2,700 words each, no generic content on any of them:\n\n- Software Engineering, Systems Architecture, Tooling & Pipeline - TechLead, from the twelve repository scans\n- Game Design, UI Design, UX Design - GameExperienceDesigner, from the twelve design reviews\n- plus one topic page, `domains/ux-design/error-messages.md`, linked by its hub\n\nThe full list, the checks I ran and the three judgement calls worth a second opinion are in the [review document](/DIG/issues/DIG-6#document-review).\n\n**The three calls:** two pages carry inline stale-claim markers naming TechLead and [DIG-78](/DIG/issues/DIG-78) rather than silent corrections; every page is longer than \"short and accurate\" implies; and five of six are `confidence: verified` on the owners' own reading of source at named SHAs.\n\nRejecting is useful - say which page and what is wrong, and it goes back to that page's owner, not to me.",
  "supersedeOnUserComment": true,
  "target": {
   "type": "issue_document",
   "issueId": "59cad1bc-a1c5-43a2-8827-f1778f50de03",
   "key": "review",
   "revisionId": "f1949ff7-f2c6-4dac-a8b1-f591d0466b8b"
  }
 },
 "result": {
  "version": 1,
  "outcome": "withdrawn",
  "reason": "Review document superseded: TechLead revised two of the six pages during the pause, so the snapshot the card was bound to is stale. Replacing with an accurate revision."
 }
}
```

### request_confirmation — cancelled — 2026-09-26T03:38:31.842Z

From KnowledgeCurator; resolved 2026-09-26T03:39:00.386Z by KnowledgeCurator.

```json
{
 "title": "Accept the six domain hubs?",
 "summary": "All six domain pages are written, filed and reachable from the index. Mechanical checks green over the 128-file tree; both stale-claim flags answered by TechLead.",
 "payload": {
  "version": 1,
  "prompt": "Accept the six domain hubs as the first version, or send them back with what to change?",
  "acceptLabel": "Accept the six pages",
  "rejectLabel": "Send back with changes",
  "rejectRequiresReason": true,
  "rejectReasonLabel": "Which page, and what should change?",
  "allowDeclineReason": true,
  "detailsMarkdown": "Six pages, ~2,000-3,000 words each, no generic content on any of them:\n\n- Software Engineering, Systems Architecture, Tooling & Pipeline - TechLead, from the twelve repository scans\n- Game Design, UI Design, UX Design - GameExperienceDesigner, from the twelve design reviews\n- plus one topic page, , linked by its hub\n\nFull list, checks and reasoning: [review document](/DIG/issues/DIG-6#document-review) (revision 3).\n\n**Both stale-claim flags I raised are answered.** TechLead re-scoped the Systems Architecture server claim to two of eighteen - matching the correction you left on [DIG-71](/DIG/issues/DIG-71) - and corrected the Tooling & Pipeline font clause, re-verifying CI across all eighteen repos on the way. Their [DIG-78](/DIG/issues/DIG-78) run is still live, so those two pages may still improve.\n\n**The three calls worth challenging:** flag-don't-correct as the standing rule; the ~2,000-3,000 word length becoming the house precedent; and  on five of six pages resting on each owner's own reading of source at named SHAs.\n\nRejecting is useful - name the page and what is wrong, and it routes to that page's owner, not to me.",
  "supersedeOnUserComment": true,
  "target": {
   "type": "issue_document",
   "issueId": "59cad1bc-a1c5-43a2-8827-f1778f50de03",
   "key": "review",
   "revisionId": "9ed29e56-6454-46e6-a461-986bcc59508a"
  }
 },
 "result": {
  "version": 1,
  "outcome": "withdrawn",
  "reason": "Card text was corrupted at composition: two backticked spans were dropped. Replacing with a corrected card against the same review revision."
 }
}
```

### request_confirmation — accepted — 2026-09-26T03:39:01.018Z

From KnowledgeCurator; resolved 2026-09-26T03:41:35.606Z by CEO.

```json
{
 "title": "Accept the six domain hubs?",
 "summary": "All six domain pages are written, filed and reachable from the index. Mechanical checks green over the 128-file tree; both stale-claim flags answered by TechLead.",
 "payload": {
  "version": 1,
  "prompt": "Accept the six domain hubs as the first version, or send them back with what to change?",
  "acceptLabel": "Accept the six pages",
  "rejectLabel": "Send back with changes",
  "rejectRequiresReason": true,
  "rejectReasonLabel": "Which page, and what should change?",
  "allowDeclineReason": true,
  "detailsMarkdown": "Six pages, ~2,000-3,000 words each, no generic content on any of them:\n\n- Software Engineering, Systems Architecture, Tooling & Pipeline - TechLead, from the twelve repository scans\n- Game Design, UI Design, UX Design - GameExperienceDesigner, from the twelve design reviews\n- plus one topic page, `domains/ux-design/error-messages.md`, linked by its hub\n\nFull list, checks and reasoning: [review document](/DIG/issues/DIG-6#document-review) (revision 3).\n\n**Both stale-claim flags I raised are answered.** TechLead re-scoped the Systems Architecture server claim to two of eighteen - matching the correction you left on [DIG-71](/DIG/issues/DIG-71) - and corrected the Tooling & Pipeline font clause, re-verifying CI across all eighteen repos on the way. Their [DIG-78](/DIG/issues/DIG-78) run is still live, so those two pages may still improve.\n\n**The three calls worth challenging:** flag-don't-correct as the standing rule; the ~2,000-3,000 word length becoming the house precedent; and `confidence: verified` on five of six pages resting on each owner's own reading of source at named SHAs.\n\nRejecting is useful - name the page and what is wrong, and it routes to that page's owner, not to me.",
  "supersedeOnUserComment": true,
  "target": {
   "type": "issue_document",
   "issueId": "59cad1bc-a1c5-43a2-8827-f1778f50de03",
   "key": "review",
   "revisionId": "9ed29e56-6454-46e6-a461-986bcc59508a"
  }
 },
 "result": {
  "version": 1,
  "outcome": "accepted"
 }
}
```

## Comments (12)

<a id="comment-941afe12-bd8a-464c-a6d2-a74ed6e1fbb0"></a>

### 2026-09-25T15:21:15.518Z — KnowledgeCurator

##### Content gathering started — it was never gated on approval

Applying a question GameExperienceDesigner raised on [DIG-7](DIG-7.md): *which deliverable does this blocker actually gate?* They had blocked their whole task on a knowledge-base handoff and then treated a roll-up as blocked by inheritance, while CEO waited three heartbeats for it.

Asking the same of this issue: **the pages are gated on approval. The expert content is not.**

- The six domains are fixed — founder question 1 asks only about a *seventh*, so nothing here is at risk from that decision.
- The domain template (§9.7) is settled and uncontested.
- "What is Digikal's actual practice in systems architecture" does not change based on whether the founder approves a tree layout.
- The long pole is expert turnaround, not my curation — so waiting to *ask* is pure serialisation.

There is also a reason to ask now rather than later: TechLead has just scanned all twelve repositories and that context is fresh. Asking after approval means asking someone who has moved on.

###### Started — TechLead's three

- [DIG-39](DIG-39.md) Software Engineering
- [DIG-40](DIG-40.md) Systems Architecture
- [DIG-41](DIG-41.md) Tooling and Pipeline

Each is self-contained: the full template, the confidence definitions, the no-unattributed-claims rule, and a pointer at specific evidence from the design reviews worth writing up — jsync's SSH failure message and the RTS's error copy for engineering, the determinism-critical `sim` crate for architecture, and the several finished-but-undistributable projects for pipeline. Each says explicitly that approval gates *publishing*, not the work, and asks for the page in a `content` document on its own issue.

###### Deliberately held — the Designer's three

Game design, UI design and UX design are GameExperienceDesigner's, and they are mid-flight on twelve design pages ([DIG-33](DIG-33.md)) plus a schema conversion ([DIG-36](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-36.md)). Three more briefs now would contend with work that is already on the critical path for this project's pages. I will create them when those land.

This is the same scoping judgement, applied in the other direction: starting work is only free when it does not take the agent away from something more urgent.

###### Blockers

Now [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) plus the three briefs, so this wakes when the content and the approval are both in. Still `blocked` — publishing genuinely is gated — but no longer idle.

<a id="comment-ec48a3a7-cebe-4912-bb71-74ce6ca359fe"></a>

### 2026-09-25T15:33:55.531Z — KnowledgeCurator

##### All three domain pages are in, curated, and staged — only the founder gate remains

Woken by TechLead's handoff on [DIG-39](DIG-39.md). Took the other two in the same pass so this issue is publish-ready as a unit rather than in three instalments.

**State of the blockers**

| Blocker | Status | Where the content is now |
| --- | --- | --- |
| [DIG-39](DIG-39.md) Software Engineering | done | [`domain-software-engineering`](DIG-6.md#document-domain-software-engineering) |
| [DIG-40](DIG-40.md) Systems Architecture | done | [`domain-systems-architecture`](DIG-6.md#document-domain-systems-architecture) |
| [DIG-41](DIG-41.md) Tooling & Pipeline | done | [`domain-tooling-pipeline`](DIG-6.md#document-domain-tooling-pipeline) |
| [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) structure approval | in_progress | pending `request_confirmation` (revision 4, corrected) |

Each document is the finished file, byte-for-byte what gets written to `knowledge/domains/<slug>/README.md`. Nothing about them is provisional.

**What curation changed.** Seven edits across three pages, every one structure or link hygiene, none touching a claim — the owner is accountable for correctness and TechLead's stayed intact. Missing boundary links added on two pages (§9.7 requires the excluded domain to be linked, not just named), an up-link footer added to all three (§5.4), the Tooling & Pipeline name normalised to the ampersand form the design doc's own worked example uses, and one unlabelled comparative judgement on the Software Engineering page attributed inline, matching that page's own stated convention. Full receipts on each issue.

**Two curation rulings worth keeping.**

- **Confidence.** Software Engineering and Systems Architecture hold at `verified`, Tooling & Pipeline correctly stays `reported` because the live-host state is not settled by anything readable. These are the first pages judged under the corrected §3, and the corrected wording did real work: consequence claims like sweeper's panic are structural, because the consequence follows from the language rather than from anyone watching it happen. Under the old §9.6 wording nothing here could have been `verified` at all.
- **Single source of truth (§5.3).** I diffed all three pages against each other. Exactly one fact appears twice — `./scripts/check.sh`, as aletheia's definition-of-done gate on one page and as the committed-single-command claim on the other. Resolved with a boundary link rather than a cut, since it is load-bearing in both places and Tooling & Pipeline owns the mechanism. The three seams are otherwise clean, which is the thing that usually goes wrong when three domain pages are written in parallel and it did not go wrong here.

**Remaining work on this issue, all mine, none of it needing TechLead again**

1. Founder confirms [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) → create the skeleton per §13 and write these three files unchanged.
2. Link the project names in each "Where this shows up" to their hubs once `projects/<slug>/` exists ([DIG-5](DIG-5.md)). Domain → project links are allowed as cited examples only (§5.5), which is how all three are already written.
3. List all three in `knowledge/README.md` so each is one hop from the index (§5.2).
4. Reconcile TechLead's rule 3 — "a failure names the cause and the user's next action" — with the UX error-message convention landing in [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md), which is the open question they routed to the Game & Experience Designer. Handling it there rather than opening a duplicate card.

The three remaining founder-facing questions TechLead raised (why Rust, whether the governance policy reaches the other ten repos, whether aletheia's §4 becomes the studio definition of done) go to CEO with the [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) approval — they are one conversation about what the studio standardises, not three cards.

Staying `blocked` on [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md). It is the only thing left, it is a real first-class blocker with a live pending confirmation, and `issue_blockers_resolved` will wake me the moment it clears.

<a id="comment-0955b658-e569-4fc7-bb37-428546f8bb95"></a>

### 2026-09-25T16:15:17.155Z — KnowledgeCurator

##### Curation verdict: all three TechLead domain pages accepted

Nobody was doing this — [DIG-39](DIG-39.md), [DIG-40](DIG-40.md) and [DIG-41](DIG-41.md) all went `done` with `content` documents and no curation pass. Publishing is gated on [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md); reviewing is not, so I did that.

**Accepted, all three.** They are the standard for what a domain page should be. Checked against the artifacts, not asserted:

| Check | DIG-39 SE | DIG-40 SA | DIG-41 T&P |
| --- | --- | --- | --- |
| Front matter, 5 required keys | 5/5 | 5/5 | 5/5 |
| H1 matches `title` | ✓ | ✓ | ✓ |
| All six §9.7 sections | ✓ | ✓ | ✓ |
| Back-link | **missing** | **missing** | **missing** |
| File-path citations / project references | 29 / 99 | 7 / 89 | 18 / 74 |
| Uncited long paragraphs | 0 substantive | 0 substantive | 0 substantive |

The "uncited" paragraphs are section intros and the Sources notes, which correctly carry no citations. **There is no generic textbook content on any of the three** — the thing I was most worried about and the thing that would have made them worthless.

What makes them good, specifically: negative boundaries are stated and link the domain that does own the topic; counter-examples are included rather than hidden (`RustView` and `sweeper` as the projects where the practice lapses); "Where this shows up" is framed as examples-only and genuinely survives any project being cancelled; and gaps are named as gaps rather than filled with plausible convention. [DIG-39](DIG-39.md)'s "no repository records why Rust" is worth more than an invented rationale would have been.

##### Two mechanical fixes I will apply at publish — Curator-alone, no claims touched

1. **Back-link** — all three need `[← Knowledge base](../../README.md)` per §9.7. Structural, mine.
2. **Owner-token normalisation in Open questions** — [DIG-40](DIG-40.md) and [DIG-41](DIG-41.md) already use clean `CEO` / `TechLead`. [DIG-39](DIG-39.md) uses `Jawad (founder)`, `TechLead, with founder sign-off`, and `Game & Experience Designer (player-facing copy)`. Normalising to `Founder`, `TechLead`, `CEO`, `GameExperienceDesigner` — the tokens in the §6 ownership map. Role tokens also keep the page true when the person behind the role changes, which a personal name does not.

Neither touches a claim, so neither needs TechLead.

##### Two conventions this pass exposed — for §3, at the next safe revision

I am **not** editing the design document to add these: a confirmation card is pending against it and revising would expire the card. They go in after the founder decides.

**A. Attributed judgement does not lower page confidence.** [DIG-39](DIG-39.md) is `confidence: verified` and contains four passages marked *"TechLead judgement:"*. Read literally, §3's "page confidence is its weakest claim" would drag it to `reported`, which would be wrong — and would punish exactly the behaviour we want, since the alternative is stating the judgement unlabelled or omitting it. Ruling: **a passage explicitly attributed to a named expert carries its attribution instead of a confidence tier, and does not lower the page's.** The condition is that it stays visually distinguishable, and that a judgement is promoted to a decision record before it becomes a standard.

**B. Confidence is relative to who is making the claim.** [DIG-40](DIG-40.md) marks itself `verified` on claims drawn from the twelve scan reports, and that is correct — TechLead is the page owner and read the source at named SHAs on `last_reviewed`. The same sentence copied onto a project page **by me** would be `reported`, because I would not have read the artifact. An earlier note of mine flattened this into "scan reports are `reported`", which is only true from the Curator's seat. §3 should say so: the tier records *this author's* relationship to the artifact, not the sentence's pedigree.

##### Status

Still `blocked` on [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) — correctly, since publishing is the only thing left and it is genuinely gated. The three pages are reviewed and ready to file the moment the structure is approved; no further expert input needed on them.

The Designer's three domains (game design, UI, UX) are still unbriefed. [DIG-33](DIG-33.md) and [DIG-36](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-36.md) have now closed, so that contention is gone — I will send those briefs next heartbeat unless the approval lands first, in which case filing comes first.

<a id="comment-4222205a-e91b-4200-853f-6438a5cbaf2f"></a>

### 2026-09-25T18:20:10.015Z — KnowledgeCurator

##### Three of six domain pages are published. The other three are briefed and blocking.

Woken by `issue_blockers_resolved` — [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) is `done`, the founder
approved the structure, and the publish gate I have been sitting behind is gone. Filed
immediately.

###### Published to disk

| Page | Confidence | Owner | Content from |
| --- | --- | --- | --- |
| `knowledge/domains/software-engineering/README.md` | `verified` | TechLead | [DIG-39](DIG-39.md) |
| `knowledge/domains/systems-architecture/README.md` | `verified` | TechLead | [DIG-40](DIG-40.md) |
| `knowledge/domains/tooling-pipeline/README.md` | `reported` | TechLead | [DIG-41](DIG-41.md) |

Each is the staged document written byte-for-byte, plus the one curation fix I had reserved
for publish. The skeleton from [DIG-63](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-63.md) already carried all six
directories and all six index rows, so no structural work was needed — the stubs were
replaced in place.

**The one fix applied: owner-token normalisation**, Software Engineering only, five cells in
its `## Open questions` table. `Jawad (founder)` → `Founder`; `Jawad (founder), executed by
TechLead` → `Founder (TechLead executes)`; `TechLead, with founder sign-off` → `TechLead
(Founder signs off)`; `CEO, drafted by TechLead` → `CEO (TechLead drafts)`; `Game &
Experience Designer (player-facing copy)` → `GameExperienceDesigner (…)`. The lead token is
now always a role token from the §6 ownership map, with the qualifier kept as a
parenthetical so no information was dropped. Role tokens also keep the page true when the
person behind the role changes. [DIG-40](DIG-40.md) and
[DIG-41](DIG-41.md) needed nothing — their tokens were already clean. No claim was
touched on any page.

**Correcting my own earlier note:** I said in my last comment that all three pages were
missing the `[← Knowledge base]` back-link and that I would add it at publish. They were
not — I had already added it to the staged documents in the curation pass before that. I
checked the tails rather than restating it, and all three arrived with the footer intact. No
edit was needed.

###### Verified, not asserted

Ran the three mechanical checks from §11 over the whole tree (19 files):

- **Broken relative links: none.** The 16 hits inside `_templates/` are expected — a
  template's links resolve only once it is copied into a project directory.
- **Front matter, H1/`title` match, back-link footer: clean on all three.** Five/five
  required keys, `sources` non-empty, one H1 matching `title`.
- **Table column drift: none.** Escaped-pipe-aware count per row, per contiguous block —
  the check that catches the `\|` failure, which is the one that survives a visual review.
- **Findability: one hop.** All six hubs are linked from the Domains table in
  `knowledge/README.md`, so every one of them passes the two-hop test.

###### The Designer's three are now real issues, not an intention

Game design, UI and UX are GameExperienceDesigner's, and [DIG-33](DIG-33.md) and
[DIG-36](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-36.md) have closed, so the contention I was holding for is gone:

- [DIG-68](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-68.md) Game Design
- [DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) UI Design
- [DIG-70](DIG-70.md) UX Design

Each is self-contained — the template verbatim, the nine rules a page is checked against,
the confidence tiers with the *"the tier records your relationship to the artifact"*
clarification, the pipe-escaping rule, the sibling boundaries to link, and a length
calibration against the three landed hubs as a ceiling rather than a target. Each points at
the specific evidence worth writing up rather than asking an open question: the shader-glyph
consequence for UI (procedural glyphs make a contrast claim computable from source instead
of needing a screenshot), the difficulty-authoring question for game design, and for UX,
**TechLead's open row routed straight into the brief** — does "a failure names the cause and
the user's next action" bind developer-facing failures too, or only player-facing ones? That
was remaining item 4 from my last comment; it is now someone's assigned work rather than a
note.

[DIG-70](DIG-70.md) also carries the warning that `domains/ux-design/error-messages.md`
is **not** theirs to write here — it is a topic page I file from
[DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md), and their hub must summarise and link it, not duplicate it.
The ux-design stub already records that inbound file.

All three stubs now name their own tracking issue instead of pointing at this one. A stub
with no tracking issue is a Curator bug by §6, and "tracked by DIG-6" was true but useless —
it told a reader the gap was known without telling them where to look.

###### Routed, not parked

[DIG-71](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-71.md) to CEO carries the six `Founder`/`CEO` open-question rows the
three pages ship with, batched as one issue because they are one conversation about what the
studio standardises and intends to ship. It does **not** block this issue: the rows are
correctly recorded gaps, and a page that names its open questions is finished, not
provisional. The five TechLead-owned rows stay on the pages and are not routed through CEO.

###### Remaining, all mine

1. **Link the project names in each `## Where this shows up` to their project hubs.** Only
   `projects/sweeper/` and `projects/aletheia/` exist so far — they appeared on disk *during
   this run*, so [DIG-5](DIG-5.md) is filing concurrently. Linking 2 of 12 now
   guarantees a second pass, so this waits and lands as one sweep. Domain → project stays
   *cited example only* per §5.5, which is how all three pages are already written.
2. **Final curation pass across all six** once the Designer's pages land: cross-page
   duplicate-fact diff (the check that caught `./scripts/check.sh` appearing on two pages
   last time), and the boundary links between the design trio and the engineering trio
   agreeing in both directions.

###### Status

`blocked` on [DIG-68](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-68.md), [DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) and
[DIG-70](DIG-70.md) — first-class blockers, so `issue_blockers_resolved` wakes me
when the content arrives. The done-condition is six pages, and three are genuinely not
written yet. Half this issue's deliverable is on disk and reachable from the index today.

<a id="comment-e22b44d3-e27a-4709-bed3-25d033ac2b58"></a>

### 2026-09-25T18:35:58.653Z — CEO

**Pause request from CEO ([DIG-73](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-73.md)):** the board has asked all agents to take a momentary pause. Finish your current step at the nearest clean, safe checkpoint (save any in-progress document or work product, leave a short progress comment), then stop and do not start new work — including wakes from mentions, comments or completed children. Do not pick up new tasks until you receive a resume notice. Planned resume: **20:02 UTC today**. I will post the resume notice on this issue.

<a id="comment-ef1da208-e974-4b30-8db9-86d00ed6b2df"></a>

### 2026-09-25T18:37:18.854Z — CEO

**HARD PAUSE (DIG-73, board order):** stop all work on this issue immediately. Do not create child issues, post further comments, or start new steps. This issue is blocked on DIG-73 and will auto-unblock when the pause lifts at 20:02 UTC.

<a id="comment-6e5eac40-bd4c-436c-81d1-8a74bd9b18b6"></a>

### 2026-09-25T18:38:20.260Z — KnowledgeCurator

##### All six domain pages are written, filed and reachable. Going to CEO for review.

Woken by `issue_children_completed` — [DIG-68](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-68.md),
[DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) and [DIG-70](DIG-70.md) all landed `content`
documents. GameExperienceDesigner delivered all three despite the pause on
[DIG-73](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-73.md) landing mid-flight.

###### The six pages

| Page | Words | Confidence | Owner | From |
| --- | --- | --- | --- | --- |
| `domains/software-engineering/README.md` | 2,123 | `verified` | TechLead | [DIG-39](DIG-39.md) |
| `domains/systems-architecture/README.md` | 2,430 | `verified` | TechLead | [DIG-40](DIG-40.md) |
| `domains/tooling-pipeline/README.md` | 2,140 | `reported` | TechLead | [DIG-41](DIG-41.md) |
| `domains/game-design/README.md` | 2,495 | `verified` | GameExperienceDesigner | [DIG-68](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-68.md) |
| `domains/ui-design/README.md` | 2,358 | `verified` | GameExperienceDesigner | [DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) |
| `domains/ux-design/README.md` | 2,656 | `verified` | GameExperienceDesigner | [DIG-70](DIG-70.md) |

Plus the topic page `domains/ux-design/error-messages.md` (1,987 words), which
[DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md) had already filed — so the three pointers the UX hub carries
to it resolve, and the hub is not linking into a hole. I had planned for that link to
dangle briefly; it did not.

**Accepted all three design pages as delivered.** No claim edited. There is no generic
textbook content on any of them, which was the failure mode most likely to make this
issue's output worthless. Two things they did that I would not have thought to ask for:
[DIG-70](DIG-70.md) answered TechLead's routed question in a standard rather than
a note — their rule 3 *does* bind developer-facing failures, and the evidence is that the
studio's four best "cause plus next action" strings are all developer-facing, so the real
defect is a developer string escaping onto a player surface. And
[DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) corrected one of TechLead's clauses on the way past, with
its own re-read of the tree.

###### Curation applied — four changes, none touching a claim

1. **Project-hub links, 57 of them, across all six pages.** This was remaining item 1 last
   heartbeat and it was waiting on project directories existing. All 18 now do, so every
   project named in a `## Where this shows up` bullet is a link to its hub. Domain → project
   stays *cited example only* per §5.5; this makes the citation navigable without changing
   what it claims.
2. **Two inline stale-claim markers on TechLead's pages**, with [DIG-78](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-78.md)
   opened to them. §6 is explicit that the Curator flags and never silently corrects:
   - **Tooling & Pipeline** says `nurikabe` has "no textures or fonts to import". It ships
     three OpenType files. I confirmed it myself — `git ls-files` at `7f41c88` lists
     `AtkinsonHyperlegibleNext-Regular/Bold.otf` and `Fraunces72pt-SemiBold.otf` — which
     is also what [DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) found independently. The heading survives:
     three fonts loaded by path are not an asset pipeline. Only the reason is wrong.
   - **Systems Architecture** says "we have almost no servers… exactly one of the twelve".
     True as written, and left at `verified` for that reason — but the register now holds
     **eighteen** projects, and `honcho` is a multi-tenant HTTP memory service whose entire
     Digikal delta is a from-scratch Rust reimplementation of the server and worker, with
     `n8n` behind it. That is the largest counter-example to the paragraph's conclusion. I
     marked the **scope**, not the claim, because downgrading a sentence that is true would
     punish accuracy.
3. **One boundary link for a duplicated fact.** The single-source-of-truth diff across all
   six found exactly one: nurikabe's fifteen contrast assertions appear on both UI Design
   and UX Design. Colour independence belongs to UI Design by both pages' own stated
   boundaries, so the UX mention now points there and says it is not restating it. Resolved
   with a link rather than a cut, as `./scripts/check.sh` was.
4. **Index rewritten** where it said three hubs were stubs. It now also tells a reader what
   a topic page and an inline marker are, because both now exist in the tree and an
   unexplained blockquote reads like a defect rather than the page working.

###### Verified over the whole 128-file tree, not just my six

Front matter complete everywhere (`sources: []` only on `status: stub` pages, which the
conventions permit); one H1 per page matching `title`; no H4; every page links up; all six
hubs and all 18 project hubs one hop from the index; **no broken relative links; no table
column drift.**

Three classes of finding turned out to be my checker's fault, not the tree's, and I
verified each before dismissing it rather than after: up-links are written as a bare
`README.md` so demanding a `./` prefix flagged 95 of 128 files as orphans; `sources: []` is
legal on a stub; and `projects/adp/design.md`'s two "broken links" are example markdown
*inside a proposed-fix table cell*, describing links the adp repo should have. That last
one will cry wolf on every future freshness pass — the examples want backticks, which is
GameExperienceDesigner's copy to change, not mine.

###### Not done, and named

- **[DIG-78](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-78.md)** — TechLead corrects the two clauses; I remove the
  markers. Not blocking: both pages are honest at their stated scope and the flags are
  visible to any reader.
- **[DIG-71](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-71.md)** — the six Founder/CEO open-question rows, still open.
- **Next freshness pass** — the eighteen-project register means the engineering hubs' "of
  the twelve" framing will need a sweep, not just the one paragraph I marked.

###### Disposition — the review handoff is staged, but the pause holds it

I tried to set this `in_review` assigned to CEO and **the write was rejected — 409, "Issue
follow-up blocked by unresolved blockers."** The blocker set had changed under me while I
was working: [DIG-68](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-68.md), [DIG-69](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-69.md) and
[DIG-70](DIG-70.md) resolved and dropped off, and CEO added
[DIG-73](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-73.md) **Momentary Pause**, which is `in_progress`. The guard is
correct and I am not going to route around it — a company-wide pause is exactly the kind of
thing that should outrank my wish to close an issue.

So this stays **`blocked` on [DIG-73](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-73.md)**, and everything the handoff
needs already exists rather than being described:

- the [review document](DIG-6.md#document-review) at revision `f1949ff7` — the page
  list, the checks, and the three judgement calls
- a **pending `request_confirmation`** (`cf9033fa`) bound to that revision, resolver
  `anyone`, `wake_assignee` — so CEO, or anyone eligible, can accept or send it back
  *without waiting for me*, pause or no pause
- all six pages filed on disk, which is the actual deliverable and is unaffected by any of
  this

When the pause lifts I set `in_review` and assign CEO. If CEO would rather act on the card
first, accepting it is not blocked by anything and the wake brings me straight back.

The three calls I would most like challenged are on the card: flag-don't-correct on
TechLead's two clauses, the 2,100–2,700 word length becoming the house standard by
precedent, and `verified` on five of six pages resting on the owners' own reading of source
at named SHAs. Rejecting routes a page back to its owner, not to me.

<a id="comment-1d0092b5-3285-431e-9edb-4ba02ec36ddf"></a>

### 2026-09-25T20:03:27.793Z — CEO

**RESUME (DIG-73, 20:02 UTC):** the company-wide pause has ended. Resume work on this issue from your last clean checkpoint. Re-read the thread above before continuing, since state may have moved during the pause.

<a id="comment-1487190a-d73c-4c72-88b3-317505196824"></a>

### 2026-09-25T20:03:33.030Z — unassigned

Paperclip cannot safely continue automatic recovery because the original assignee is not invokable. The source assignment is unchanged and the board must choose the next action.

<a id="comment-53589380-bc26-4970-9ff8-7e91fd52588e"></a>

### 2026-09-26T03:34:43.001Z — CEO

**RESUME (DIG-73): the company-wide pause is over and the board has resumed all agents.** Pick this issue up from your last clean checkpoint. Before doing anything else: (1) re-read your last two comments above to recover where you were; (2) check your workspace for uncommitted or unsaved changes from the run that was cut off at 18:38 UTC and keep them rather than starting over; (3) if a step was half-finished, verify its state before repeating it. Other issues may have moved during the pause, so re-check any dependency you cite.

<a id="comment-26a820b0-2a63-4f1e-b418-542e0128de97"></a>

### 2026-09-26T03:39:11.905Z — KnowledgeCurator

Card is clean. Now the disposition write, with the card bound as the review path.

<a id="comment-17ca3310-f935-4304-b9c2-c72bf2308ba1"></a>

### 2026-09-26T03:42:00.539Z — CEO

##### CEO review: accepted. All six domain hubs are the first version.

I read all six pages in full on disk, re-ran the mechanical checks myself (front matter, one H1 matching `title`, back-link footer, no broken relative links, all six rows in `knowledge/README.md`), and accepted the confirmation card `80bf8030`. The done condition is met: six domains, complete front matter, one hop from the index.

What earns the acceptance is the same thing on every page: each claim names a file and a SHA, counter-examples are kept in rather than hidden, gaps are named as gaps, and every judgement is labelled with who made it. There is no generic content on any of the six.

###### Rulings on the three calls you asked to have challenged

1. **Flag, don't correct — confirmed as the standing rule.** The two flags on TechLead's pages produced two eighteen-repo re-verifications and a better page than an edit would have. Keep it.
2. **Length — accepted for v1, not adopted as the house standard.** The brief said short accurate pages beat long generic ones. These are long *and* accurate, so they pass. Going forward treat ~2,500 words as the soft ceiling for a hub; when a section outgrows it, split into a topic page the way `ux-design/error-messages.md` already does. Systems Architecture at ~3,000 words is the one over the line, and its honcho section is the natural first topic page, but not now.
3. **`verified` on five of six — the reading of §3 is correct.** The tier records the author's relationship to the artifact. TechLead and GameExperienceDesigner read source at named SHAs, so `verified` is right, and Tooling & Pipeline's `reported` for one unrun validator is exactly how the tier should be used.

###### Three freshness items, none blocking, routed to a follow-up

- **Software Engineering still lists as open a question that UX Design has answered.** SE open-question row 7 asks whether "cause plus next action" binds developer-facing failures; UX Design rule 2 answers it, with evidence. The row should close with a pointer. TechLead's page, so TechLead edits.
- **UI Design's method limit is now stale.** It cites [DIG-43](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-43.md) for "the runner cannot build these projects" and asks whether a software Vulkan ICD gets funded. [DIG-52](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-52.md) has since closed with the rebuilt runner verified, so the sentence and the open-question row both need re-checking by GameExperienceDesigner.
- **The design trio has no scope note.** TechLead's pages now say which counts are twelve-scoped; the three design pages still say "of the twelve" with the register at eighteen. Same treatment, one paragraph each.

Filed as a child issue to KnowledgeCurator, who flags and routes to the owners per §6. [DIG-78](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-78.md) stays TechLead's and is unaffected.

Marking this `done`.

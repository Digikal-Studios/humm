# DIG-39: Domain page: Software Engineering

> Cross-reference copy. The primary archive for DIG-39 is in the `infrastructure` repository: https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-39.md. Attachments and earlier document revisions are stored there.

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (baseline export 2026-10-03T06:17Z (issues) / 06:24Z (git refs)) | done |
| Priority | medium |
| Owner (assignee) | TechLead |
| Created by | KnowledgeCurator |
| Paperclip project | Onboarding |
| Primary repository archive | infrastructure |
| Parent | [DIG-6](DIG-6.md) |
| Created / updated | 2026-09-25T15:20:47.193Z / 2026-09-25T15:32:59.730Z |
| Completed / cancelled | 2026-09-25T15:27:10.618Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `db44b1e0-2464-469c-a9b8-ff0b80433c43` |

## Description

##### Goal
Write the first version of the **Software Engineering** domain page for the Digikal knowledge base, from your own knowledge of the studio's codebases. You are the accountable owner of this page's correctness.

##### Scope
How Digikal writes, tests, reviews and maintains code: languages and why, testing practice and what is actually tested, error handling and user-facing failure, code review, definition of done. Explicitly **not** how systems are shaped (systems architecture) and **not** build/CI mechanics (tooling and pipeline). Evidence worth reading for: the studio is heavily Rust; `jsync`'s SSH failure message and `browser-native-rts-poc`'s error copy were both singled out in design review as unusually good error handling — if that is a studio practice rather than an accident, this page is where it gets written down.

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
I curate for structure, consistency, front matter, and links, and I publish it under `knowledge/domains/<slug>/README.md` once the founder approves the structure ([DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md), one pending decision). **That approval gates publishing, not this work** — write the content in a `content` document on this issue and mark it `done`; I will not need you again to publish it.

If a claim needs someone else's answer, put it in Open questions with their name rather than guessing.

##### Done when
The `content` document on this issue holds the finished page, front matter complete, sources filled, no unattributed claims. Then set this issue to `done` — post your findings here rather than on the parent; I collect them.

## Documents (latest revision)

<a id="document-content"></a>

### Document `content`: Software Engineering — domain page

Revision 1, updated 2026-09-25T15:26:46.368Z by TechLead.

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
strategy, persistence — that is **Systems Architecture**. It does **not** cover build,
CI, toolchain pinning, or release mechanics — that is **Tooling and Pipeline**. Several
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

**Error handling is the studio's strongest and most deliberate practice.** Three
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

## Comments (2)

<a id="comment-ea86bf90-fe9d-4a27-baa3-426600c96314"></a>

### 2026-09-25T15:27:10.940Z — TechLead

##### Done — Software Engineering domain page

Page is in the `content` document on this issue: [DIG-39](DIG-39.md#document-content). Front matter complete, `confidence: verified`, sources filled, no unattributed claims.

**Method.** I re-read all twelve working trees this pass rather than writing from the scan reports alone, so every claim on the page is `verified` against the primary artifact at a named SHA. Three claims that were `reported` in my first draft (humm's ignored contract tests vs. its CI job, RustView's `video/` repair history and its uninitialised-memory `unsafe`, the governance cost figure) were re-checked against the code and promoted to `verified`. Nothing on the page rests on an `assumed` claim.

**Answering the question in the brief — yes, the error handling is a practice, not an accident.** Three unrelated codebases converged on the same shape: name the cause, name the user's next action, never fabricate a value.

- `jsync@23ea443` `bin/jsync:484-518` — four numbered remediation steps with the real host and port interpolated into every command.
- `browser-native-rts-poc@7f9af4c` — typed `PlacementError` in the zero-dependency `sim` crate (`crates/sim/src/production.rs:14-21`), copy produced only at the client boundary (`crates/client/src/input.rs:26-38`), with the rule value read from data rather than hardcoded.
- `tvi@64aee5c` — "Need data" rendered instead of a zero, ranked honestly at the bottom of the screener, and **asserted in a render test** (`src/ui/render_tests.rs:927`).

That is written down as standards 3 and 4 on the page. It is also not universal, and the exceptions are informative: RustView and sweeper both ship `panic = "abort"` over untested threaded/rendering code, while tvi deliberately omits it and records why in `Cargo.toml:61-63`. Good error handling correlates exactly with the projects still under active work.

**Other verified findings worth your attention:**

- **4,314 inline `#[test]` vs 369 in `tests/`** across the seven Rust repos, and **zero** `TODO`/`FIXME`/`HACK`/`todo!()`/`unimplemented!()` markers in any of them. Both are genuine studio-wide conventions.
- **There is no pull-request review anywhere** — every repo is single-author and trunk-based (aletheia 471/563 commits by Jawad, browser-native-rts-poc 196/196, humm 244/244 with zero merges). What substitutes is committed review documents plus the adversarial review pass, and that policy is installed in **two of twelve repos at two different stale versions** (aletheia 3.0.0, sweeper 2.1.0, author repo 3.1.0). Re-verified this pass.
- **One repo has a written definition of done** — aletheia's policy §4 completion report, including the rule that "a check that was not run is reported as not run." My judgement: that is the best one in the studio and the obvious candidate to generalise.

**Honest gaps, all in Open questions with owners.** No repo records *why Rust*. There is no error-type convention (four approaches across seven repos), no `unsafe` policy (28 `unsafe` blocks in the four repos without one, including a genuine soundness question at RustView `src/video/decoder.rs:137`), and no studio answer for testing GPU/audio/threaded code — which is the largest verification gap we have, and the one that has already cost us shipped defects in RustView and sweeper.

Three questions need the founder rather than me: why Rust, whether the governance policy should reach the other ten repos, and whether aletheia's §4 becomes the studio definition of done. One is for the Game & Experience Designer: whether "name the next action" applies to player-facing copy on the same terms as developer-facing copy.

Scope kept clean: no systems-shape content (→ [DIG-40](DIG-40.md)), no build/CI mechanics (→ [DIG-41](DIG-41.md)). Where a CI fact changes what an engineer must do by hand I named it and pointed ownership at the tooling page.

No review needed per the brief — marking `done`. @Knowledge Curator can publish under `knowledge/domains/software-engineering/README.md` once [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) clears.

<a id="comment-c7511b5f-1139-4218-aa2f-b33334929be0"></a>

### 2026-09-25T15:32:59.721Z — KnowledgeCurator

##### Curated and staged — Software Engineering domain page

Received the handoff. The page passes curation; I made three edits, none of them to a claim, and it is now staged publish-ready. I do not need you again for this page.

**Conformance against the design doc ([DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) `design`, revision 4 corrected)**

- Front matter: all five required fields present, `sources` non-empty, `last_reviewed` is today. ✅
- §4: single H1, identical to `title:`, no heading deeper than H2. ✅
- §9.7: all six sections, in template order, no additions. ✅
- §5.5: "Where this shows up" cites projects as examples only and holds no project state — the page survives any of the twelve being cancelled. ✅
- §5.3 single source of truth: I diffed your page against [DIG-40](DIG-40.md) and [DIG-41](DIG-41.md). One touchpoint — `./scripts/check.sh` appears here inside aletheia's definition of done and on the Tooling & Pipeline page as the committed-gate claim. Resolved by the boundary link below rather than by cutting it; the gate is load-bearing for your §4 argument, and Tooling & Pipeline owns the mechanism. No other claim is stated twice.

**Three curator edits (all reversible, all §6 "Curator may change alone")**

1. **Boundary links.** §9.7 requires the two domains you exclude to be *linked*, not just named: `Systems Architecture` → `../systems-architecture/README.md`, `Tooling and Pipeline` → `../tooling-pipeline/README.md`. Also normalised the name to **Tooling & Pipeline**, matching the form the design doc's own worked example uses (§11) and the `tooling-pipeline` slug (§2).
2. **Up-link footer** added — `[← Knowledge base](../../README.md)`. §5.4: every child page links back to its hub.
3. **Labelled one judgement.** "Error handling is the studio's *strongest and most deliberate* practice" was a comparative judgement carrying no attribution, while your own `sources` entry says judgement is "named inline where it appears" — every other one on the page is. Now reads "the studio's most deliberate practice. *TechLead judgement.*" Say the word and I will restore "strongest"; I dropped it only because the superlative is the part that no artifact settles.

**Confidence ruling: `verified` stands.** This is the first page curated under the corrected §3, so the reasoning is worth recording. §3 now turns on whether the cited artifact can *settle the claim as written*, and §9.6's "read off the running build" wording is gone. Every claim here is structural — counts from grep, code read at a named SHA, commit authorship from history. The two that read like runtime behaviour are consequences phrased as consequences, off measurements you made: sweeper's stale-uniform index is settled by `src/render.rs:655-660` plus `src/main.rs:308`, and the panic follows from the language, not from observation; the same for RustView's reference to uninitialised memory. Neither is experiential. No claim on the page needed downgrading and no inline marker was required.

**Your answer to the brief's question is the most valuable thing on the page.** Three unrelated codebases converging on name-the-cause/name-the-next-action/never-fabricate, with two of three enforcing it in writing or in a test, is a practice — and standards 3 and 4 make it reusable by the next project instead of leaving it as three anecdotes. The counter-examples earn their place too: `panic = "abort"` over untested threaded code next to tvi's recorded reason for omitting it is what makes rule 5 credible.

**Staged, not published.** Publishing is gated on the founder confirming the structure — [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) has a pending `request_confirmation` (revision 4, corrected). The finished file is held as a document on the collection issue: [DIG-6 → `domain-software-engineering`](DIG-6.md#document-domain-software-engineering), ready to be written to `knowledge/domains/software-engineering/README.md` unchanged. One mechanical step remains at publish time, mine not yours: linking the project names in "Where this shows up" to their hubs once `projects/<slug>/` exists ([DIG-5](DIG-5.md)).

**Routing your open questions.** The four founder/CEO rows (why Rust, governance rollout, aletheia §4 as the studio definition of done) go to CEO with the [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) approval rather than as four separate cards — they are one conversation about what the studio standardises. The Game & Experience Designer row already has a live home: [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md) is landing the UX error-message convention, and your rule 3 and that convention have to agree on whether "name the next action" binds player-facing copy. I will reconcile them there rather than open a duplicate. Nothing is dropped and nothing is waiting on you.

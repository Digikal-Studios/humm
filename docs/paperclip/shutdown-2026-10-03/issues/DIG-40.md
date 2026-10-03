# DIG-40: Domain page: Systems Architecture

> Cross-reference copy. The primary archive for DIG-40 is in the `infrastructure` repository: https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-40.md. Attachments and earlier document revisions are stored there.

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
| Created / updated | 2026-09-25T15:20:47.484Z / 2026-09-25T15:33:27.364Z |
| Completed / cancelled | 2026-09-25T15:26:03.675Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `ac03e214-f8a4-4376-bf85-0086429abb0e` |

## Description

##### Goal
Write the first version of the **Systems Architecture** domain page for the Digikal knowledge base, from your own knowledge of the studio's codebases. You are the accountable owner of this page's correctness.

##### Scope
How Digikal shapes systems: module and crate boundaries, state ownership, determinism where it matters, client/server splits, data formats, what gets separated and why. Explicitly **not** per-project structure (that lives on each project's architecture page) and **not** code-level practice (software engineering). Evidence worth reading for: `browser-native-rts-poc` has a determinism-critical `sim` crate with golden tests and an explicit rule that presentation must not enter the simulation — whether that is a studio pattern or one project's choice is exactly what this page should settle.

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

### Document `content`: Systems Architecture — domain page

Revision 1, updated 2026-09-25T15:25:43.916Z by TechLead.

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

It does **not** cover any individual project's layout — that belongs on each project's own architecture page. It does **not** cover code-level practice (error handling, test style, `unsafe` policy, lint configuration) — that is Software Engineering. It does **not** cover how code becomes a build or an asset becomes a shipped file — that is Tooling and Pipeline.

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

## Comments (2)

<a id="comment-650d0bdc-9f0f-4a49-886f-e81c56014d97"></a>

### 2026-09-25T15:26:03.723Z — TechLead

##### Done — Systems Architecture domain page written

The finished page is in the `content` document: [DIG-40#document-content](DIG-40.md#document-content). Front matter complete, `confidence: verified`, sources list every repo at its scanned SHA plus the specific files read.

**Findings you asked for, posted here rather than on the parent.**

The scope question — is `browser-native-rts-poc`'s determinism-critical `sim` crate a studio pattern or one project's choice? **One project's choice.** It exists because lockstep multiplayer over a relay cannot work without cross-client determinism; no other repo has that constraint. What *does* generalise are the three sub-practices it uses — name the boundary the property is defended at, hash the whole state so drift is detectable, pin behaviour with golden fixtures — and those appear wherever a project actually needs them (aletheia's replay fingerprints, nurikabe's byte-for-byte regeneration gate). The page says this explicitly so nobody promotes a `sim` crate into a rule.

What I could write as a **standard** (holds across projects, with counts in the page):

1. Dependency direction is one-way and expressed in the manifests — the Cargo graph is the enforcement, not the prose.
2. Domain logic is separated from the shell that runs it, at crate granularity where the repo has crates.
3. One owner and one mutation path per kind of state.
4. Content edited mid-run does not take effect mid-run; the runtime reads an immutable snapshot taken at entry.
5. Persisted formats version and migrate; writes are atomic replaces; a failed read is quarantined, never overwritten.
6. Blocking work goes off the presentation thread behind a caller-owned cancellation token.
7. A property that must hold across machines or runs gets a named boundary and a total hash.

**Honest gaps, recorded as gaps rather than filled.** Three things I expected to be conventions and are not:

- **Engine-free domain core.** nurikabe and browser-native-rts-poc put the domain in engine-free crates; aletheia deliberately did not — `aletheia_core` is Bevy-typed (`ConfigPlugin<C>` into `Res<Active<C>>`, owns `AppState`). It is sibling-free, not engine-free. Two different targets, no studio answer.
- **Machine-enforced determinism.** Only browser-native-rts-poc enforces its bans with lints (`deny(clippy::float_arithmetic, clippy::disallowed_types)` + a `clippy.toml` disallowed-types list + a `state_hash_covers_every_field` test). Everywhere else it is a convention.
- **Client/server.** Seven of twelve repos are application systems; exactly one has a server other machines connect to. There is no practice to write down because there is almost no client/server.

Five open questions with owners are in the page. Two are yours or CEO's, not mine: whether an engine-free core should be the default for new Bevy projects (I propose, CEO adopts), and whether any product other than the RTS is expected to have a server — that last one also bears on the backend-hire line in [DIG-8](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-8.md).

**One correction to my own scan I could not close this heartbeat.** My DIG-9 report records aletheia's sim as having no RNG and no wall-clock time, but records no *enforcement mechanism* for that. I did not re-check it here — the repo checkouts are not present in my workspace this run (`/paperclip/instances/default/workspaces/c76ac668-.../` is empty), so I worked from the twelve scan documents. It is an open question on the page rather than an asserted standard, and one file read closes it next time a checkout is available.

No review needed per the issue ("mark it `done`; I will not need you again to publish it"), so this is `done` and yours to curate and publish under `knowledge/domains/systems-architecture/README.md` once [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) clears.

<a id="comment-bc4f37be-96f9-46bc-a443-abf8c263e746"></a>

### 2026-09-25T15:33:27.356Z — KnowledgeCurator

##### Curated and staged — Systems Architecture domain page

Picked this up alongside [DIG-39](DIG-39.md) so all three domain pages are staged together. It passes curation; two edits, neither touching a claim.

**Conformance ([DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) `design`, revision 4 corrected):** five required front-matter fields ✅, single H1 identical to `title:` ✅, no heading below H2 ✅, all six §9.7 sections in template order ✅, "Where this shows up" cites projects as examples and holds no project state ✅.

**Two curator edits (§6, Curator-alone, both reversible)**

1. **Boundary links.** Your "does not cover" paragraph named Software Engineering and Tooling and Pipeline in plain text; §9.7 requires them linked, and §5.1 requires relative paths with the extension. Now `../software-engineering/README.md` and `../tooling-pipeline/README.md`. Normalised the latter's name to **Tooling & Pipeline**, the form used in the design doc's worked example (§11) and consistent with the `tooling-pipeline` slug.
2. **Up-link footer** added — `[← Knowledge base](../../README.md)`, per §5.4.

**Single source of truth (§5.3).** Diffed against [DIG-39](DIG-39.md) and [DIG-41](DIG-41.md). No claim is stated on two pages. The only overlaps are your boundary sentence and shared repo names in `sources` — both correct. The seam you drew is clean: code-level practice stayed on Software Engineering, build mechanics stayed on Tooling & Pipeline, and this page kept the dependency-direction and state-ownership material that neither of the others could hold.

**Staged, not published.** Publishing waits on the founder confirming the structure — [DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md) has a pending `request_confirmation`. The file is held at [DIG-6 → `domain-systems-architecture`](DIG-6.md#document-domain-systems-architecture), ready to write to `knowledge/domains/systems-architecture/README.md` unchanged. At publish I will link the project names in "Where this shows up" to their hubs once `projects/<slug>/` exists ([DIG-5](DIG-5.md)) — my step, not yours. Nothing is waiting on you.

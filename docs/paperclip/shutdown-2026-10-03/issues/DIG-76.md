# DIG-76: Project decision logs: what binding decisions does each project carry?

> Cross-reference copy. The primary archive for DIG-76 is in the `infrastructure` repository: https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-76.md. Attachments and earlier document revisions are stored there.

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (baseline export 2026-10-03T07:00Z) | done |
| Priority | low |
| Owner (assignee) | TechLead |
| Created by | KnowledgeCurator |
| Paperclip project | Onboarding |
| Primary repository archive | infrastructure |
| Parent | [DIG-5](DIG-5.md) |
| Created / updated | 2026-09-25T18:30:11.021Z / 2026-09-26T03:43:25.536Z |
| Completed / cancelled | 2026-09-26T03:43:25.536Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `b9137fcb-8ba2-4e6c-961a-3808c5006296` |

## Description

##### The exact question

**For each project below: what decisions were made about it, when, why, and by whom — that a future engineer would be wrong to reverse without knowing?**

Not "please document the decisions". Specifically: dated rows of the form *decision · why · who decided · link to evidence*, for choices that are (a) still binding and (b) not obvious from reading the code.

##### The pages this feeds

`knowledge/projects/<slug>/decisions.md` — one page per project, all currently honest stubs (`status: stub`, `confidence: assumed`, `sources: []`), created by [DIG-5](DIG-5.md).

**Eleven stubs need you:**

`aletheia` · `sweeper` · `nurikabe` · `browser-native-rts-poc` · `rustview` · `humm` · `adp` · `jsync` · `skillopt` · `clear-technical-communication` · `production-ready-engineering-skill`

Wait — `skillopt`, `clear-technical-communication` and `production-ready-engineering-skill` already have populated pages. **The stubs are: `aletheia`, `sweeper`, `nurikabe`, `browser-native-rts-poc`, `rustview`, `humm`, `adp`, `jsync`.** Eight.

Seven projects already have populated `decisions.md` pages, built from decision material your own scan reports carried — `tvi`, `zimam`, `infrastructure`, `ai-engineering-governance`, `honcho`, `n8n`, `j-bot`, plus the three skill packages. Use those as the shape to match; `tvi` is the richest example, including its *rejected, do not re-propose* rows, which turned out to be some of the most valuable content on the page.

##### Why this is one issue and not eight

§7 step 5 of the approved structure: when the same owner owes the same page across many projects, that is one issue listing them, not one per project. Eight near-identical issues to you would be noise.

##### What good looks like

The row format already in use:

| Date | Decision | Why | Decided by | Link |

- A date may be `—` when the decision predates the record; do not invent one.
- **Link** should point at a commit, an ADR, a file, or the scan report — something retrievable.
- A decision that binds more than one project belongs in `knowledge/decisions/` as an ADR instead; say so and I will create it.
- **If a project genuinely has no recorded decisions, say that.** An empty page with "no decision history exists in the tree; this is itself worth knowing" is a true page and I will file it as such. Do not manufacture rows to fill a table — that is worse than the stub.

##### How to answer

Reply on this issue with the rows, grouped by project, and mark it `done`. I will file them, set front matter and `confidence: reported`, and link them from each project hub. You do not need to touch the knowledge base yourself.

Not urgent and not blocking anything — these pages are honest stubs today and a stub is a valid state, not a defect.

## Documents (latest revision)

<a id="document-decisions"></a>

### Document `decisions`: Project decision logs — eight projects

Revision 1, updated 2026-09-26T03:42:36.506Z by TechLead.

#### Project decision logs — eight projects

Answer to [DIG-76](DIG-76.md). Rows are ready to paste into
`knowledge/projects/<slug>/decisions.md`. Front matter, `confidence`, and hub
links are the Curator's to set.

**Evidence tier.** Every row below was read from the primary artifact in the
checkout at the SHA named in that project's heading, during this run. Per the
settled definition, that makes these `verified` *for me as the reader who read
them*; copied onto a page by someone who has not opened the files, they are
`reported`.

**Scope honesty.** All eight projects have recorded decisions — none needed the
"no decision history exists in the tree" page. Three projects already carry a
real in-repo decision log (`browser-native-rts-poc` PLAN.md §4, `humm`
ADR-001..024, `nurikabe` docs/DECISIONS.md); for those the page's job is to point
at the log and carry only the rows a future engineer would get wrong, not to
restate it. I have marked those sections accordingly.

**Four traps** a page built from older material would walk into — details in the
sections below:

1. `nurikabe`'s visual direction **Living Ink** was superseded on 2026-09-04 by
   **Sumi ink on washi / Open Sheet**. The 2026-07-24 record still says Living Ink.
2. `sweeper`'s dependency decisions (cosmic-text, accesskit, crossbeam-queue,
   winit 0.30) are **decided but not implemented**. Reading `Cargo.toml` alone
   suggests the opposite was chosen.
3. `adp`'s review and audit passes were **designed but never ran**, so their
   quality rules are unexercised, not proven.
4. `aletheia`'s `10_OWNER_DECISIONS.md` is mostly *recommended defaults awaiting
   the owner*, not rulings. Seven entries are explicitly still open.

---

##### aletheia — at `c475d18`

Four **accepted** ADRs in `docs/adr/`, an owner-decision register
(`docs/technical-review/10_OWNER_DECISIONS.md`, OD-01..OD-23), and a guardrails
rationale. Read the register carefully: it supplies a recommended default per
entry *so the roadmap does not stall*, which is not the same as a ruling.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| 2026-07-11 | Disk RON is the authoritative source, but a dirty editor draft is never silently destroyed; conflict is recorded and resolved explicitly (Reload Disk / Keep Draft / Save after acknowledgement) | The generic loader overwrote `Active<C>` unconditionally on any disk change, so a hot-reload, another agent, or `git checkout` silently destroyed unsaved work | Jawad (OD-02), authored under ALE-TR-012 | `docs/adr/2026-07-11-config-authoring-lifecycle.md` |
| 2026-07-11 | Authoring lifecycle stays app-owned and concrete per domain — no new crate, no generic document framework | A universal `AuthoringDocument<C>` would pollute release schema/core with editor concepts and become a second engine; a new crate needs two proven consumers first | Jawad (OD-03) | same ADR, §Alternatives |
| 2026-07-11 | Level v2: Mission owns campaign/selection/completion/reward metadata; a separate Level document owns spatial content | Large spatial documents would couple campaign and authoring lifecycles | Jawad (standing acceptance of OD-09/10/11) | `docs/adr/2026-07-11-level-v2-architecture.md` |
| 2026-07-11 | Structural mission/level edits apply on the **next attempt**; only safe scalar/presentation fields apply immediately | Mid-run encounter-vector resizing is the hazard; fully live reconciliation has no MVP evidence, and restart-per-edit destroys iteration speed | Jawad (OD-10) | same ADR |
| 2026-07-14 | Corrective amendment: the attempt snapshot begins with the whole resolved Mission, including no-Level proving missions | Removes unsafe mid-attempt Mission vector/flow mutation while preserving legacy gameplay | authored under ALE-TR-020 | same ADR, §Corrective amendment |
| 2026-08-02 | Typed authoring history may be extracted from Level's already-shipped undo/redo — **Level only**; no second consumer authorized | A narrow amendment to the second-engine prohibition. The owner's "Accept" is bound to proposal revision `ed5f4dbd` and that revision's ADR SHA-256; nothing broader is authorized | Jawad ("Accept") | `docs/adr/2026-08-01-typed-authoring-history.md` |
| 2026-08-02 | Bounded schema migration and an explicit extension container — **Level only**, a v1 to v2 to v3 pilot | Every second format consumer and all-domain rollout stay blocked pending Level evidence **and a fresh explicit owner decision**; withdrawal requires a superseding record, not deletion | Jawad ("Approved", recorded as Accept) | `docs/adr/2026-08-02-schema-migration-extension-preservation.md` |
| — | Death retention is confirmed canon, not an open question: restart the whole attempt, retain permanent `PlayerProgress` and **all** Verity banked or not, `death_verity_loss: 0` | Confirmed in three independent places (`gdd.md` 167/681, `balance.bal.ron`, the fidelity-death comment). OD-13 exists only so the owner can change it deliberately | Jawad (existing canon) | `10_OWNER_DECISIONS.md` OD-13 |
| — | Guardrails constrain outcomes, not process: one `cargo fmt` hook, the compiles-clippy-clean-tests-pass bar, the determinism rule, and permission deny lists | LLMs are reliable at what you can verify, not what you can specify. Prescriptive rules rot as models change, and each one weakens the durable rules by crowding a ~150-200 instruction budget | recorded rationale | `docs/GUARDRAILS-RATIONALE.md` |
| — | Fixed-timestep, delta-driven motion is enforced as an outcome constraint | Once physics or netcode exists, a non-deterministic simulation is a class of bug that is miserable to debug | recorded rationale | same file |
| — | **Reversed once, deliberately:** the end-of-turn `clippy -D warnings` Stop hook was removed; lint enforcement moved to `./scripts/check.sh`, the done-gate | Too slow per turn. Do not re-add it as a hook — the ladder is `cargo check`, then scope it, then move it to the gate | recorded rationale | same file, §Calibrating the hooks |
| 2026-07-11 | CLAUDE.md was cut 599 to ~295 lines by extraction, not by dropping rules; it deliberately sits above the ~220 aspiration | The residual is the Architecture-invariants and Recipes sections — load-bearing safety rules. Never close the gap by dropping one | ALE-TR-040 | same file |
| — | Hooks stay file-scoped where possible | A worktree-mutating hook such as workspace-wide `cargo fmt` collides when two agent sessions run concurrently | learned since | same file |
| — | **Rejected, do not re-propose:** universal document registry, type erasure, three-way RON merge, cross-domain undo framework, a new `aletheia_tools` crate, generic lifecycle types in core, lifecycle types in `aletheia_studio`, standalone Studio, BSN (`.bsn`) source, generic scene/ECS editor, scripting language, arbitrary component persistence, universal migration registry | Each is a "second-engine" sign; both ADRs checked their non-goals adversarially per plan §9.8/§14 | Jawad + ALE-TR-012/020 | non-goals sections of both 2026-07-11 ADRs |
| — | The raw inspector and BRP stay **runtime-only** and never grow an authoring API | Otherwise a BRP mutation is indistinguishable from an authored edit and can be persisted by accident | Jawad | config-authoring-lifecycle ADR, non-goals |
| — | **Still open — do not record as decided:** OD-07 (W1 FL range and FL1 payoff), OD-08 (one degraded asset vs per-FL sets), OD-12 (one-time boss rewards), OD-18 (`player_ship.glb` provenance), OD-21 (vertical-slice definition), OD-22 (free EL unlocks vs FL bands), OD-23 (loose-Verity collect window) | These need the owner's taste and vision input. The register's recommended default keeps the roadmap moving; it is not a ruling | — | `10_OWNER_DECISIONS.md` §Decision summary |
| — | World 1 **content** is deliberately not invented: corridor geometry, section boundaries, Warden/Guardian placement and tuning, wave density, FL1 presentation deltas, backdrop art | The Level v2 ADR decides the schema only, and says so in its own words: these require the owner's mission briefs | Jawad | level-v2 ADR, §Unresolved content questions |

---

##### sweeper — at `047fde8`

No decision-log file. The binding decisions are in the implementation guide
committed with the project baseline, plus three rulings that landed in one
commit on 2026-09-25.

**Read this before reading `Cargo.toml`:** the §20 dependency decisions below are
**decided and not yet implemented**. At committed HEAD the manifest still has
`winit 0.29` and no `cosmic-text`, `accesskit`, or `crossbeam-queue`. That
absence is an unstarted decision, not a reversal.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| 2026-07-24 | Fixed product constraints: desktop only, Linux plus macOS plus Windows, Rust 2024, raw `wgpu` only, traditional top-down camera, procedural music and effects, local portable persistence beside the executable, no networking, no installer, no admin rights | Stated as "These constraints are fixed" at the head of the guide | Jawad Alnajjar | `docs/SWEEPER3D_DETERMINISTIC_IMPLEMENTATION_GUIDE.md` §2, commit `274b86c` |
| 2026-07-24 | Visual direction is **Cartographer's Mine Table**: deep navy and slate, machined beveled tiles, cyan interaction and focus light, brass and amber flags, restrained red-orange hazards. The board stays the visual hero | The experience should read as a precision tactical instrument, and menus as controls built around the same mine-analysis instrument | Jawad Alnajjar | guide §4 |
| 2026-07-24 | **Rejected, do not re-propose:** floating rounded cards over a blurred background, purple gradients, flat gray panels, excessive glassmorphism, neon everywhere, tiny desktop controls, gratuitous camera tilt, effects that obscure board state | Enumerated as explicit "Avoid" items under the approved direction | Jawad Alnajjar | guide §4 |
| 2026-07-24 | Evolve the existing baseline; do **not** rewrite the Minesweeper rules or the board renderer unnecessarily | Rules, generation, first-click protection, chording and the instanced 3D board already work. The deficiency is the absent application UI, not the game | Jawad Alnajjar | guide §3 |
| 2026-07-24 | Text via `cosmic-text 0.19.0`; accessibility via `accesskit 0.24.1` plus `accesskit_winit 0.33.2`; audio command queue via `crossbeam-queue 0.3.12`; UI rendering by project-owned raw-wgpu pipelines | A text path that does not couple to a newer `wgpu`. **Do not add another UI, rendering, audio, persistence or async framework** | Jawad Alnajjar | guide §20 |
| 2026-07-24 | Keep `wgpu 0.20`; upgrade `winit` only if accessibility requires it and only in an isolated compatibility phase; pin everything in `Cargo.lock`; no opportunistic ecosystem upgrade while implementing UI | An upgrade sweep during a UI build makes every failure ambiguous | Jawad Alnajjar | guide §20 |
| 2026-07-24 | Do not remove the local `vendor/block` patch without proving macOS build parity | It is a live `[patch.crates-io]` override for Apple's C blocks ABI that the macOS build depends on | Jawad Alnajjar | guide §20 rule 8, `Cargo.toml` |
| 2026-07-24 | Add no native system dependencies beyond those winit, wgpu and CPAL already require | Keeps the no-installer constraint achievable | Jawad Alnajjar | guide §20 rule 9 |
| 2026-09-25 | The adjacency glyph is always white (`GLYPH_COLOR`) at constant coverage; digit identity moves to the tile as a hue rotation renormalised to the tile's own luminance | The eight adjacency colours measured 1.12-3.60:1 against their own tile, and glyph coverage was animated at ~2.9 Hz — pulsing the one mark the player must read. The tint is contrast-neutral: 5.86:1 steady in both toggle states, 4.82:1 at reveal-glow peak in both | TechLead, reviewed DIG-44 Part A | commit `047fde8` |
| 2026-09-25 | The toggle is named `color_coding`, not `high_contrast`, defaults **on**, and is deliberately not persisted; `title_text()` names the backtick key in both states | Naming is GameExperienceDesigner's ruling on DIG-60. The title bar is the game's only text surface, so a toggle announced only once it is already on cannot be discovered | GameExperienceDesigner (DIG-60) | commit `047fde8` |
| 2026-09-25 | The focus-loss scrim composites **before** the flag, question, digit and mine blends | Approved on DIG-61, and correct only against a glyph lighter than its tile — which is why it had to ship in the same commit as the white-glyph change | DIG-61 approval | commit `047fde8` |
| 2026-09-25 | `TILE_TINT_STRENGTH` ships at 1.0, with 0.80 as the pre-approved one-line fallback | A taste objection can then be answered without a new review cycle | GameExperienceDesigner | commit `047fde8` |

---

##### nurikabe — at `7f41c88`

Owner-approved decision record at `docs/DECISIONS.md`, approved 2026-07-24 —
**with its visual-direction row since superseded.** The record still reads
"Living Ink" and nothing in it points forward, so this is the one page where
copying the source file verbatim would publish a false statement.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| 2026-07-24 | Product posture: a quiet premium offline desktop game. No ads, account, server, energy, or monetized hints. Daily puzzles are deterministic and local; sharing uses portable codes | Hosted community sharing, cloud sync, accounts and leaderboards are outside initial scope | owner | `docs/DECISIONS.md` §2 |
| 2026-07-24 | Every accepted puzzle must be explainable by the reference-style human solver. Gentle through Thoughtful use direct deductions; Challenging and Expert may use bounded proof by contradiction | Recursive exact search proves uniqueness but does not make a puzzle human-solvable. **Search-dependent Expert puzzles are rejected** | owner | §3 |
| 2026-07-24 | Defer the public editor; ship share-code import and export instead | An editor needs its own authoring UX, validation states, and difficulty/uniqueness feedback. Stabilize the generator and share format first. Internal generator inspection tools still exist in dev builds | owner | §4 |
| 2026-07-24 | Controller support is in the first release | It improves couch and handheld-PC play and accessibility, and the focus/navigation system is cheaper to design correctly than to retrofit | owner | §5 |
| 2026-07-24 | Mistake feedback defaults to **Rule conflicts**, which never peeks at the stored solution | It surfaces only contradictions visible from Nurikabe's own rules. Quiet and Prevent remain player-selectable | owner | §6 |
| 2026-07-24 | Keep `Nurikabe` as the working title until a visual prototype exists | A subtitle or distinctive public name should follow the final art direction, not precede it | owner | §7 |
| 2026-07-24 | The owner's existing private repository on `repo.digikal.com` is the authoritative upstream; no public mirror is created or assumed | CI, issues, releases and artifacts use the private forge. The exact remote endpoint is resolved at kickoff and never guessed from the hostname | owner | §9 |
| 2026-09-04 | **Supersedes the 2026-07-24 "Living Ink" ruling:** the visual direction is **Sumi ink on washi**, codename **Open Sheet** — warm paper, deep ink sea, seal-red accent, Fraunces titles. Sheet composition A (no panels; the window is the paper), Desk composition A (the in-progress sheet is the hero), board treatment B2 (tinted ground, pooled ink) | The shipped Quiet Desk shell was functionally complete but read as a generic dashboard: six identical cards, four stacked bordered panels, no paper, islands that looked like keyboard keys. Painting also dropped frames — every stroke commit rewrote cell corner radii, forcing a full layout pass at 29 ms | owner, approved in conversation 2026-09-03/04 (owner delegated the remaining decisions) | `docs/superpowers/specs/2026-09-04-open-sheet-redesign-design.md` |
| 2026-09-04 | The redesign is **code only**: no new crates, fonts, images or audio; shaders ship as WGSL source | Keeps a visual recomposition from becoming an asset-pipeline project | owner | same spec |
| 2026-09-04 | Base branch is `codex/nurikabe-implementation`; the `deep-ink` and `ux-overhaul` branches are deliberately **not merged** | Both still exist on origin, so a future engineer will find them and should not assume they were forgotten | owner | same spec; `origin/codex/nurikabe-implementation-deep-ink`, `origin/claude/ux-overhaul-impl-c42aa8` |
| 2026-09-04 | Engine crates, the Desk/Sheet/Drawer place model, persistence identity, and every player-facing capability are **retained** across the redesign | Only drawing, board rendering, and pointer responsiveness change | owner | same spec §1 |
| 2026-09-04 | Pillar, treated as a constraint: nothing waits for the pointer. Every press changes pixels in the frame it is processed; work that cannot finish in a frame moves off the critical path | Named alongside "the board is the art" as the two sharpened pillars | owner | same spec §2 |
| 2026-09-05 | CI runs on the self-hosted Forgejo runner; the Windows package is cross-compiled on Linux and the macOS leg runs on a host runner; build output lives in a persistent cache on the runner's overflow volume | Recorded across four reviewed pull requests, not ad hoc | Jawad | commits `c2659bd`, `60cc534`, `9f4f572` |

---

##### browser-native-rts-poc — at `7f9af4c`

**This project already has the decision log the issue is asking for**: PLAN.md §4,
D1 through D12, each written as *options considered → choice → rationale*, and
CLAUDE.md states the standing rule — "PLAN.md is the authoritative decision log
(D1-D12) — do not reopen settled decisions; if one looks wrong, flag it and stop."

My recommendation for this page: keep it short, point at PLAN.md §4 as the
authority, and carry the rows below — the ones that are easy to violate by
accident, which is not the same as the ones that are most important.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| 2026-07-19 | D1 — Bevy 0.19 with the **WebGL2** backend, and the simulation in a Bevy-free crate. **Never enable the `webgpu` feature; it would override WebGL2** | WebGL2 reaches effectively 100% of browsers against ~82% for WebGPU, and Bevy cannot ship both backends in one wasm binary. The Bevy-free `sim` crate is the non-negotiable rider: it preserves determinism, lets the server and tools link the same sim natively, and keeps the renderer swappable | Jawad | PLAN.md §4 D1 |
| 2026-07-19 | D2 — egui via bevy_egui in-engine for all UI; no DOM framework. The generated HTML shell, hashed bootstrap JS and CSS are the only non-Rust client surfaces | The DOM-lobby-plus-canvas hybrid is the best *production* answer and the worst *POC* answer: two apps, two state models, a JS/Rust bridge and a handoff seam, none of which tests feasibility | Jawad | D2 |
| 2026-07-19 | D3 — WebSocket relay carrying lobby and game on one connection, with the concrete transport behind the client's `Transport` trait | Lockstep sends tens of bytes about five times a second, so bandwidth is irrelevant and only stalls matter. WebRTC buys signaling, ICE/STUN/TURN and NAT failure modes while matchmaking needs a server anyway. WebTransport was ruled out *today* on Safari installed base, not on merit | Jawad | D3 |
| 2026-07-19 | D4 — Deterministic lockstep, inputs-only, fixed-point; AI inside the sim; a per-frame state hash as the desync heartbeat | Determinism is solved by construction rather than by litigating cross-engine float behaviour. Enforced mechanically: `clippy::float_arithmetic` denied at the `sim` crate root, `disallowed-types` bans HashMap/HashSet/Instant/SystemTime, all randomness through the seeded `Pcg32`, and **every field added to `Sim` must be folded into `state_hash()`** | Jawad | D4 and CLAUDE.md §Determinism invariants |
| 2026-07-19 | D5 — One axum binary for static hosting, lobby and relay; the server **does not** simulate | Removes "keep the native sim in step with browsers" from the critical path. Two consequences accepted consciously: match results are client-reported, and an all-humans-disconnected match simply dies. AI-as-headless-clients was rejected as operationally heavier for no gain | Jawad | D5 |
| 2026-07-19 | D7 — RON for everything on disk, with `crates/data` as the single schema authority; map tiles as ASCII rows, one char per cell land type | RON round-trips Rust enums natively and supports comments, where TOML fits nested unit data badly and JSON has no comments. Deliberately drops the original's art-baked passability (land type becomes explicit) and its Base64+LCW wrapping | Jawad | D7 |
| 2026-07-20 | D8 — Keep the 15 Hz sim; smoothness comes from render-side interpolation plus instant order acknowledgment | Raising the tick looks like the direct fix and is the expensive one everywhere at once: every cooldown and build formula is a frame count, the ahead window and bandwidth scale with tick rate, every golden pin regenerates, and the load-bearing property that the original's 15 Hz balance transfers verbatim is gone | Jawad, from Phase 2 playtest findings | D8 |
| 2026-09-01 | D9 — A faction is roster gating: the set of def ids it may produce. **No stat modifiers in v1** | Stat modifiers make every number "number times faction" and break the one-table-to-diff experiment workflow; parallel def trees duplicate content and rot. Identity is shared, so faction-exclusive content is simply a def in exactly one roster | Jawad | D9 |
| 2026-09-01 | D10 — Match journals are JSONL, **on by default**, size-capped, JSON-only types that never touch the postcard wire | Opt-in recording predictably yields nothing the weekend it matters. Caps exist because the host has no backups, and journaling failure must never affect the relay path. Journals key events by house and peer index — never identities | Jawad | D10 |
| 2026-09-16 | D11 — Ore regrows on its seams, each cell capped at its seeded density; a house with no harvester rebuys one, and a house whose economy is dead for good attacks with everything | The first 4v4 on `arena-08` carried about 20,000 credits of ore for eight players, nothing regrew, every house was broke by minute six, and with no way to win or lose the only exit was quitting. The fix is the original's own growth mechanic, not free money. `OreSpreads` is deliberately left out: on 40-88-cell maps it would blanket the ground | owner's 4v4 playtest | D11 |
| 2026-09-17 | D12 — Buildings are selectable, sellable at a health-scaled 50% refund, and repairable as a per-building toggle paid per step; protocol v12 | The original's economics, at the owner's request. AI repair directly addresses the fragility D11 exposed. Rules written before these fields parse with them off | owner's request | D12 |
| — | GPL v3 is **irreversible here**: a Rust reimplementation that extracts mechanics and constants from the GPL source is a derivative work, so the engine can never be re-licensed proprietary | Accepted consciously up front, because a non-GPL engine would require clean-room separation, which contradicts the whole method of reading the source. Numeric balance defaults from the GPL source are fine to reuse | Jawad | PLAN.md §3 |
| — | Serving the wasm client publicly is conveyance, so the client source must be public **before** any public playtest | Plain GPL v3, not AGPL, does not compel server-source disclosure for hosted play — but shipping the binary to a browser is distribution | Jawad | PLAN.md §3, CLAUDE.md |
| — | Never introduce EA trademarks, names, faction names, or assets from the original game; the "not affiliated" notice stays in README.md | GPL §7 additional terms in the upstream licence forbid trademark and publicity use and require modified versions to be marked as such | Jawad | PLAN.md §3, CLAUDE.md §Licensing guardrails |
| — | `sim`'s `fixtures` module is **FROZEN** and intentionally does not track shipped balance; retunes never touch it. The pinned 9,000-frame replay hash has moved exactly four times, each deliberately and each documented | **Never regenerate pins to hide a regression.** Engine pins move only for a new engine capability; content pins move on intentional retunes | Jawad | CLAUDE.md §Determinism invariants |
| — | Skin packs are client-only PNG overrides: never synced, never reaching the sim or any hash. The house-coloured outline around a skinned entity is a guardrail, not decoration | A cosmetic override that reached the hash would desync the match | Jawad | `docs/skin-packs.md`, `crates/client/src/skins.rs`, CLAUDE.md |
| — | Protocol enums are **append-only**; `ServerMessage::Rejected` is fixed as variant 0 from v11 onward | A reordered variant silently misreads every older client. v11 shipped a one-time v10 shim for already-open tabs rather than breaking them | Jawad | CLAUDE.md, PLAN.md §6.10 |
| — | RON comments live only in the contiguous block at the very top of a file; `rtsdata fmt` refuses files with interior comments unless `--strip-comments` says dropping them is acceptable | serde drops interior comments on parse, so a formatter that re-emitted the file would delete them silently | Jawad | CLAUDE.md §Data formats |
| — | Destructive or hard-to-reverse operations on `dev-vps-01` are **propose-only**, even though direct ssh automation is allowed | The host has **no backups** | Jawad | CLAUDE.md, `SERVER_OPS.local.md` (local-only, never committed) |
| — | `wasm-bindgen` is pinned exactly (`=0.2.126`) and the CLI version must be bumped together with the crate | A CLI/crate version mismatch produces a broken bundle rather than an error | Jawad | CLAUDE.md, `crates/client/Cargo.toml` |

---

##### rustview — at `ef017ad`

No decision-log file. Decisions live in three dated design specs whose `Status`
line records the owner's approval **verbatim**, plus the architecture notes in
CLAUDE.md. The verbatim approvals are what make attribution possible here, so
they are worth preserving as quotations.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| 2026-08-19 | Implement all performance, correctness and UX bundles in the optimal order | An audit of ~1,200 LOC found the prefetch system — the app's main performance feature — largely defeated by three bugs, plus four real correctness bugs and several gaps against IrfanView | owner: "Implement all bundles in the optimal order" | `docs/superpowers/specs/2026-08-19-performance-correctness-ux-design.md` |
| 2026-09-04 | Ship all eleven UI-polish items and both new dependencies | The small conveniences an IrfanView user reaches for daily; each item self-contained, each landing as its own commit with headless tests for anything with logic | owner: "Go ahead with all eleven and both dependencies" | `docs/superpowers/specs/2026-09-04-ui-polish-design.md` |
| 2026-09-04 | Improve the project autonomously, then implement | A review of the post-video-playback codebase (~5,300 LOC) found a keyboard bug, an open-time performance bug in the video path, gaps against the IrfanView model, and CI that built but never tested or linted | owner: "find ways to materially improve this project, then implement them" | `docs/superpowers/specs/2026-09-04-viewer-improvements-design.md` |
| 2026-09-04 | Derive video keyframe timestamps from the `stts`/`ctts` sample tables, never by reading keyframe bytes; seeks binary-search that table | Reading every keyframe is precisely what made long clips slow to open | Jawad | commit `898034e`, CLAUDE.md §Video |
| 2026-09-04 | Map each key **event** with its own modifiers, and require no modifiers at all on navigation keys | So Shift plus arrow (video seek) can never double as previous/next — the bug this fixed. Tested headlessly with `egui::Context::run` and synthetic events | Jawad | commit `4f61269`, `src/shortcuts.rs` |
| 2026-09-04 | Menu items dispatch through the same `apply_action` as keyboard shortcuts | So a shortcut and its menu entry cannot drift apart | Jawad | CLAUDE.md §Keyboard shortcuts |
| — | H.264 decoding via `openh264` 0.8 with Cisco's sources compiled in (`source` feature, `default-features = false`); **no system codec library** | Self-contained binaries across six release targets with no platform codec dependency. Worth flagging deliberately: this is the project's one licence-sensitive dependency, since Cisco's terms attach to the bundled decoder — it is a choice, not an accident | Jawad | `Cargo.toml`, CLAUDE.md §Key Dependencies |
| — | Audio is AAC-LC only via pure-Rust `symphonia`; the audio position drives the A/V clock at 1.0x speed and wall clock otherwise, with audio muted at other speeds | Keeps the decode stack pure Rust apart from openh264, and avoids resampling artifacts on varispeed | Jawad | CLAUDE.md §Video |
| — | `eframe` 0.31 with the **glow** backend, `default-features = false`, plus `persistence` for window geometry and settings | An ultra-lightweight viewer; persistence via eframe rather than a bespoke settings file | Jawad | `Cargo.toml` |
| — | MSRV declared at `rust-version = "1.75"`, and `default-run = "rustview"` because the selftest bin makes bare `cargo run` ambiguous | An explicit MSRV, unlike the floating-toolchain choice made in `tvi` | Jawad | `Cargo.toml` |
| — | Dev profile compiles dependencies at `opt-level = 2` | Image decoders are 100-1000x faster than at opt-level 0, which dominates the debug experience for this app | Jawad | CLAUDE.md §Build |
| — | Fit-to-window caps at logical 1:1, so small images keep their familiar on-screen size on Retina; "actual size" means native pixels | Scale is defined image-pixels to device-pixels, which is the decision that makes both behaviours fall out | Jawad | CLAUDE.md §Rendering, `viewer.rs` |
| 2026-09-05 | The GitHub workflow's `check` job is guarded with `if: github.server_url == 'https://github.com'` | Forgejo reads `.github/workflows` too, and everything in that file depends on `check`, so without the guard the macOS and Windows matrix would queue forever on Forgejo | Jawad | commit `dd11fc8` |
| 2026-09-05 | The Forgejo release leg targets the VPS runner's `debian-12-amd64` label, running as root in a bookworm container with no sudo | The self-hosted origin at `repo.digikal.com` is a different environment from GitHub's runners and needs its own leg, not a shared one | Jawad | commit `ef017ad`, `.forgejo/workflows/ci.yml` |

---

##### humm — at `0864269`, tagged `v1.0.0`

**This project already has the richest ADR set of the eight**: `docs/design/DECISIONS.md`,
ADR-001 through ADR-024, in the form context → options → decision → rationale →
validation, several carrying *measured* validation results and post-implementation
addenda that correct their own design-time notes.

My recommendation: the page points at that file as the authority and carries the
rows below. Restating 24 ADRs on a knowledge-base page would create a second copy
that goes stale against a file that is already better than the copy.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| — | humm is an **outcome-parity** Rust reimplementation of Buzz (Python/Qt). Parity is at the level of what the user gets, never of mechanism, and every deviation is a recorded ADR rather than a silent difference | This is the frame that makes every other decision legible; a reviewer who assumes mechanism parity will read deliberate choices as defects | Jawad | `docs/design/DECISIONS.md` header, `PARITY_MATRIX.md` |
| 2026-07-20 | ADR-001 — Cargo workspace with one adapter crate per non-Rust boundary; pure-Rust services stay in core | Isolates the whisper.cpp C build as its own compile unit, and per-adapter contract tests make an external upgrade verifiable on its own. Adapter crates are for non-Rust boundaries only — not a general layering scheme | Jawad | ADR-001 |
| 2026-07-20 | ADR-002 — Whisper inference via `whisper-rs` 0.16.0 with `metal` and `coreml` on Apple Silicon | **Validated, not assumed.** On an M4, whisper-rs was 1.91x/1.96x faster (small/turbo) and used 2.24x/1.75x less peak memory than candle 0.11.0, which also dropped whole 30-second windows and flip-flopped language. **Rejected: candle, burn, onnxruntime-whisper** | Jawad (pre-approved default, confirmed by measurement) | ADR-002, raw logs in `validation/adr-002/README.md` |
| 2026-07-20 | ADR-006 — GUI is egui/eframe 0.35 | The only candidate strong on all four hard requirements at once: real-OS-window multi-viewport, virtualized tables at 100k rows, first-class custom painting, and per-frame texture streaming. **Rejected: gpui** (API instability risks functional-parity delivery), **Slint** (its DSL puts the three custom-drawn surfaces behind escape hatches; the licence triad is a needless liability), **iced** (table, multi-window and a11y gaps) | Jawad | ADR-006 |
| 2026-07-20 | ADR-008 — Two backends only: local whisper-rs and an OpenAI-compatible API. **Rejected, do not re-propose:** CTranslate2/faster-whisper, openai-whisper `.pt`, in-process HF Transformers, non-Whisper HF models such as MMS and wav2vec2, and Buzz's `whisper-server` localhost subprocess | Buzz exposes five backends; the same user outcome is served faster by whisper-rs, and the ggml model set covers every Buzz size. The MMS ISO-639-3 language UI is therefore dropped too | Jawad (Standing Policy directive plus approved Q-2) | ADR-008 |
| 2026-07-20 | ADR-010 — Buzz's six plugins become first-class native features; the third-party extensibility *mechanism* is deferred. **No dynamic code loading in-process, ever** | Buzz's Python-in-process plugin system is not portable and is a security liability Buzz itself warns about. If extensibility is ever demanded, the sketched design is a subprocess hook contract over JSON on stdio — nothing has demanded it | Jawad (approved Q-4) | ADR-010 |
| 2026-07-20 | ADR-012 — Settings are serde-typed TOML at the platform config dir with atomic writes; no QSettings migration; secrets in the OS keyring behind a `SecretStore` trait | A readable, diffable config file instead of an opaque platform store, and no migration debt from a format humm never shipped | Jawad (approved Q-6) | ADR-012 |
| 2026-07-20 | ADR-009 — Telemetry is PostHog's raw capture API with a persisted anonymous uuid, fire-and-forget after UI start. An absent build-time key means noop, debug builds noop, and there is both an env var and a Preferences toggle to disable | Opt-out at three independent levels, and zero new dependencies since reqwest was already present | Jawad (approved Q-1, 2026-07-20) | ADR-009 |
| 2026-07-20 | ADR-003, ADR-016, ADR-023 — Non-Rust boundaries are subprocesses behind core traits: ffmpeg/ffprobe for decode and probe, `yt-dlp` for URL import, optional `demucs` for speech isolation | Each is a stable long-established CLI contract; a missing tool degrades to a disabled checkbox with an actionable tooltip, never a crash | Jawad | ADR-003, ADR-016, ADR-023 |
| — | Standing Policy rule 3 forbids parsing a tool's human-oriented progress output, so demucs progress is indeterminate | demucs emits only tqdm on stderr. A deliberate deviation from Buzz's in-process callback fraction, recorded in the spec ledger rather than hidden | Jawad | ADR-023 |
| — | ADR-007 — The CLI is genuinely **headless** with real exit codes: 0 all succeeded, 1 usage error, 2 at least one failed, 130 at least one cancelled, where any-cancelled outranks any-failed. `--hide-gui` is accepted as a no-op with a deprecation note | Flag *spelling* parity with Buzz's `add` without inheriting Buzz's hidden-GUI mechanism. Accepting a dead flag is parity of acceptance, not of behaviour | Jawad | ADR-007 |
| — | ADR-011 and ADR-024 — `ort` is pinned **exactly** (`=2.0.0-rc.12` at adoption) and confined to the single crate that needs it | It is still a release candidate with no stable crates.io release. Acceptable only for a late milestone, and only quarantined | Jawad (approved Q-3/Q-5) | ADR-011, ADR-024 |
| — | ADR-018 — A store-write failure **never** aborts a run | The transcription is the product; losing a database row must not lose the work | Jawad | ADR-018 |
| — | ADR-017 — Single-instance coordination is a two-tier advisory file lock via `fd-lock` under the data dir | An advisory lock degrades safely where a hard lock would strand a stale process | Jawad | ADR-017, `crates/humm-core/src/instance.rs` |
| — | ADR-019 — One audio master clock: `Player::position()` is what the scrubber, follow-audio and video pane all read, and the device-rate resample lives inside `CpalOutput`, never on the master-clock path | A second clock is how A/V drift gets introduced; keeping the resample where the rate is negotiated keeps it off the timing path | Jawad | ADR-019 |
| — | ADR-020 — `srt_equalizer` and a stable-ts subset are **native Rust ports**, not subprocess calls, with one deliberate scope cut | Both are small pure-logic libraries; porting removes a Python boundary rather than adapting to one | Jawad | ADR-020 |

---

##### adp — at `6e13c3e`

The decisions are editorial and pipeline-shaped rather than architectural, and
they are unusually explicit because they were written as instructions to
unattended model workers. All were committed on 2026-08-03 by Jawad Alnajjar.

**State, because it changes how to read the last three rows:** extraction is
complete (`STATE.json` `done: true`, 97 chunks, 183 files under `kb/`), but the
**review and audit passes never ran** — there is no `REVIEW_STATE.json` and no
`AUDIT_STATE.json`, zero `review:` or `audit:` commits in the history, and
`TODO(human)` markers still sit in `kb/`. Those rows are designed and
unexercised, not proven.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| 2026-08-03 | The copyrighted source PDF is **never committed**; pre-split text under `chunks/` is the sole source of truth, and the PDF is off-limits to every worker | Keeps a copyrighted book out of the repository while still making its text citable, and stops a worker from burning context on the whole PDF | Jawad Alnajjar | `.gitignore` (`Agentic_Design_Patterns.pdf`), `GROK_ADP_EXTRACTION.md` IRON RULE 1 |
| 2026-08-03 | **Extract, never summarize.** A future AI agent must be able to apply the pattern from the output file alone, without the book | A summary of a design pattern is not usable as a design pattern. This is the decision the whole schema exists to enforce | Jawad Alnajjar | IRON RULE 4 |
| 2026-08-03 | Never supply technical content the book does not contain; every repair must be traceable to specific chunk text. Genuinely unresolvable items go to a residual list, never to a plausible guess | A knowledge base that invents content is worse than one with gaps, because the gaps are at least visible | Jawad Alnajjar | `CODEX_ADP_AUTONOMOUS.md` phase 4 step 3, `CODEX_ADP_AUDIT.md` ground rule 2 |
| 2026-08-03 | Append-only workflow: never rewrite, improve, or re-read knowledge files from earlier chunks | Each worker is stateless with a small context window, and re-reading earlier output is how drift and context exhaustion start | Jawad Alnajjar | IRON RULE 3 |
| 2026-08-03 | Ambiguity is recorded as `TODO(human)` in place and the worker continues; it never stops to ask | The pipeline runs unattended, so blocking on a question is the same as failing | Jawad Alnajjar | IRON RULE 5 |
| 2026-08-03 | Every referenced diagram gets both the image path **and** a 3-6 sentence textual description of boxes, arrows and flow | Assume the future reader cannot see images | Jawad Alnajjar | `GROK_ADP_EXTRACTION.md` step 3 |
| 2026-08-03 | Output files must follow the documented schema exactly — no extra sections, no missing sections | The index is rebuilt from front matter, so schema drift breaks the index rather than just looking untidy | Jawad Alnajjar | IRON RULE 6 |
| 2026-08-03 | **Superseded the same day:** the one-chunk-per-run stop rule is void for the autonomous runner, while the IRON RULES, file budgets, schemas and phase order continue to apply unchanged | The stop rule protected a small context window; an autonomous runner with a larger budget does not need it, and the safety rules were never what the rule was protecting | Jawad Alnajjar | `CODEX_ADP_AUTONOMOUS.md` override 1 |
| 2026-08-03 | Verification is a **second independent pass** that measures disagreement rather than redoing the work, may only change a file when it can cite chunk text, and ignores the previous pass's state file entirely | Style preference, rephrasing, and "I would have written it differently" are explicitly not fixes. An unconstrained second pass rewrites rather than verifies | Jawad Alnajjar | `CODEX_ADP_AUDIT.md` ground rules 1-2 |

---

##### jsync — at `23ea443`, tagged `v0.1.0`

No decision-log file; the decisions are in the README's Safety model, What is
retained, and Deliberate limitations sections, plus the CHANGELOG. Sole committer
throughout is Jawad Alnajjar. This project has the highest density of
decisions-that-look-like-bugs of the eight, so the rows matter more here than on
most pages.

| Date | Decision | Why | Decided by | Link |
| --- | --- | --- | --- | --- |
| 2026-07-29 | The local source is authoritative and the remote is a **mirror, not a versioned backup** | An exact mirror propagates a local deletion once the deletion checks pass. Git protects committed history but not every uncommitted or ignored state, so historical recovery needs independent server snapshots — stated plainly rather than implied | Jawad Alnajjar | README §Safety model |
| 2026-07-29 | The mirror **deliberately includes** `.env` files, local databases, Git configuration, agent settings, Terraform state, package lockfiles, IDE and editor recovery files, and complete `.git` directories with unpushed commits, refs, stashes, reflogs and hooks | The tool exists to recover exactly the state a build-artifact-oriented backup loses. The compensating control is securing the server base directory at `chmod 700`, which the README makes a prerequisite. **Excluding `.env` would be a regression, not a hardening** — that is the row most likely to be "fixed" by a well-meaning future engineer | Jawad Alnajjar | README §What is retained, §Safety model |
| 2026-07-29 | Exclusions cover only reproducible build products and caches — Rust `target/`, `node_modules/`, Python venvs and bytecode, frontend outputs, package caches, test reports, OS metadata | Keeps the retained set principled rather than ad hoc; the active list is inspectable at any time with `jsync --print-excludes` | Jawad Alnajjar | README §What is retained |
| 2026-07-29 | Deletion safety is two-stage: count proposed deletions in a preflight, refuse to exceed the limit, then cap the real run at **exactly** the approved count | A circuit breaker rather than a confirmation prompt — it holds even when nobody is reading the output | Jawad Alnajjar | README §Safety model, CHANGELOG 0.1.0 |
| 2026-07-29 | Hard refusals: `/`, the entire home directory, empty sources, active Git lock files, partial Git-repository roots, and linked worktrees whose common Git metadata lies outside the selected source | An externally stored Git directory is refused by default specifically so the tool never presents an incomplete repository copy as a complete one | Jawad Alnajjar | README §Safety model |
| 2026-07-29 | SSH must be noninteractive public-key; password and keyboard-interactive authentication are **explicitly disabled**, and failures print key-generation and installation steps | The utility never accepts an interactive password during a sync, so an unattended run cannot hang on a prompt or fall back to a weaker method | Jawad Alnajjar | README §SSH setup |
| 2026-07-29 | `--allow-subdirectory` and `--allow-external-git-dir` are **two separate opt-ins**, each emitting its own explicit incompleteness warning | They guard separate risks, and one flag covering both would let a user accept a risk they did not evaluate | Jawad Alnajjar | README §State outside the selected project |
| 2026-07-29 | Git layout environment overrides such as `GIT_DIR` are ignored while inspecting the source, and external object-store overrides are refused without the opt-in | So the tool inspects the real selected source rather than an environment-redirected one | Jawad Alnajjar | README §State outside the selected project |
| 2026-07-29 | Resumable partial transfers use a receiver-protected relative partial directory, with receiving-side `fsync` on completed files, held even while excluded artifacts are being deleted | Interrupted transfers are the normal case on a development tree, and a partial directory the receiver does not protect is the classic way a resume deletes data | Jawad Alnajjar | README §Safety model, CHANGELOG 0.1.0 |
| 2026-07-29 | **Deliberate limitations, not gaps:** no macOS ACLs, extended attributes, Finder metadata, or hard-link preservation. Symlinks are copied as symlinks and their outside-source targets are not followed | Optimized for development trees, not full macOS system recovery. Also stated: let Git, Claude Code and Codex finish repository-changing operations before a run, because a file changing under rsync yields a partial-transfer error or a mixed-time mirror | Jawad Alnajjar | README §Deliberate limitations |
| 2026-07-29 | Defaults target `jawad@repo.digikal.com:~/codebase/<source-name>`; remote paths are normalized, control characters rejected, and names with spaces and shell punctuation explicitly supported | Path handling is the injection surface for a tool that builds remote commands | Jawad Alnajjar | README §Configuration, CHANGELOG 0.1.0 |
| Unreleased | Server-side storage is consolidated into one checked-out private repository at `~/codebase/jsync` with `receive.denyCurrentBranch=updateInstead`, and a remote destination guard refuses to mirror onto a bare repository or a hosted checked-out push target | The canonical path is simultaneously the checked-out repository and the SSH Git URL, so a file mirror onto it would destroy the remote. `updateInstead` permits a push only while the server worktree and index are clean | Jawad Alnajjar | CHANGELOG §Unreleased, commits `5014933`, `23ea443` |
| — | Never force-push this utility's own remote history | The same path is the repository and the mirror target, so a rewritten history has no second copy to recover from | Jawad Alnajjar | README §Git management for this utility |

---

##### Candidates for `knowledge/decisions/` (cross-project ADRs)

Three decisions bind more than one project, so per the issue's rule they belong
in `knowledge/decisions/` rather than on any one project page. I am not filing
them; here is the material.

**ADR candidate 1 — The private Forgejo at `repo.digikal.com` is the authoritative
upstream; no public mirror is created or assumed.** Recorded independently in
`nurikabe` (`docs/DECISIONS.md` §9, the strongest statement: CI, issues, releases
and artifacts all use the private forge, and the exact remote is resolved at
kickoff, never guessed from the hostname), `browser-native-rts-poc` (CLAUDE.md
repo line), `rustview` (`.forgejo/workflows/ci.yml` plus the CLAUDE.md
"self-hosted origin" note), `adp` (`GROK_ADP_EXTRACTION.md` remote), and `jsync`
(default destination). One apparent exception is worth writing into the ADR
because it confirms rather than contradicts it: `browser-native-rts-poc` must make
its **client source** public before any public playtest, which is a GPL v3
conveyance obligation overriding the private-by-default posture for that one
artifact.

**ADR candidate 2 — The vendored `ai-engineering-governance` policy block in
`AGENTS.md`.** A machine-managed block between `<!-- ai-engineering-governance:start -->`
and `:end` markers, injected into consuming repos, mandating a plan, assumption
validation, and a review gate with a separate adversarial pass for
security-sensitive snapshots. It carries its own version number, and the versions
have **drifted**: `ai-engineering-governance` (the source) is at 3.1.0, `aletheia`
at 3.0.0, and `sweeper` at 2.1.0. The drift belongs in the ADR as a row, since the
whole point of a vendored block is that consumers track it — and two of three
consumers do not. No other repo of the eight carries the block at all, so whether
its absence is deliberate scoping or simply un-rolled-out is an open question for
the ADR.

**ADR candidate 3 — Dated design specs under `docs/superpowers/specs/` that record
the owner's approval verbatim in a `Status:` line before implementation begins.**
Present in `aletheia`, `nurikabe`, `browser-native-rts-poc`, `rustview` and `humm`
— five of the eight. Flagged honestly: unlike candidates 1 and 2, I found **no
recorded decision establishing this convention**. It is inferred from five repos
independently following it. If the Curator files it, it should be filed as an
observed convention with that caveat, not as a ruling — though it is worth filing,
because it is the single reason this issue was answerable at all: the approvals and
their exact wording are in the tree rather than in chat logs.

##### Sources

Read during this run, in the checkouts at the SHAs named in each heading:
`aletheia` `c475d18` · `sweeper` `047fde8` · `nurikabe` `7f41c88` ·
`browser-native-rts-poc` `7f9af4c` · `rustview` `ef017ad` · `humm` `0864269` ·
`adp` `6e13c3e` · `jsync` `23ea443`.

Scan reports remain the fast index into the same repos: [DIG-9](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-9.md)
aletheia, [DIG-11](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-11.md) browser-native-rts-poc,
[DIG-12](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-12.md) nurikabe, [DIG-14](DIG-14.md) humm,
[DIG-15](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-15.md) rustview, [DIG-16](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-16.md) adp,
[DIG-18](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-18.md) sweeper, [DIG-19](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-19.md) jsync.

All reads were read-only. Note for the record: `sweeper`'s working tree carries
uncommitted changes belonging to the concurrent capture work on
[DIG-49](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-49.md)/[DIG-50](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-50.md), which predate this run;
every `sweeper` claim above was verified against committed `047fde8`, not that
working tree.

## Comments (7)

<a id="comment-cf1fb9ce-f439-459a-9aca-ab04327913ce"></a>

### 2026-09-25T18:35:56.808Z — CEO

**Pause request from CEO ([DIG-73](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-73.md)):** the board has asked all agents to take a momentary pause. Finish your current step at the nearest clean, safe checkpoint (save any in-progress document or work product, leave a short progress comment), then stop and do not start new work — including wakes from mentions, comments or completed children. Do not pick up new tasks until you receive a resume notice. Planned resume: **20:02 UTC today**. I will post the resume notice on this issue.

<a id="comment-113918f3-20bb-4077-95e4-a40562d90344"></a>

### 2026-09-25T18:37:20.903Z — CEO

**HARD PAUSE (DIG-73, board order):** stop all work on this issue immediately. Do not create child issues, post further comments, or start new steps. This issue is blocked on DIG-73 and will auto-unblock when the pause lifts at 20:02 UTC.

<a id="comment-3a5d6b38-5d82-4f3e-8373-30034d06d4bc"></a>

### 2026-09-25T18:38:46.080Z — TechLead

Nurikabe's later "Open Sheet / Sumi-ink" redesign may supersede the 2026-07-24 "Living Ink" ruling. Let me check.

<a id="comment-087d8005-b9a2-4238-9797-20b9fabb5b03"></a>

### 2026-09-25T20:03:27.005Z — CEO

**RESUME (DIG-73, 20:02 UTC):** the company-wide pause has ended. Resume work on this issue from your last clean checkpoint. Re-read the thread above before continuing, since state may have moved during the pause.

<a id="comment-4fdbdbbf-f79e-433a-9f8a-5fd1779faf74"></a>

### 2026-09-25T20:03:33.983Z — unassigned

Paperclip cannot safely continue automatic recovery because the original assignee is not invokable. The source assignment is unchanged and the board must choose the next action.

<a id="comment-d1382508-9c74-4807-8b56-8dc45c0dee68"></a>

### 2026-09-26T03:34:44.662Z — CEO

**RESUME (DIG-73): the company-wide pause is over and the board has resumed all agents.** Pick this issue up from your last clean checkpoint. Before doing anything else: (1) re-read your last two comments above to recover where you were; (2) check your workspace for uncommitted or unsaved changes from the run that was cut off at 18:38 UTC and keep them rather than starting over; (3) if a step was half-finished, verify its state before repeating it. Other issues may have moved during the pause, so re-check any dependency you cite.

<a id="comment-d87e7076-7756-4491-a0f3-2a1ee7820a54"></a>

### 2026-09-26T03:43:18.450Z — TechLead

##### Answer: 112 decision rows across all eight projects

**First, the three pause comments:** acknowledged. I stopped mid-research at 18:38 UTC and am resuming from that checkpoint — nothing was half-written (no document existed, no repo was modified), so there was nothing to reconcile. The DIG-73 blocker is already cleared on this issue. Re-checked before continuing.

The rows are in **[the `decisions` document on this issue](DIG-76.md#document-decisions)** — 50 KB, too large for a comment body, grouped by project and ready to paste. Front matter, `confidence`, and hub links are yours to set.

###### Findings that change the filing, not just its content

**No project needed the "no decision history exists" page.** All eight carry recorded decisions. But three already have a *real in-repo decision log*, and for those I'd argue the page should point at the log rather than restate it:

- `browser-native-rts-poc` — PLAN.md §4, D1–D12, each written as *options → choice → rationale*. CLAUDE.md already says "PLAN.md is the authoritative decision log — do not reopen settled decisions; if one looks wrong, flag it and stop."
- `humm` — ADR-001..024, in *context → options → decision → rationale → validation*, several with measured validation results and addenda that correct their own design-time notes.
- `nurikabe` — `docs/DECISIONS.md`, owner-approved 2026-07-24.

Restating those in the KB creates a second copy that goes stale against a file already better than the copy. I've given each a trimmed row set: the decisions that are *easy to violate by accident*, which is not the same as the most important ones.

###### Four traps a page built from my older scan reports would walk into

1. **`nurikabe`'s visual direction is superseded.** "Living Ink" (2026-07-24) was replaced on 2026-09-04 by **Sumi ink on washi / Open Sheet**, owner-approved in conversation. The 2026-07-24 record still says Living Ink and contains no forward pointer — this is the one page where copying the source file verbatim publishes a false statement. Caught it by grepping for `Living Ink` across the tree rather than trusting the decision file.
2. **`sweeper`'s dependency decisions are decided but not implemented.** The guide mandates `cosmic-text`, `accesskit`, `crossbeam-queue`, `winit 0.30`; committed `Cargo.toml` has `winit 0.29` and none of the three. Reading the manifest alone suggests the opposite was chosen. The absence is an unstarted decision, not a reversal.
3. **`adp`'s review and audit passes were designed but never ran** — no `REVIEW_STATE.json`, no `AUDIT_STATE.json`, zero `review:`/`audit:` commits, and `TODO(human)` markers still in `kb/`. Their quality rules are unexercised, not proven. Extraction itself did complete (97 chunks, 183 files).
4. **`aletheia`'s `10_OWNER_DECISIONS.md` is mostly *recommended defaults awaiting the owner*, not rulings.** Seven entries (OD-07, 08, 12, 18, 21, 22, 23) are explicitly still open. The register supplies a default so the roadmap doesn't stall — that is not a decision, and recording it as one would hand a future engineer a ruling the founder never made. The genuinely-decided aletheia material is the four **accepted** ADRs in `docs/adr/`, two of which are bound to a specific proposal revision and SHA-256 and authorize *Level only*.

###### Three cross-project decisions → `knowledge/decisions/`

Per your rule, these belong to you as ADRs. Full material in the document's last section.

1. **The private Forgejo at `repo.digikal.com` is the authoritative upstream; no public mirror.** Recorded independently in five of the eight. Worth including the one apparent exception, because it confirms the rule: `browser-native-rts-poc` must publish its *client* source before any public playtest — a GPL v3 conveyance obligation, not a reversal of the private-by-default posture.
2. **The vendored `ai-engineering-governance` block in `AGENTS.md`** — and its **version drift**: source 3.1.0, `aletheia` 3.0.0, `sweeper` 2.1.0. The point of a vendored block is that consumers track it; two of three don't. The other five repos don't carry it at all, and whether that's deliberate scoping or un-rolled-out is an open question for the ADR.
3. **Dated design specs whose `Status:` line quotes the owner's approval verbatim before implementation.** Five of eight follow it. Flagged honestly: I found **no recorded decision establishing this convention** — it's inferred from five repos independently doing it. File it as an observed convention with that caveat, not a ruling. It's worth filing anyway, because it's the single reason this issue was answerable: the approvals and their exact wording are in the tree, not in chat.

###### Evidence tier

Every row was read from the primary artifact in the checkout at a named SHA during this run (`aletheia` `c475d18` · `sweeper` `047fde8` · `nurikabe` `7f41c88` · `browser-native-rts-poc` `7f9af4c` · `rustview` `ef017ad` · `humm` `0864269` · `adp` `6e13c3e` · `jsync` `23ea443`). That makes them `verified` for me as the reader; copied onto a page by someone who hasn't opened the files they're `reported` — which is the tier you named, so no change needed on your side.

All reads read-only. For the record: `sweeper`'s working tree carries uncommitted changes from the concurrent [DIG-49](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-49.md)/[DIG-50](https://repo.digikal.com/jawad/infrastructure/src/branch/main/docs/paperclip/shutdown-2026-10-03/issues/DIG-50.md) capture work that predate this run, so every `sweeper` claim was verified against committed `047fde8` rather than that tree.

Marking `done` — the rows are the deliverable and no follow-up sits on this issue. The two things I'd flag as genuinely worth someone's attention, neither blocking: the governance-block version drift (row 2 above), and `adp`'s unrun verification passes.

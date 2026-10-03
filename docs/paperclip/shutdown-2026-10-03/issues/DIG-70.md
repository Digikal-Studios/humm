# DIG-70: Domain page: UX Design

> Cross-reference copy. The primary archive for DIG-70 is in the `infrastructure` repository: https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-70.md. Attachments and earlier document revisions are stored there.

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (closing delta 2026-10-03) | done |
| Priority | medium |
| Owner (assignee) | GameExperienceDesigner |
| Created by | KnowledgeCurator |
| Paperclip project | Onboarding |
| Primary repository archive | infrastructure |
| Parent | [DIG-6](DIG-6.md) |
| Created / updated | 2026-09-25T18:16:34.600Z / 2026-09-25T18:26:46.457Z |
| Completed / cancelled | 2026-09-25T18:26:46.457Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `c4aa879a-2e6d-4579-b006-aa98501cbff7` |

## Description

##### Goal

Write the first version of **UX Design** — the hub page at
`knowledge/domains/ux-design/README.md`. It is a stub today. You own it per the
ownership map in the approved structure: game design, UI design and UX design are yours.

This is one of three briefs I am sending you in this pass (Game Design, UI Design, UX Design — the other two are their own issues). They are
independent — do them in any order, or in one sitting if that is cheaper. The structure
approval ([DIG-3](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-3.md)) has landed, so these pages publish as soon as they
arrive.

A domain page answers *how Digikal does this kind of work, across projects*. It stays
true if every current project is cancelled. That is the test that separates it from the
twelve `design.md` pages you already wrote.

##### Material you already have

**Read this first — one file in your own domain directory is not yours to write in
this issue.** `domains/ux-design/error-messages.md` is the four-part error-message
convention from your `domain-error-messages` document; I file it from
[DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md) as a **topic page**, per the placement decision in
[filing-plan](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md#document-filing-plan) §1.1 — it had outgrown a screen and
carried nine design reviews and nine tree SHAs in `sources` that the rest of UX design does
not share.

So for this page: **summarise the convention in one or two sentences under
`## Standards and conventions` and link `error-messages.md`. Do not restate its claims and
do not fold it back in.** The hub summarises and links; the topic page holds the claims and
their provenance. You own both.

Other material:

- **Your twelve design reviews** (`design-pages` on [DIG-38](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-38.md), schema
  from [DIG-36](https://repo.digikal.com/jawad/infrastructure/src/branch/docs/paperclip-shutdown-2026-10-03/docs/paperclip/shutdown-2026-10-03/issues/DIG-36.md) / [DIG-42](DIG-42.md)).
- **TechLead's [Software Engineering](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/software-engineering/README.md) hub**, filed
  today, which lands on the same seam from the other side and leaves you one explicit open
  question. Its rule 3 is *"a user-facing failure names the cause and the user's next
  action, and never renders a fabricated value"*, evidenced by jsync's four-step SSH
  remediation, browser-native-rts-poc's typed `PlacementError` mapped to copy only at the
  presentation boundary, and tvi rendering `"Need data"` — ranked honestly at the bottom
  rather than hidden, and asserted in a render test. **Their open question, routed to
  you: does that rule apply to developer-facing failures too, or only player-facing
  ones?** jsync's is developer-facing, tvi's is user-facing. Answering it here closes
  their row and is the highest-value thing on this page.

Questions worth answering if the material supports them — **skip any the evidence does not
reach**:

- Feedback for long work: four projects run cancellable background work and poll from the
  UI. Is there a UX convention for what the user sees while they wait, or four different
  answers?
- Refusing to fabricate: `"Need data"` is one instance. Is "never show a number you do not
  have" a studio rule, and where else does it hold or fail?
- Accessibility: is there any checklist in use today? If not, that is the finding, and
  naming the first three things that should be on it is worth more than a generic list.

##### Boundaries — what belongs on the sibling pages, not this one

- Screens, controls, colour, typography, contrast →
  [UI Design](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/ui-design/README.md) (your other brief).
- Rules, systems, difficulty, progression → [Game Design](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/game-design/README.md)
  (your other brief).
- Developer-facing error *handling mechanics* — typed errors, where copy is produced,
  `panic` policy → [Software Engineering](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/software-engineering/README.md) (TechLead,
  filed). The wording the user reads is yours; the machinery is theirs.
- Any one project's flows → that project's `design.md`.

##### The template — copy this shape exactly

```
---
title: <Domain Name>
owner: GameExperienceDesigner
last_reviewed: 2026-09-25
confidence: verified | reported | assumed
sources:
  - "<retrievable thing: issue id + date, a file path at a SHA, a named review>"
---

# <Domain Name>

## What this covers
Two or three sentences. Also state what it explicitly does **not** cover, with a
link to the domain that does.

## How Digikal does this
The studio's actual practice, not a textbook summary. If we have no practice yet,
say so — an honest gap beats generic filler.

## Standards and conventions
Concrete rules that apply across projects.

## Open questions
| Question | Owner | Blocks |
| --- | --- | --- |

## Where this shows up
Projects that exercise this domain, as examples only. This page must stay true if
every one of them is cancelled.

## Sources

---
[← Knowledge base](../../README.md)
```

###### The rules your page is checked against

These come from the founder-approved structure document (DIG-3, approved 2026-09-25). You
do not need to read it — everything binding is here.

1. **All six sections, in that order, with those exact headings.** One H1, matching
   `title` exactly. H2 for sections, H3 below. No H4 — if you need one, the section wants
   to be a topic page.
2. **Front matter: five fields, none empty.** `owner` is exactly one agent (you).
   `sources` is never empty and every entry must be *retrievable* — an issue id **and its
   date**, a file path at a named SHA, a named review. "My design experience" is not a
   source.
3. **`confidence` is the page's weakest claim**, and it records *your* relationship to the
   artifact, not the claim's pedigree: `verified` = you checked it against the primary
   artifact (the code, a running build, a signed-off document) on or after
   `last_reviewed`; `reported` = you or a report stated it and nobody re-checked;
   `assumed` = inferred or placeholder. Your twelve design reviews are `verified` **from
   your seat** because you read the source — say so in `sources` and claim the tier.
4. **No unattributed claims.** Every factual statement traces to a named artifact. A
   judgement call is welcome but must be labelled inline — write
   `*GameExperienceDesigner judgement: …*`. A labelled judgement does **not** lower the
   page's confidence; an unlabelled one is a defect.
5. **No generic textbook content.** If a paragraph would be equally true at any studio,
   cut it. A short accurate page beats a long generic one. This is the single most common
   way a domain page fails review.
6. **Name gaps as gaps.** "Digikal has no practice here yet, and here is what would
   settle it" is a valid and valuable section. Do not fill a gap with plausible
   convention — that is the one thing that makes the page actively harmful.
7. **`## Where this shows up` is examples only.** The page must still be true if every
   project listed is cancelled. Anything that is *project state* belongs on that project's
   `design.md`, not here.
8. **Escape pipes inside table cells — write `\|`.** A `|` ends the cell *even inside
   backticks*, and it silently shifts every later value one column left. Your Open
   questions table is the exposed one.
9. **Boundaries are the point of a domain page.** `## What this covers` must name what
   this page does *not* cover and link the sibling that does. Relative links with the
   `.md` extension, e.g. `../ui-design/README.md`.

###### Length

Calibrate against the three landed hubs: ~2,000–2,400 words each. Treat that as the
**ceiling, not the target**. If a section outgrows a screen *and* has its own distinct
sources, tell me and we make it a topic page inside your domain rather than growing the
hub — that is exactly what happened with `error-messages.md`.

###### What to hand back

Put the finished page in a document with key **`content`** on **your own issue** —
the complete file, front matter to back-link, byte-for-byte what I should write to disk.
Then mark your issue `done` with a short comment. Do not write into `knowledge/`
yourself and do not comment on [DIG-6](DIG-6.md) — I wake automatically when
your issue closes, and I do the filing and the curation pass.

I may change structure, headings, ordering, links and front-matter hygiene on my own. I
will **not** change a claim without asking you. You stay accountable for the page being
true; I am accountable for it being findable and consistent.

###### Already on disk for you to read

The three TechLead hubs are filed and are the standard to match — same workspace,
`knowledge/domains/{software-engineering,systems-architecture,tooling-pipeline}/README.md`.
The rules above in one screen: `knowledge/CONVENTIONS.md`. Your template:
`knowledge/_templates/domain.md`.


## Documents (latest revision)

<a id="document-content"></a>

### Document `content`: knowledge/domains/ux-design/README.md

Revision 1 (id `740b7732-7419-4faa-a122-5bfa60b2d653`), updated 2026-09-25T18:26:10.496Z by GameExperienceDesigner.

---
title: UX Design
owner: GameExperienceDesigner
last_reviewed: 2026-09-25
confidence: verified
sources:
  - "Twelve design reviews by GameExperienceDesigner, all 2026-09-25: DIG-21 aletheia, DIG-22 browser-native-rts-poc, DIG-23 nurikabe, DIG-24 sweeper, DIG-25 RustView, DIG-26 humm, DIG-27 tvi, DIG-28 zimam, DIG-29 ai-engineering-governance, DIG-30 jsync, DIG-31 adp, DIG-32 infrastructure — all twelve re-read in full this pass from the `design-pages` document on DIG-7, 2026-09-25"
  - "humm@0864269 `docs/design/UX_SPEC.md` §4, §6, §J-1; `crates/humm-gui/src/format.rs:102-125`; `views/task_table.rs:357`, `:367`; `views/options_sheet.rs:761`; `assets/i18n/en/humm.ftl:107`"
  - "nurikabe@7f41c88 `crates/nurikabe-generator/src/lib.rs:55-62`; `crates/nurikabe-game/src/session/jobs.rs:297`, `shell/entry.rs:219`, `meta/desk_model.rs:76-95`, `board/gesture.rs:848-858`, `board/shaders/board.wgsl:258`, `:284`, `contrast.rs:105-640`, `settings.rs:68-145`; `README.md:36-37`; `docs/GENERATOR_AUDIT.md:100-101`"
  - "tvi@64aee5c `src/finance/conviction.rs:576`; `src/ui/components/detail_verdict.rs:85-87`, `decision_brief.rs:17-20`; `src/ui/screens/screener.rs:331-335`, `:577-583`, `home.rs:397-489`; `src/app/mod.rs:1941-1957`, `:6059-6098`; `src/app/input.rs:1544-1552`; `src/config.rs:635-693`"
  - "RustView@ef017ad `src/app.rs:390-398`, `:460-468`, `:505-508`, `:695-708`, `:955-961`, `:1017-1043`; `src/file_nav.rs:4-9`, `:254-258`; `src/ui/status_bar.rs:9-30`; `src/image_loader.rs:388`"
  - "browser-native-rts-poc@7f9af4c `crates/client/bootstrap.js`; `crates/client/src/ui/overlays.rs:7-43`; `src/input.rs:318-328`; `src/render.rs:1087-1114`; `index.html:32`"
  - "aletheia@c475d18 `crates/aletheia_core/src/state.rs:9-18`; `crates/aletheia_game/src/hud.rs:500-519`; `dev/editor/accessibility_test.rs:20-36`"
  - "sweeper@274b86c `src/main.rs:307-313`, `README.md:41`; jsync@23ea443 `bin/jsync:484-518`; infrastructure@cc17670 `docs/operations.md:14-19`; ai-engineering-governance@11ec305 `Makefile:9`; zimam `PDS:1182`, `PDS:1190`"
  - "DIG-3 `design` document (Knowledge Base Structure and Maintenance Rules), revision 7e0aa87c, approved by the founder 2026-09-25 — the rules this page is written to"
  - "GameExperienceDesigner judgement, labelled inline wherever it appears"
---

#### UX Design

##### What this covers

How Digikal's software behaves toward the person operating it: the shape of a first
run, what the interface says while it is working, what it says when it fails, what it
lets the user take back, and who is locked out. It covers the wording a person reads
and the sequence they move through, on every surface we ship — desktop GUI, browser
game, terminal UI, CLI and runbook.

It does **not** cover screens, controls, colour, typography or contrast measurement,
which is [UI Design](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/ui-design/README.md); or rules, systems, difficulty and
progression, which is [Game Design](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/game-design/README.md). It does not cover how a
failure is *detected* or where copy is produced — typed errors, presentation
boundaries and `panic` policy belong to
[Software Engineering](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/software-engineering/README.md). The wording the user reads
is ours; the machinery is theirs. The shape of a blocking error message has its own
page here: [Error Messages](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/ux-design/error-messages.md). Any one product's flows belong on that
project's `design.md`.

##### How Digikal does this

**Every product was built by its only user, so first run is the one path the author
never walks.** aletheia has no title screen, no controls prompt, no pause and no exit —
`AppState` declares `MainMenu` and nothing transitions to it
(`crates/aletheia_core/src/state.rs:9-18`). browser-native-rts-poc shows nine dense
lines before the player has seen a unit and offers no way to reopen them
(`crates/client/src/ui/overlays.rs:7-43`). humm asks a new user to choose among
fourteen model sizes with no size, speed or quality shown, and the "benchmark my
machine and pick for me" banner its own spec promises was never built (`UX_SPEC.md`
§6). sweeper has eleven keyboard commands, zero in-app discoverability, and a README
documenting bindings that do not exist (`README.md:41`). nurikabe is the exception and
shows what the fix costs: the Desk resolves to exactly one hero action by a pure rule
(`meta/desk_model.rs:76-95`). *GameExperienceDesigner judgement: single-author
development is the explanation, not a coincidence — nobody here has been a first-time
user of a Digikal product since the first commit.*

**Empty, loading and error are three distinct states in two products and two states in
the rest.** nurikabe writes copy for empty sheet, cancelled job, stale verdict and a
disabled Restore; tvi branches its empty states on *why* they are empty
(`screener.rs:331-335`) and shares one actionable empty state across six tabs
(`src/app/mod.rs:1941-1957`). Against that, RustView opens a video by clearing the
media and parsing the header synchronously on the UI thread, so the screen that says
no file is open is what it shows *while a file is opening*
(`src/app.rs:390-398`, `:955-961`, `:1017-1043`).

**Feedback for long work is not four different answers — it is one answer, written
once, that three other projects already own the plumbing to satisfy.** humm's
`UX_SPEC.md` §4 states it: determinate progress where knowable, a phase label, a cancel
affordance, and a terminal state that is never silent. The code keeps it — the Status
column names the phase, not just a percentage (`Downloading model (30%)`,
`In Progress (50%) — ETA: 3m 42s`, `format.rs:102-125`) — and indeterminate states are
honestly animated, "an animated bar, never a fabricated one". The other three fail it
with material they already hold. nurikabe's generator exposes
`GenerationProgress { phase, attempt }` and the game passes `&mut IgnoreProgress` and
discards it (`session/jobs.rs:297`), leaving a sweeping strip and the word "Checking…".
tvi knows both the request count (`Est. calls 55`, `screener.rs:577-583`) and the exact
pacing interval and converts neither into a time estimate for a multi-minute scan.
browser-native-rts-poc ships a static "Downloading game…" against an 8 MiB budget, with
reload — which restarts the download — as the only affordance (`bootstrap.js`).

> **Reported, not re-measured.** nurikabe's own release soak records Challenging p95
> 8.40 s and Expert p95 15.88 s, max 46.86 s (`docs/GENERATOR_AUDIT.md:100-101`), on
> the author's machine. I read the figures; nobody re-ran them.

**Forgiveness is confirm-shaped, and the guard is usually on the cheaper action.**
Confirm is common across the twelve, undo is rare, recover is rarer. humm has confirm
and neither of the other two while autosaving every keystroke in its transcript
editor, and says so itself: "This action cannot be undone." (`humm.ftl:107`). tvi needs
`d` then `d` to remove a watchlist row (`src/app/mod.rs:6059-6098`) while one
unconfirmed `Esc` discards four steps of typed reasoning at the end of the buy gate
(`src/app/input.rs:1544-1552`). RustView's `Escape` stack teaches "back" on three rungs
and quits on the fourth (`src/app.rs:695-708`). *GameExperienceDesigner judgement:
confirm is the weakest leg precisely because it is the cheapest to add, and it is
dismissed on autopilot when it answers a habitual keystroke — which makes humm binding
`⌘S` to Clear History a worse defect than its missing undo stack.* Worth protecting by
name: nurikabe commits a whole drag as one undo transaction
(`board/gesture.rs:848-858`), and zimam requires typed confirmation only where
protected paths or elevated permissions are actually present (`PDS:1182`).

**Refusing to fabricate is genuine practice, and broader than "never show a number you
do not have."** tvi is the reference: missing provider data becomes the verdict
`Need data` rather than a zero, enforced upstream in the engine
(`conviction.rs:576`), and it *suppresses* the "journal your decision" prompt because
journaling a call on absent evidence is premature (`detail_verdict.rs:85-87`). zimam
refuses to render a clean review when the reviewer merely said nothing ("Coverage, not
silence", `PDS:1190`). But that rule as written in
[Software Engineering](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/software-engineering/README.md) catches only one of three
failure shapes, and the other two are live in the tree — rule 3 below.

**Accessibility has three independent instances and no checklist.** nurikabe computes
fifteen contrast assertions over text pairs, board fills, the shader's ring tones and
switch states (`contrast.rs:105-640`). aletheia's *editor* runs every journey through
AccessKit at 200% zoom and 320×450 (`accessibility_test.rs:20-36`) while the game side
has no accessibility section in `gdd.md` at all. zimam specifies a CI gate and has no
implementation. Nothing is shared between the three, and nine of the twelve products
have no accessibility artifact of any kind. Rule 7 is the first three rows of the
checklist that does not exist, chosen by what actually failed most often rather than by
what a generic list would contain.

##### Standards and conventions

1. **A blocking message has four parts** — name the problem in the user's terms with
   the deciding value, number the fix, end with a command that proves it worked, and
   foreclose the dangerous workaround by name. The claims, evidence and anti-patterns
   are on [Error Messages](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/ux-design/error-messages.md); that page is the authority and this line
   is only the pointer.

2. **The register belongs to the surface the string lands on, not to the layer that
   produced it.** This answers the question
   [Software Engineering](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/software-engineering/README.md) routes here: their rule 3
   applies to developer-facing failures too, unchanged. The studio's four best
   instances of "cause plus next action" are all developer- or operator-facing —
   jsync's SSH remediation (`bin/jsync:484-518`), infrastructure's
   `docs/operations.md:14-19`, ai-engineering-governance's `Makefile:9`, and tvi's
   `startup_error_message` / `startup_fix_hint` pair (`src/config.rs:635-693`) — so
   there is no evidence we hold developer copy to a lower bar and plenty that we hold
   it higher. What *is* audience-dependent is vocabulary: jsync's precision ("linked
   worktree", "alternates file") is an asset for its one reader and a wall for the
   second engineer, who meets those terms while already blocked. The failure our
   reviews actually found is not "developer copy is worse" — it is a developer-facing
   string escaping onto a player-facing surface: nurikabe's
   "Uniqueness proof exceeded its safe bound." reaching a puzzle player through
   `shell/entry.rs:219`, RustView interpolating a decoder library's own text
   (`src/image_loader.rs:388`), humm putting ffmpeg's raw stderr in the Status column.
   Write to whoever reads the surface; keep the precise form behind a "Technical
   details" disclosure or on stderr.

3. **Never render a value whose truth conditions differ from what the surface
   implies.** "Never show a number you do not have" is one of three shapes and the
   least common here. *Absent shown as inapplicable:* tvi renders unknown evidence
   coverage as `—` (`decision_brief.rs:17-20`), which reads as "not applicable" rather
   than "unknown" — in the product that made "Need data" a first-class verdict.
   *Stale shown as current:* RustView composes one status row from two sources that
   update at different times, so during navigation the previous file's name and
   dimensions sit beside the new file's counter (`src/ui/status_bar.rs:9-30`); humm's
   options-sheet summary reads "Whisper · Small" when the OpenAI API backend is selected
   (`options_sheet.rs:761`). *True of the system, false as an answer:* RustView reports
   `3 / 3` in a folder of 400 files, because unsupported formats are excluded by the
   scan and never counted (`src/file_nav.rs:4-9`, `:254-258`).

4. **Long work states its phase, shows determinate progress where the number exists,
   offers a reachable cancel, and never ends in silence.** humm's `UX_SPEC.md` §4
   promoted from one project's policy to a studio rule, because it is the only version
   anyone here wrote down and because the three projects that fail it fail with data
   they already hold. "Reachable" is load-bearing: humm's own model-download Cancel
   existed and sat outside the viewport until M8.

5. **Empty, loading and error are three states, never two.** An empty state doubling as
   a loading state is one defect (RustView, above); an error state rendered by blanking
   the surface is the other — `clear_view` resets the title and blanks the bar to
   `No file loaded` while the navigator still holds a valid position
   (`src/app.rs:505-508`).

6. **Prefer undo to confirm, and put the guard on the expensive action rather than the
   cheap one.** Our products currently do the reverse often enough that it is worth
   checking deliberately.

7. **The first three rows of the accessibility checklist we do not have.**
   - **Does the non-colour channel survive the pipeline it renders through?** All three
     failures are in products that *did* the colour-independence work and lost it
     downstream: nurikabe suppresses the island dot and `≈`/`×` glyphs in thumbnails
     and below 12 px cells, making its twelve teaching boards the least readable boards
     in the product (`board.wgsl:258`, `:284`); the RTS draws relationship rings at
     fixed *world* size, so they fall to ~11 px at 4v4 zoom (`render.rs:1087-1114`);
     aletheia's era pipeline desaturates HUD tones until `Good` and `Warning` collapse
     to roughly 1.01:1 at FL0 (`hud.rs:500-519`). Check at the size and in the mode the
     thing actually ships in.
   - **Is input remappable, and does anything already claim it is?** Present in one
     product (humm: twenty shortcuts, live re-apply, `Esc` reserved); absent in sweeper,
     browser-native-rts-poc, aletheia, RustView and tvi. nurikabe has the complete data
     model — 22 actions across keyboard, controller and mouse (`settings.rs:68-145`) —
     no UI for it, and a README promising "per-action remapping" (`README.md:36-37`).
     The cheapest first rung is comparing the claim to the product.
   - **Can every primary verb be reached from the keyboard?** humm's task table mutates
     selection only inside pointer handlers (`task_table.rs:357`, `:367`), so a keyboard
     user can run a transcription and then cannot open the result — a Level A failure
     transitively blocking every downstream verb. browser-native-rts-poc gives its
     canvas `tabindex="0"` and a focus ring that buys nothing (`index.html:32`).

##### Open questions

| Question | Owner | Blocks |
| --- | --- | --- |
| Does Digikal claim accessibility, and at what level? Nine of twelve products have no accessibility artifact; three have good, uncoordinated ones. The honest options are a studio floor (rule 7 on anything with a user), per-product claims, or an explicit "not a goal" so reviews stop raising it. Half-claiming is the worst of the three. | Founder | Whether a project may ship a partial keyboard path; the accessibility section of every `design.md` |
| Is a first-run path in scope while the author is the only user of each product? Every product fails here and the fix is cheap in each — but it is only worth funding if someone other than the author is expected to open these. | Founder | aletheia ALE-01, browser-native-rts-poc RTS-02, humm HUM-06, sweeper SWP-10, nurikabe NUR-10 |
| Should rule 4 be a written studio standard with a named owner, or stay one project's policy? Promoting it means someone maintains it and three projects gain rows on their pages. | CEO, with TechLead on where a shared copy convention would live | Whether a `design.md` can cite a missing progress phase as a defect rather than a note |
| One studio voice, or a voice per product? nurikabe's blame-free whisper register, tvi's dense cockpit and jsync's `jsync: warning:` prefix are three deliberate, incompatible registers. My view is that voice is per-product while the rules on this page are studio-wide — but that is a founder call and it decides whether a copy pass is ever shared. | Founder | Any cross-product copy work; the reading-level question open on [Error Messages](https://repo.digikal.com/jawad/digikal-knowledge-base/src/commit/7dc917c3324a18354613e38fa36138cbdc747fc9/knowledge/domains/ux-design/error-messages.md) |

##### Where this shows up

Examples only. Every rule above survives all of these being cancelled.

- **humm** — the reference for rule 4 and the counter-example for rule 6: the studio's
  only written progress convention, and no undo anywhere alongside per-keystroke
  autosave.
- **tvi** — the reference for refusing to fabricate and for empty states that branch on
  cause; also the clearest case of the guard sitting on the cheap action.
- **nurikabe** — the reference for onboarding, undo granularity, and computed rather
  than eyeballed accessibility.
- **jsync**, **infrastructure**, **ai-engineering-governance** — the studio's best
  cause-plus-next-action copy, all three developer- or operator-facing. The evidence
  behind rule 2.
- **RustView** — counter-example on three rules at once: welcome screen as loading
  state, stale values rendered as current, a true count read as a false answer.
- **browser-native-rts-poc** — both ends: the best in-product refusal copy in the studio
  and the least designed loading experience.
- **aletheia**, **sweeper**, **zimam**, **adp** — each supplies one row above; none is
  load-bearing for a rule on its own.

##### Sources

Listed in full in the front matter. Every factual statement traces to the twelve design
reviews on DIG-21 through DIG-32 (all 2026-09-25, all re-read in full this pass), to a
file path at a named tree SHA recorded in them, or to GameExperienceDesigner judgement,
labelled inline.

> **Method note.** No product was built, run or played, and `confidence: verified`
> records my relationship to the artifact, not a measurement of anyone using one. What
> is verified is that I read all twelve design reviews in full on 2026-09-25 and that
> every claim above is structural and re-checkable at the cited line — a progress
> channel constructed and discarded, a summary string that never reads a backend field,
> a selection mutated only inside pointer handlers, a shader condition suppressing a
> glyph below a size. The conventions in Standards are **proposed studio practice
> inferred from that evidence**: recommendations, not measurements. The one figure that
> is neither structural nor mine carries an inline `Reported` marker above. Twelve
> products have been reviewed and **none has been observed in use by anybody**, which
> is the largest limitation on this page.

---
[← Knowledge base](https://repo.digikal.com/jawad/infrastructure/src/commit/375380865bd9ba0f43e71e86654134362c50174e/README.md)


## Comments (1)

<a id="comment-b11622b8-ee45-474f-a8c6-226d67acbe0b"></a>

### 2026-09-25T18:26:42.773Z — GameExperienceDesigner

Comment id `b11622b8-ee45-474f-a8c6-226d67acbe0b`; written by run `db11841e-8a5e-4e5e-a1d2-0f3eec4309e7`.

**Done — page delivered as document `content` on this issue, byte-for-byte what to write to `knowledge/domains/ux-design/README.md`.** Verified from a GET, not the PUT echo (revision `740b7732`, 18,680 bytes, SHA matches local).

**The highest-value thing on it — TechLead's routed question is answered in Standards rule 2.** Their rule 3 applies to developer-facing failures too, unchanged. The studio's four best instances of "cause plus next action" are *all* developer- or operator-facing (jsync `bin/jsync:484-518`, infrastructure `docs/operations.md:14-19`, ai-engineering-governance `Makefile:9`, tvi `startup_error_message`/`startup_fix_hint`), so there is no evidence we hold developer copy to a lower bar. What is audience-dependent is vocabulary, not the rule. The real failure our reviews found is a developer-facing string escaping onto a player-facing surface — nurikabe's "Uniqueness proof exceeded its safe bound." reaching a puzzle player, RustView's decoder passthrough, humm's ffmpeg stderr in the Status column. Rule: **the register belongs to the surface the string lands on, not the layer that produced it.** That closes their row.

**The other three questions, all answerable from the evidence:**

- **Long work — not four answers, one.** humm's `UX_SPEC.md` §4 (phase label, determinate progress where knowable, reachable cancel, terminal state never silent) is the only written version in the studio, and the three projects that fail it fail with plumbing they already own: nurikabe constructs `GenerationProgress { phase, attempt }` and passes `&mut IgnoreProgress`; tvi knows both the request count and the pacing interval and converts neither to time; the RTS ships a static "Downloading game…". Promoted to studio rule 4.
- **Refusing to fabricate is broader than "never show a number you do not have"** — three failure shapes, two of which the Software Engineering wording does not catch: absent shown as inapplicable (tvi's `—` for unknown coverage), stale shown as current (RustView's status row), and true-of-the-system-false-as-an-answer (RustView's `3 / 3` in a folder of 400). Rule 3.
- **Accessibility — no checklist exists.** Three independent instances (nurikabe's 15 contrast assertions, aletheia's *editor* AccessKit suite, zimam's unimplemented CI gate), nothing shared, nine of twelve products with no artifact at all. Rule 7 is the first three rows, chosen by what failed most across the twelve reviews, not by what a generic list contains.

`error-messages.md` is summarised in one line under Standards and linked; its claims are not restated.

**Two things for your filing pass:**

1. **Length: 2,483 words, ~80 over the 2,400 ceiling.** Every paragraph carries cited evidence and I would rather you see it than have me cut a claim to hit a number — but if you want it inside the range, the cheapest 100 words are the `## Where this shows up` bullets, which are pure roll-up of evidence already stated above.
2. **I deliberately did not link project `design.md` pages.** `knowledge/projects/` currently holds only `README.md`, so those links would be broken on arrival and the freshness pass would flag them as orphans. Projects are named in prose with the review issue cited instead. Adding the links once the twelve project pages land is yours under structure/links — please do.

No section outgrew a screen with its own distinct sources, so nothing here wants to be a second topic page.

**Still open on my side:** DIG-68 (Game Design) and DIG-69 (UI Design) from the same pass — untouched this heartbeat, which was scoped to this issue.

## Agent runs (2)

Every Paperclip heartbeat run recorded against this issue. A run's results are the comments, documents, attachments and commits it wrote (comment headers above name the run). Raw run transcripts are not archived, except the runs listed with a transcript copy below; see [inventory.md](../inventory.md#exclusions).

| Run | Agent | Status | Started | Finished | Log bytes | Transcript copy |
|---|---|---|---|---|---|---|
| <a id="run-919511d4-c6ab-46c5-9b8b-e38c79a3fb00"></a>`919511d4-c6ab-46c5-9b8b-e38c79a3fb00` | KnowledgeCurator | succeeded | 2026-09-25T18:10:38.050Z | 2026-09-25T18:21:19.808Z | 263032 | — |
| <a id="run-db11841e-8a5e-4e5e-a1d2-0f3eec4309e7"></a>`db11841e-8a5e-4e5e-a1d2-0f3eec4309e7` | GameExperienceDesigner | succeeded | 2026-09-25T18:16:34.879Z | 2026-09-25T18:27:22.718Z | 240787 | — |

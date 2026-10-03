# DIG-26: Design review: humm

| Field | Value |
|---|---|
| Status at cutoff (2026-10-03T06:12:00Z) | done |
| Status at export (closing delta 2026-10-03) | done |
| Priority | high |
| Owner (assignee) | GameExperienceDesigner |
| Created by | GameExperienceDesigner |
| Paperclip project | humm |
| Primary repository archive | humm |
| Parent | [DIG-7](DIG-7.md) |
| Created / updated | 2026-09-25T14:16:47.473Z / 2026-09-25T14:27:46.809Z |
| Completed / cancelled | 2026-09-25T14:27:46.773Z / — |
| Blocked by | — |
| Blocks | — |
| Paperclip id | `eb07af0b-add3-4790-a2fb-a1bff0994ee1` |

## Description

##### Goal
Design-side review of **humm** from the player's or user's seat. Child of [DIG-7](DIG-7.md).

##### What this project is
macOS desktop (egui) + CLI audio/video transcription app; a Rust reimplementation of Buzz. Whisper.cpp, watched folders, live mic, diarization, translation, summaries, exports. Declared done at 46/50 parity rows.

##### Review depth
**DEEP** — full review. Read the actual UI/input/HUD/menu source and assets, not just the Tech Lead's report. Walk the first-run path, the core loop, and the failure/empty/loading states.

##### Inputs (read-only)
- Tech Lead status report: [DIG-14](DIG-14.md#document-report) (document key `report` on issue `dca0f1f2-2b88-4104-a5ec-619469e5158f`)
- Repository checkout: `/paperclip/instances/default/projects/8e469247-d6ea-4681-b627-51d88fec4b14/4b20acc2-e9a9-40bb-99cc-19366e78e905/humm`
- **The repository is read-only for this task. Do not modify, build, or run anything in it.**

##### Deliverable
A document with key `review` on THIS issue, containing:
1. **What it is from the player's / user's seat** — one paragraph, no engineering framing.
2. **Strengths worth protecting** — what a redesign must not break.
3. **Risks ranked by impact** — each one cites concrete evidence (a screen, a flow, a file path and line, an asset) AND offers at least one concrete alternative. No risk without an alternative.
4. **Accessibility** — WCAG POUR, colour independence, remappable input, reduced motion, assist options, reading level.
5. **Ethics** — monetization and retention mechanics if any exist; name dark patterns explicitly. Say "none present" if none.
6. **Open questions for the founder.**

Cite design lenses by name where they drive a judgment (core loop, MDA, flow channel, meaningful choice, progression/pacing, juice, Nielsen heuristics, Norman affordances/signifiers/mapping, recognition over recall, progressive disclosure, Hick's, Fitts's, Doherty, peak-end, forgiveness, Gestalt, platform conventions).

##### Done when
The `review` document exists, every risk has evidence and an alternative, open questions are listed. Then set this issue to `done` and post a short comment pointing at the document. The roll-up happens on [DIG-7](DIG-7.md); do not post findings there.


## Documents (latest revision)

<a id="document-review"></a>

### Document `review`: humm — Design Review (UX / UI / interaction)

Revision 1 (id `d80fe676-77fa-41cc-999f-880ada828b15`), updated 2026-09-25T14:27:31.775Z by GameExperienceDesigner.

#### humm — Design Review (UX / UI / interaction)

**Scope:** DIG-26, DEEP review. Read-only pass over the repository checkout at
`.../4b20acc2-e9a9-40bb-99cc-19366e78e905/humm` @ `0864269`, plus the Tech Lead's status
report ([DIG-14](DIG-14.md#document-report)). Nothing was built, run, or modified.
Evidence is source and shipped copy: `crates/humm-gui/src/**`, the English string catalog
`crates/humm-gui/assets/i18n/en/humm.ftl`, `docs/design/UX_SPEC.md`, and the windowed
acceptance record `docs/gui-manual/M8.md`.

**Reviewer's note on method:** humm is a desktop utility, not a game, so the game-design
lenses (core loop, MDA, flow channel, juice) apply by analogy — I use "core loop" for the
repeated user cycle and "flow channel" for the match between task difficulty and what the
app makes easy. The UX, UI and accessibility lenses apply directly and carry most of the
weight here.

---

##### 1. What it is, from the user's seat

You have a pile of recordings — interviews, lecture captures, a podcast episode, a video
someone sent you — and you need the words out of them. You drag the files onto humm's window,
a single form appears with sensible settings already filled in from last time, you press Run,
and a table row ticks up a percentage while your Mac's fans stay quiet. When it finishes you
double-click the row and get a transcript you can read, search, correct word by word against
synced playback, nudge the timings of, have translated, have the speakers named in, and export
as subtitles. If you'd rather not touch it at all, you point it at a folder and it transcribes
whatever lands there; if you'd rather talk to it live, it opens a microphone window that types
what you say in real time and can throw that onto a second screen for an audience. Everything
it does happens on your own machine unless you deliberately hand it an API key. The whole thing
is fast, calm, and refreshingly free of accounts, upsells and spinners — and it is also, today,
a tool that assumes you already know what a "small ggml model" is, never tells you where it put
your files, and cannot undo anything you do to a transcript.

---

##### 2. Strengths worth protecting

A redesign must not break any of these.

1. **Nothing hides behind a spinner.** `UX_SPEC.md` §4 set the rule — "determinate progress
   where knowable, phase label, cancel affordance, and a terminal state that is *never*
   silent" — and the code keeps it. The Status column names the *phase*, not just a
   percentage (`crates/humm-gui/src/format.rs:102-125`: `Downloading model (30%)`,
   `Downloading media (75%)`, `isolating speech`, `In Progress (50%) — ETA: 3m 42s`), and
   indeterminate states are honestly animated rather than faked
   (`views/speaker_id.rs:74-80`, "an animated bar, never a fabricated one"). This is the
   single best thing about the product's feel and it should be treated as non-negotiable.

2. **Model downloads never block the import.** Choosing a model you don't have queues the
   task anyway and downloads as the task's first phase (`UX_SPEC.md` §J-1). This is the
   correct call and a real improvement on the original. Protect the non-modality.

3. **The sheet is sticky and Enter-able.** The options sheet prefills from last use and
   persists on *every* exit path including Cancel and Escape
   (`views/options_sheet.rs:1011-1013`), and Run fires on Enter when no text field has focus
   (`:1125-1136`). Repeat runs really are Enter-Enter. That is the core loop working.

4. **Settings commit on change — no OK/Cancel anywhere in Preferences**, and the UI language
   re-renders live across already-open windows (`UX_SPEC.md` §J-11). Modern, correct, and
   rare in ported desktop apps.

5. **Destructive operations mostly produce new artifacts rather than mutating.** Speaker-ID
   Save writes a *new* transcript and leaves the source viewer and row untouched
   (`UX_SPEC.md` §J-7); the resizer defaults `resizer_create_new_transcript = true`
   (`docs/HUMM_SURFACE.md` §6.1). Keep that default posture.

6. **The ambient folder-watch pill.** A live "Watching <folder>" chip with a working pause
   toggle that drives the real service, not a label (`views/main_window.rs:579-592`;
   verified windowed in `docs/gui-manual/M8.md`). Buzz gave no ambient indication at all.
   Norman's *visibility of system state*, done right.

7. **Privacy copy that actually enumerates.** The Privacy tab lists the exact seven fields a
   telemetry event contains, in plain words
   (`assets/i18n/en/humm.ftl:266-278`: app version, system name, OS version, kernel release,
   CPU architecture, system locale, a random anonymous identifier). Most apps write "usage
   data". Protect this literal enumeration.

8. **Degraded capabilities disable rather than crash, with actionable tooltips naming the
   fix** — e.g. `options-extract-speech-missing-tooltip = demucs not found — install it
   (e.g. \`pipx install demucs\`) to enable speech extraction` (`humm.ftl:174`), and the
   same shape for yt-dlp (`:148`). Nielsen #9 done properly: the error says what to do.

9. **The keyboard-shortcut editor warns about conflicts but does not reject the binding**
   (`UX_SPEC.md` §3). Respecting the user's intent over the app's tidiness is the right
   ethic; keep it.

---

##### 3. Risks, ranked by impact

Each risk: evidence → why it matters → at least one concrete alternative.

---

###### R1 — There is no undo. Anywhere. And the editor autosaves every keystroke. **(highest)**

**Evidence.** A repository-wide search for undo functionality in
`crates/humm-gui/src` and `crates/humm-core/src/viewer` returns no undo stack, no revert, no
history — the only matches are unrelated test fixtures and one comment. Meanwhile the viewer
*instant-persists*: `UX_SPEC.md` §J-6 — "every committed edit persists synchronously and
flashes the 'edited · saved' indicator (`ViewerVm::saved_flash`)". The timing nudges are
bound to `⌘←/⌘→/⌘⇧←/⌘⇧→` (`shortcuts.rs:173-176`) at `NUDGE_MS = 500`
(`crates/humm-core/src/viewer/edit.rs:56`) and also write straight through.
Clear History deletes rows permanently behind a single confirm whose own copy says so:
`clear-confirm-body = Are you sure you want to delete the selected transcription(s)? This
action cannot be undone.` (`humm.ftl:107`).

**Why it matters.** *Forgiveness* is the lens, and humm currently has only one of its three
legs. It has **confirm** (on Clear History). It does not have **undo** and it does not have
**recover**. The combination of aggressive autosave and zero undo is worse than either alone:
the app's best feature — a transcript you can correct line by line against playback — is also
the place where a stray keypress is permanent. A user who selects a segment, fat-fingers a
character, and tabs away has silently destroyed a line of a 900-line transcript with no trace.
Four ⌘← presses that were meant for something else move a subtitle two seconds out of sync,
permanently. This is the one finding I would fix before any other.

**Alternatives (pick one, ideally both):**
- **(a) Per-viewer undo stack.** Bound `⌘Z` / `⌘⇧Z` to an in-memory ring of the last ~50
  edit operations (segment text, start, end), each already a discrete, typed operation in
  `humm_core::viewer::edit`. Because edits are already funneled through one validated
  mutation path, the inverse operation is cheap to record. Persist nothing extra — a
  session-scoped stack covers the realistic damage window.
- **(b) Soft-delete for Clear History.** Add a `deleted_at` column instead of
  `delete_tasks`, filter it out of the table, and show an undo affordance in the existing
  toast ("3 transcriptions removed — Undo") for the toast's 5-second life, then purge on
  next launch. Reuses the toast stack that already exists (`views/toasts.rs`).
- **(c) Minimum viable, if (a) is out of budget:** make segment edits commit on blur/Enter
  only (they largely do) and add an **Esc = revert this cell** while editing. One key, one
  state, no stack.

---

###### R2 — The task table cannot be operated without a pointing device

**Evidence.** Selection is mutated in exactly three places, all inside pointer handlers:
`views/task_table.rs:357` (`apply_row_click`, from `response.clicked()`), `:367`
(`select_only` on double-click), and `apply_row_click`'s own body at `:428-439`. The
keyboard handler, `handle_keyboard_shortcuts` (`:396-416`), handles only Delete, ⌘C and
Return — and returns immediately when the selection is empty (`:397-399`). Rows are drawn as
`ui.label(text)` plus a bare `ui.interact(rect, id, Sense::click())`
(`interactive_cell`, `:459-464`) — not focusable widgets. There is no arrow-key navigation,
no ⌘A, no Tab order into the table.

**Why it matters.** Every primary verb in the app is gated on selection: Open Transcript,
Cancel Transcription, Clear History, Restart, Rename, Notes
(`views/main_window.rs:193-209`, `toolbar_enablement`). A keyboard-only user — switch access,
severe RSI, VoiceOver — can launch humm, open the import sheet with ⌘O, and run a
transcription, and then **cannot open the result**. This is a WCAG 2.1.1 (Keyboard) failure
at Level A on the app's primary surface, and it also costs sighted power users: the table is
the hub, and it has no keyboard story at all.

**Alternative.** Give the table a focus cursor: an `Option<usize>` "focused row" on
`TableLayout`, driven by `Key::ArrowUp`/`ArrowDown` (with `Shift` extending from the existing
`layout.anchor`, `Home`/`End`, and `⌘A`), rendered with the same `row.set_selected` highlight
plus a focus ring. Add `Sense::focusable_noninteractive` (or an explicit
`ui.interact(...).request_focus()`) so the table is reachable by Tab. The selection model
(`select_only` / `select_range` / `toggle_selection`) already exists and is unit-tested — this
is a driver, not a redesign.

---

###### R3 — "Where did my file go?" The GUI never says where exports are written

**Evidence.** The options sheet builds every spec with `directory: None`
(`views/options_sheet.rs:521-530`), and `OutputSpec.directory`'s own doc says `None` writes
alongside the input file (`crates/humm-core/src/domain.rs:574`). The sheet has no
output-directory control at all — its rows are backend, model, task, language, word
timestamps, extract speech, export checkboxes, and an Advanced section
(`views/options_sheet.rs:1071-1118`). The completion toast reports a **count**:
`format!("{} — {} {}", tr("toast-completed"), written.len(), tr("toast-completed-suffix"))`
→ *"Transcription complete — 2 file(s) written"* (`app.rs:1212-1226`), discarding the
`written: Vec<PathBuf>` it is holding. There is no "Reveal in Finder" on a completed row
(the row context menu offers only Restart / Rename / Add-Edit Notes,
`views/task_table.rs:370-389`) — even though the Models tab *does* have a working
`models-show-location-button = Show file location` (`humm.ftl:286`).

**And it gets sharper for URL imports.** A URL task's media is fetched into
`media_dir/<task-id>/` (`crates/humm-core/src/queue.rs:1863`), which is
`~/Library/Application Support/humm/media/<task-id>/` (`docs/HUMM_SURFACE.md` §6). With
`directory: None`, "alongside the input file" therefore means *inside that opaque
UUID-named folder in Application Support*. A user who imports a URL with SRT checked gets a
subtitle file they will never find, and a toast that says "1 file(s) written".

**Why it matters.** This is the end of the core loop, and the Peak-End rule says the end is
what the experience is remembered as. Right now the loop ends with an unanswered question.
It is also a Nielsen #1 (visibility of system status) and Norman *feedback* failure: the
system knows the exact paths and chooses not to show them.

**Alternatives:**
- **(a) Cheapest, highest value:** make the completion toast name the destination and be
  clickable — *"Transcription complete — 2 files in ~/Movies"*, click reveals in Finder.
  The paths are already in `TaskEventKind::Completed { written }`; the toast stack already
  supports click actions (`ToastAction`, `app.rs:1335-1344`), and `open::that` is already
  used for the log folder (`:1339`).
- **(b) Add "Reveal exports in Finder" to the completed-row context menu**, mirroring the
  Models tab's existing button.
- **(c) Add a destination row to the options sheet** — a single "Save exports to: [Same
  folder as source ▾] [Choose…]" control, defaulting to current behavior, and make it
  mandatory-non-default for URL imports (where "same folder as source" is meaningless).
  Surface the resolved path in the sheet's existing one-line summary.

---

###### R4 — 15 locales shipped, and the most-read text in the app is hardcoded English

**Evidence.** `crates/humm-gui/src/format.rs` deliberately bypasses the Fluent catalog for
every status string: `"Queued"` (`:118`), `in_progress_text` → `"In Progress ({pct}%) — ETA:
{eta}"` (`:142`), `completed_text` → `"Completed (3m 5s)"` (`:157`),
`format!("Failed ({})", …)` (`:121`), `"Canceled"` (`:122`), `"Skipped"` (`:123`),
`"Downloading model ({}%)"` (`:106`), `"Downloading media ({}%)"` (`:110`). The module
doc-comment states the reason plainly: "every literal string here is byte-exact Buzz text".
Dates are likewise fixed to `%Y-%m-%d %H:%M:%S` (`:75`) with no locale formatting. The one
exception proves the rule — `Phase::Isolating` goes through `tr()` *because* it has no Buzz
original to be byte-exact about (`:112`, and the comment at `:96-101`).

**Why it matters.** The project ported all fifteen of Buzz's locales, bundled a CJK font to
close the tofu gap, and made language switching apply live across windows — a substantial,
genuinely user-serving investment. And then the Status column, which is the single cell a
user looks at most in the entire product, stays in English for all fourteen non-English
users. A German user sees `Status` translated as a header and `In Progress (42%) — ETA: 3m
42s` underneath it. This is an engineering constraint (byte-exact snapshot parity) leaking
into user-facing copy, which is exactly backwards: parity should be measured against
*behavior*, not against the literal bytes of another product's English.

**Alternative.** Move the status strings into the Fluent catalog with the current
byte-exact English as the `en` values. The byte-exactness tests then assert
`tr("status-in-progress")` under a pinned `en` locale, which is a *stronger* test (it pins
the catalog too) and costs nothing at runtime. The module's stated blocker — that `tr_args`
wraps placeables in bidi-isolation marks — is avoidable: use `tr()` for the pattern and
`format!` for the numbers, exactly as `StoreHealthBanner::message` already does for its own
`" (N)"` counter (`views/main_window.rs:158-168`). That precedent is in the codebase already.
Date columns should use a locale-aware format for display while keeping ISO in the store.

---

###### R5 — ⌘S deletes your history and ⌘X cancels your transcription

**Evidence.** `crates/humm-gui/src/shortcuts.rs:177-178`:
`ShortcutAction::ClearHistory => "Ctrl+S"`, `ShortcutAction::StopTranscription => "Ctrl+X"`.
These render as ⌘S and ⌘X on macOS. `UX_SPEC.md` §3 acknowledges it —
"including Ctrl+S = Clear History (surprising, but rebindable; a confirm dialog guards it)".
⌘S with a valid selection goes straight to the delete-confirm modal
(`views/main_window.rs:421-423` → `app.rs:1392-1396`). Other collisions in the same table:
`⌘P = Play/Pause` (`:170`), `⌘T = Timestamps view` (`:165`).

**Why it matters.** Norman's *mapping*, and platform convention as a safety mechanism. On
macOS, ⌘S means "save what I am looking at" with ~100% consistency, and users press it
reflexively and often — especially in an app whose central activity is *editing a
transcript*. Here it opens a dialog that proposes to permanently delete the selected
transcriptions. The confirm dialog is the only thing between a reflex and data loss, and
confirms are famously dismissed on autopilot when they appear in response to a habitual
keystroke. "It's rebindable" does not help the user who has not yet learned they need to
rebind it. Note this is a *default*, and a default is a design decision.

**Alternatives:**
- **(a) Change the two defaults** to macOS-safe equivalents (⌫/Delete already does Clear
  History; ⌘. is the conventional macOS "cancel current operation"). Keep the Buzz values
  reachable via Reset-to-Buzz-defaults if parity must be preserved as an option.
- **(b) If the defaults must stand:** make ⌘S in the main window a no-op that shows a toast
  — *"⌘S is bound to Clear History in humm. Change it in Preferences → Shortcuts."* — with
  a link. Loud, once, non-destructive.
- **(c) Minimum:** make the confirm modal's destructive button non-default (Cancel focused,
  Ok requiring an explicit click) and name the count and files: *"Delete 12 transcriptions?
  This cannot be undone."* Pair with R1(b) and the risk largely evaporates.

---

###### R6 — First run has no onboarding, and the one safety net arms only after you've been burned

**Evidence.**
- The empty state is two elements: `hero-drop = Drop media here, or ⌘O` and
  `hero-import-button = Import files…` (`humm.ftl:42-43`; `views/main_window.rs:640-649`).
  There is no explanation of what happens next, no mention of model download, no first-run
  tour.
- The default model is `ModelSize::Small` (`crates/humm-core/src/domain.rs:527`, a
  deliberate deviation from Buzz's `tiny`) — roughly 465 MB, and **not present on a fresh
  install**. The sheet shows the combo entry "Small" with no size, no download state, and no
  indication the first Run will pull half a gigabyte.
- `UX_SPEC.md` §6 promises a "first-run banner offers 'benchmark my machine and pick for
  me' ✚". **It was never implemented** — no such banner, string, or code path exists
  (`benchmark my machine` appears only in `UX_SPEC.md` itself; `measured_speed` is written
  only by `live_session::record_measured_speed` and read only by the Models-tab badge and
  the realtime-fit warning).
- Consequently the Models tab's speed badge is blank on a fresh install (stated openly in
  `UX_SPEC.md` §J-9: "genuinely 'once benchmarked on this machine' and empty on a fresh
  install"), and the live-recording realtime warning
  (`views/recording.rs:266-283`) requires a *measured* ratio `< 1.0`
  (`recording.rs:390-397`) — i.e. it cannot fire until after a session has already fallen
  behind.

**Why it matters.** *Flow channel*: the challenge on the first run (choose among 14 model
sizes, four of which map to the same backend, with no size, speed, or quality information)
is far above the skill of a new user, and the app provides no scaffolding. *Progression and
pacing*: the app's own measured-speed system — a genuinely good idea — is back-loaded so
that its guidance arrives only after the user has had the bad experience it exists to
prevent. And a fresh user's very first action silently commits them to a 465 MB download
they were never told about.

**Alternatives:**
- **(a) Build the promised banner, cheaply.** On first launch with no models present, show a
  one-line hero sub-heading: *"No speech model yet. humm will download **Small** (465 MB) the
  first time you transcribe — or pick a different size now."* with a link to Preferences →
  Models. Zero new machinery, resolves the surprise.
- **(b) Seed `measured_speed` at first launch** from a short synthetic benchmark (the M1
  benchmark harness already exists) or from a static per-device-class table keyed on
  `sysctl` machine model. Either makes the speed badge and the realtime-fit warning useful
  on day one instead of day two.
- **(c) Annotate the model combo and the Models tab rows** with size and a one-word quality/
  speed hint (`Tiny — fastest, roughest · 74 MB`), so the choice is informed at the point of
  decision rather than after it. See also R10.

---

###### R7 — The only route to a failure's real cause disappears after five seconds

**Evidence.** A failed task pushes a toast whose body is the error's **first line only**
(`app.rs:1227-1237`), carrying the full message in `ToastKind::TaskFailed { full_message }`.
The full text, the `toast-detail-open-log = Open log folder` button and the
`toast-detail-retry = Retry` button live in the toast's detail popover
(`humm.ftl:128-131`). Every toast — including failures — auto-expires after
`TOAST_DURATION = Duration::from_secs(5)` (`views/toasts.rs:21-22, 107-110`); only the
store-health banner is exempt, and the module doc says so explicitly (`:5-6`). Clicking the
failed **row** does not reopen the detail: the table's click handler only selects
(`views/task_table.rs:356-358`). What survives is the Status cell's
`Failed (<raw error>)` (`format.rs:121`) — which, on corrupt input, is ffmpeg's raw stderr:
`docs/gui-manual/M8.md` §8.4 records `decode failed: [mp3float @ 0x…] Header missing / Last
message repeated 73 times` landing in the Status column *and* the stored row.

**Why it matters.** Nielsen #9 (help users recognize, diagnose and recover). The user starts
a batch of 20 files, goes to make coffee, comes back to three failed rows, and the only
diagnosis available is a truncated C library diagnostic containing a pointer address, inside
a resizable table cell. Retry is reachable via the row context menu, but "Open log folder"
and the full error are not reachable at all.

**Alternatives:**
- **(a) Don't auto-expire failure toasts.** `ToastKind::TaskFailed` should require explicit
  dismissal, like the store-health banner. One-line change to the `expire` predicate.
- **(b) Make the Status cell the permanent door.** A click (or a dedicated "Show error…"
  context-menu item on Failed rows) reopens the same detail popover with full text, Open log
  folder and Retry. The popover component already exists.
- **(c) Humanize the top line.** Map the known failure classes to a one-line cause + fix
  ("Couldn't read this file — it may be corrupt or an unsupported format") and keep the raw
  stderr behind a "Technical details" disclosure in the popover. The actionable-message
  convention is already project policy (`CLAUDE.md`, "Actionable messages ('X missing — do
  Y')") — it just isn't applied to passthrough errors from subprocesses.

---

###### R8 — The options sheet's summary line misreports the backend

**Evidence.** `views/options_sheet.rs:761`:
```rust
format!("Whisper \u{b7} {model_part} \u{b7} {language_part} \u{b7} {formats_part}")
```
`summary_text` never reads `state.backend`. When the user selects **OpenAI Whisper API** in
the backend combo, the size combo is hidden (`:1072-1080`, `size_combo_visible`) and the Api
Key row appears — but the summary above still reads *"Whisper · Small · Detect Language ·
SRT+TXT"*, naming a local engine that will not run and a model size that has no effect.

**Why it matters.** The summary line exists precisely so repeat users can confirm-at-a-glance
and press Enter (`UX_SPEC.md` §J-1: "so repeat runs are Enter-Enter"). A confirmation
affordance that is wrong on the one setting with a cost and a privacy consequence attached —
local vs. "your audio leaves this machine" — is worse than no summary. Nielsen #1 and Norman
*feedback*: the readout contradicts the control directly above it.

**Alternative.** Derive the first segment from `state.backend`
(`"Whisper"` / `tr("options-backend-openai-api")`) and omit the size segment entirely when
the size combo is hidden — the visibility predicate `size_combo_visible(state.backend)`
already exists and is used two lines away. Add a unit test asserting the API-backend summary
contains neither `"Whisper ·"` nor a size. (Existing tests around `:1705` cover the local
case only.)

---

###### R9 — The export filename template is a whitespace-exact mini-language with no documentation and silent failure

**Evidence.** Substitution is literal string replacement, spaces included
(`crates/humm-core/src/export/mod.rs:182-187`):
```rust
.replace("{{ input_file_name }}", input_file_name)
.replace("{{ task }}", task.as_str())
.replace("{{ language }}", …)
.replace("{{ model_type }}", "Whisper.cpp")
.replace("{{ model_size }}", …)
.replace("{{ date_time }}", &date_time)
```
The Preferences field is a bare single-line `TextEdit` whose only affordance is hint text
showing the default template — which vanishes the moment the user types
(`views/prefs_general.rs`, the `prefs-general-export-template-label` row: `.hint_text(
DEFAULT_EXPORT_TEMPLATE)`). There is no placeholder list, no insert menu, and no preview.

**Why it matters.** *Recognition over recall*, textbook. A user must recall six token names
**and their exact internal spacing**. `{{input_file_name}}` — the spelling most people would
write — is silently emitted as literal text into their filenames. The failure is invisible
until they look at the folder, by which point a batch has run. `{{ model_type }}` is also a
fossil: it always renders the constant `"Whisper.cpp"` (`:158-163`), so it is a token that
can only ever produce one value.

**Alternatives:**
- **(a) Trim-tolerant tokens:** match `{{\s*name\s*}}` with a regex instead of six exact
  strings. `regex` is already a dependency of `humm-gui`. Removes the whole class of failure.
- **(b) Insert-token menu + live preview.** A small "＋" button listing the tokens with
  plain-English labels, plus a greyed line under the field showing the resolved filename for
  a sample input — *"e.g. interview-03 (transcribed on 2026-09-25).srt"*. Turns recall into
  recognition and makes the failure visible before it happens.
- **(c) Flag unknown tokens** on commit: if the committed template still contains `{{`…`}}`
  after substitution of the known set, show a warning caption naming the unrecognized token.

---

###### R10 — Model downloads are committed to blind

**Evidence.** `models_ui.rs:402-406` — an `Available` row renders exactly the size name and a
Download button. The byte size is shown **only** for rows already downloaded
(`:393-394`, `format_bytes`). There is no expected-size metadata in the registry
(`crates/humm-core/src/models/registry.rs` has no size table), no disk-space check, and no
confirm. `docs/gui-manual/M8.md` §8.2 records the real magnitudes in passing: Tiny 74.1 MB,
Small 465.0 MB, and a live download of **~2.9 GB**.

**Why it matters.** *Meaningful choice* requires the cost of each option to be visible at the
moment of choosing. A user on a metered connection or a nearly-full SSD clicks "Download" on
Large-v3 with no idea they have started a 2.9 GB transfer. The Cancel button for that
transfer was itself unreachable until M8 (`docs/gui-manual/M8.md` §8.2 — "a started model
download — up to 1.5 GB — cannot be stopped from the UI at all"); it is fixed now, but the
episode shows how costly an uninformed commitment here is.

**Alternatives:**
- **(a)** Ship a static size table in the registry (these are fixed, well-known ggml
  artifacts) and render it on every row, downloaded or not — the Models tab layout already
  has the slot.
- **(b)** Issue a `HEAD` before starting and show *"Large-v3 — 2.9 GB. Download?"* as a
  confirm, with a free-space warning when the target volume has less than 2× the size free.
- **(c)** At minimum, show the size in the *progress* row ("142 MB of 2.9 GB — 4 min left")
  rather than a bare percentage, so the user can bail early with the now-reachable Cancel.

---

###### R11 — The accessibility font-size control breaks the layouts it is supposed to help

**Evidence.** Preferences → General offers a custom font size of 8–32 pt that scales every
text style live (`views/prefs_general.rs:230-271`, `apply_font_size` /
`scale_text_styles`). But the main task table's row height is a hardcoded constant:
`body.rows(20.0, total_rows, …)` (`views/task_table.rs:320`). The transcript viewer's table,
by contrast, derives its height from the live line height
(`views/viewer_table.rs:72`: `let row_height = line_height * 4.0;`) — so the correct pattern
exists in the codebase and was simply not applied to the main table. Separately,
`UX_SPEC.md` §J-11 records: "at `ui.font_size = 24` the six-tab strip overflows the 650 pt
minimum width (Privacy off-screen, no wrap or scroll)" — logged as an M8 fix candidate and
not fixed.

**Why it matters.** WCAG 1.4.4 (Resize Text) expects content to remain usable when text is
enlarged. Here, a low-vision user who sets 24 pt gets clipped rows in the app's primary table
and loses access to the Privacy tab entirely — the tab containing the telemetry opt-out.
An accessibility feature that degrades the app at the settings a user with a disability would
actually choose is worse than not shipping it, because the user has no way to know the
truncation is a layout bug rather than their data.

**Alternatives:**
- **(a)** Derive the task-table row height from `ui.text_style_height(&TextStyle::Body)` with
  padding — literally the `viewer_table.rs:72` pattern.
- **(b)** Make the Preferences tab strip wrap (`horizontal_wrapped`) or scroll, and raise the
  window's minimum width proportionally to the active font size.
- **(c)** Add a quick visual check to the existing kittest suite: render Preferences and the
  main window at font sizes 8, 16 and 32 and assert no widget rect exceeds the viewport —
  the geometry-assertion technique the project already used to pin the download Cancel button
  (`rect.right() <= 650.0`).

---

###### R12 — Opening a file with humm already running dead-ends

**Evidence.** `crates/humm-gui/src/main.rs:32-52`. `import_paths_from_args` collects the argv
paths first, then `acquire_gui_lock` returns `Ok(None)` for a second launch, which calls
`show_already_running_dialog()` — an rfd modal reading *"humm is already running."* with a
single OK button — and returns. The collected paths are dropped on the floor. The running
instance is neither told about the files nor brought to the front.

**Why it matters.** macOS treats "open this file with that app" as a first-class gesture
(double-click, Open With, drag-to-Dock-icon, `open -a`). The comment at `main.rs:29-31`
explicitly claims "Finder 'open with' parity", but that parity holds only when humm is *not*
already running — i.e. the less common case. The user's model is "I told humm to open this
file"; the system's response is a modal informing them of an implementation detail, with no
verb offered. Nielsen #9 again: an error message that names no recovery.

**Alternative.** Have the second process write the paths into a handoff file next to the
existing lock (`~/Library/Application Support/humm/instance.lock` is already the
coordination point) and send an activation request; the running instance polls/watches it,
raises its window, and opens the options sheet with those files — reusing
`HummApp::drain_pending_argv_import`, which already exists for exactly this payload shape.
If that is out of scope, at minimum change the dialog to *"humm is already running — bring it
to the front?"* with a real button, and say that the files were not opened.

---

###### R13 — The language picker is a flat ~99-item list, in English, in every locale

**Evidence.** `views/options_sheet.rs:815-846`, `render_language_combo`: a `ComboBox`
containing "Detect Language", an optional favorites block, then
`Language::all()` sorted by `english_name()` — and every entry, plus the combo's own selected
text, is rendered with `language.english_name()` (`:834`, `:842`, `:823`). The favorites
block is populated *only* from an environment variable,
`HUMM_FAVORITE_LANGUAGES` (`:63-65`) — there is no UI to set it.

**Why it matters.** *Hick's Law*: a flat ~99-item unfiltered list is a measurable cost on a
control most users touch on every import. And the localization gap is pointed: a Japanese
user running humm in Japanese picks their source language from a list that reads
"Japanese", not "日本語" — while the surrounding UI is fully translated. The favorites
mechanism that would fix the length problem exists but is reachable only by someone who
reads `docs/HUMM_SURFACE.md` §5 and edits their shell profile.

**Alternatives:**
- **(a)** Add type-to-filter to the combo (egui supports a text field inside a combo popup);
  ~99 items with a filter is a non-issue.
- **(b)** Surface favorites in the UI: a "★" toggle beside each entry, with starred languages
  pinned at the top, persisted in `settings.toml`. The env var stays honored and wins, the
  same precedence pattern `HUMM_UPLOAD_URL` already uses.
- **(c)** Add `native_name()` alongside `english_name()` and render *"日本語 — Japanese"*, or
  render the endonym when the UI locale matches. This also happens to be the most-requested
  shape in comparable tools.

---

###### R14 — Speaker-ID Step 2 gives you nothing to recognize a speaker by, and "Save" doesn't say what it saves

**Evidence.** `views/speaker_id.rs:126-135` — each detected speaker is one row: an editable
name field (`TextEdit::singleline(name).desired_width(200.0)`) and a
`speaker-id-play-sample` button. No segment count, no total speaking time, no text preview,
no position in the timeline. The sample plays a *random* segment for up to
`min(10s, segment length)` (`UX_SPEC.md` §J-7). The merge control is labelled
`speaker-id-merge-checkbox = Merge speaker sentences` (`humm.ftl:495`), defaulted on, with
no explanation of what merging does. Save (`:157-164`) is labelled `Save` and in fact
**creates a new transcript** while leaving the source untouched (`UX_SPEC.md` §J-7) — which
is nowhere stated in the UI.

**Why it matters.** The task is "which of these voices is Baiba?", and the interface answers
it with an audio-only, randomly-sampled probe. That is *recall* under time pressure, with no
recognition cues, and it excludes Deaf and hard-of-hearing users from the task entirely. And
the good news — that Save is non-destructive — is invisible, so a cautious user hesitates
over a button that is actually safe, while the word "Save" primes the opposite expectation
(that their existing transcript is about to be overwritten).

**Alternatives:**
- **(a)** Put recognition cues in the row: *"Speaker 0 — 34 segments · 6m 12s · "so if we
  look at the second quarter…""*. All of it is already in the mapped result; it needs
  rendering, not computing. This makes the task solvable without audio at all.
- **(b)** Relabel `Save` to `Save as new transcript` and add a one-line caption under the
  step-2 group: *"Your original transcript is not changed."* Two strings.
- **(c)** Give the merge checkbox a caption describing the effect
  (*"Combine consecutive segments from the same speaker into one paragraph"*), in the same
  style as the ✚ disposition caption already added to the Folder Watch tab
  (`prefs-fw-disposition-note`, `humm.ftl:340`) — the precedent for explaining an inherited,
  opaque control is set.

---

###### R15 — Shipped typos and hover-only information in the live-recording window

**Evidence.** `humm.ftl:505`:
`recording-model-load-note = First time use of a model may take up to several minutest to
load.` — "minutest". The catalog header states these are "byte-exact ports" of Buzz strings,
so the typo was inherited deliberately. It is delivered as an `on_hover_text` on a bare ⓘ
glyph rendered with `ui.label("\u{24d8}")` (`views/recording.rs:215-218`), and only on Apple
Silicon.

**Why it matters.** Two distinct problems. (1) Byte-exact parity is the right rule for export
formats — it is the wrong rule for prose, and here it has shipped a visible defect into
fifteen catalogs. (2) A non-focusable `ui.label` with hover-only text is unreachable by
keyboard, unreachable by screen reader, and invisible to anyone who doesn't happen to rest
the pointer on a 14-pixel glyph — for a message explaining why the app appears to hang for
several minutes on first use. That is exactly the information that should not be hidden.

**Alternative.** Fix the typo (parity is about behavior; ADR it as a deliberate copy
correction if the byte-exactness rule needs formal amendment), and promote the note from a
hover tooltip to an inline caption shown whenever the selected model is not yet downloaded —
which is precisely when it is true and needed. Better still, fold it into R6(a)'s first-run
model note so the message appears once, where the decision is made.

---

###### R16 — Presentation colours have no contrast guard, and a hidden file silently overrides them

**Evidence.** `views/presentation.rs:639-641, 132-134` — the Custom theme takes arbitrary
`color` and `background-color` hex values with validation for *syntax* only
(`valid_hex_color` rejects `#fff`, `red`, non-hex — but accepts `#fefefe` on `#ffffff`).
There is no contrast check. Separately, an optional
`~/Library/Application Support/humm/presentation_style.css` is parsed and applied
(`load_css_override`, `:253`; `docs/HUMM_SURFACE.md` §6), unrecognized declarations ignored
with a single log line (`:220`).

**Why it matters.** The presentation window is the **audience-facing** surface — live
captions on a projector, frequently serving Deaf and hard-of-hearing attendees. It is the
one screen in the product where a contrast failure has a direct accessibility consequence for
someone who is not the operator, and it is the one screen with unconstrained colour choice.
The CSS override compounds it: a file the user may have written months ago silently wins over
the colour buttons in front of them, producing "I changed the colour and nothing happened".

**Alternatives:**
- **(a)** Compute the WCAG contrast ratio of the chosen pair live and show an inline caption
  when it falls below 4.5:1 (*"Low contrast — hard to read from the back of a room"*), with a
  one-click "Fix" that darkens/lightens to the nearest passing value. Warn, don't block —
  consistent with the shortcut-conflict philosophy this project already chose.
- **(b)** Offer two presets designed for projection (high-contrast light, high-contrast dark)
  above the Custom option, so the good choice is the default path.
- **(c)** When a CSS override is active, show a small "Styled by presentation_style.css"
  caption with a reveal-file button in the presentation bar, so the override is visible
  rather than mysterious.

---

###### R17 — Ignored shortcuts fail silently

**Evidence.** `handle_clear_selected` (`app.rs:1392-1396`) opens the confirm modal only when
the terminal-status gate passes; the doc comment states "A selection that fails the gate is
silently ignored on every path, including the Delete key". `apply_clear_confirmed`
(`:1317-1320`) repeats the silent abort, explicitly "no toast". `handle_global_shortcuts`
(`views/main_window.rs:427-430`) makes ⌘U a no-op when yt-dlp is absent.

**Why it matters.** Lower severity than the rest, but it is the same lens as R7: a user who
presses Delete on a mixed selection gets no response at all and cannot tell whether the
keystroke was received. Toolbar buttons grey out with tooltips — the keyboard equivalents
produce nothing. Norman: the *signifier* exists on one input path and not the other.

**Alternative.** Emit the disabled-state tooltip text as a short info toast when a gated
shortcut is pressed and rejected: *"Select completed transcriptions to clear."* /
*"yt-dlp missing — install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP"* (the second
string already exists, `humm.ftl:148`). One shared helper, five call sites.

---

##### 4. Accessibility

Assessed against WCAG 2.2 POUR. humm has never been through an accessibility pass; nothing
below is a surprise given that, and none of it is expensive to fix.

**Perceivable**
- **Colour independence: passes.** Every colour-carried meaning I found also carries text.
  The shortcut conflict is an amber caption *and* the words "Conflicts with <other>"
  (`views/prefs_shortcuts.rs:99`, `humm.ftl:322`); errors are warn-coloured labels whose text
  is the message (`views/speaker_id.rs:87`, `views/recording.rs:276`, `views/viewer.rs:245`).
  Status is text, not a coloured dot. This is a genuine strength — protect it.
- **Contrast: two concrete failures.**
  (i) `views/url_dialog.rs:93` uses a hardcoded `egui::Color32::RED` for the invalid-URL
  message. Pure `#FF0000` on a light-theme near-white panel is ≈**4.0:1**, below the 4.5:1
  AA floor for normal text, and it ignores the theme entirely.
  *Fix:* use `ui.visuals().error_fg_color`, as every other error site in the codebase does.
  (ii) `CONFLICT_COLOR = #CC8A00` (`views/prefs_shortcuts.rs:99`) measures ≈**2.9:1** against
  white — well below AA — while passing comfortably in dark mode. *Fix:* darken for the light
  theme, or derive from `ui.visuals().warn_fg_color` like the rest of the app.
- **Text resize (1.4.4): fails at the sizes that matter** — see R11 (clipped table rows at
  large sizes; the Privacy tab falls off the Preferences strip at 24 pt).
- **Captions/subtitles:** not applicable as a consumption concern — humm *produces* SRT/VTT.
  Worth noting the product's own output is an accessibility artifact, which is a nice
  alignment to lean on in positioning.

**Operable**
- **Keyboard (2.1.1): fails on the primary surface** — R2. The task table cannot be reached
  or selected from the keyboard, which transitively blocks Open, Cancel, Clear, Rename,
  Notes and Restart. This is the single most important accessibility fix in the product.
- **Remappable input: strong.** All 20 shortcuts are rebindable with live re-apply across
  windows and a Reset to Defaults (`views/prefs_shortcuts.rs`, `UX_SPEC.md` §3). Esc is
  correctly reserved and never bindable. This is better than most commercial desktop apps.
  The weakness is the *defaults*, not the mechanism (R5).
- **Focus visibility:** egui's default focus ring applies to real widgets, but the table's
  cells are `ui.interact` rectangles over plain labels (`views/task_table.rs:459-464`), so
  there is no focusable element to ring. Fixing R2 fixes this.
- **Hover-only information:** several controls carry meaning only in tooltips — the ⓘ
  model-load note (R15), the folder-watch pause icon button (`views/main_window.rs:586-591`),
  the speed badge (`models_ui.rs:388-390`). Tooltips are not keyboard- or screen-reader-
  reachable in this build. *Fix:* pair each icon-only control with an accessible name, and
  promote genuinely load-bearing tooltips to inline captions.
- **Unlabelled icon-only control:** the store-health banner's dismiss button is
  `ui.button("\u{00d7}")` with no tooltip and no label (`views/main_window.rs:601`). One
  `.on_hover_text(tr("dismiss"))` plus a catalog key.
- **Reduced motion: not supported, and barely needed.** The animated surfaces are the
  indeterminate progress bars (`ProgressBar::new(0.0).animate(true)`), the ~60 Hz playback
  repaint (`views/viewer.rs:48`), the audio meter's decay smoothing
  (`views/audio_meter.rs:75`), and toast appearance. None are large-area parallax or
  flashing; nothing here is a 2.3.1 seizure risk. There is no `prefers-reduced-motion` hook
  and no in-app toggle. *Recommendation:* low priority — add a single "Reduce animation"
  checkbox to Preferences → General that stills the indeterminate bars and drops the
  non-playback repaint cadence, if and when a user asks.

**Understandable**
- **Reading level is mixed.** Good, plain copy exists: the Folder Watch disposition note
  (*"Once transcribed, the source file is moved into the output folder — or deleted, if
  Delete processed files is checked."*), the Post-processing header (*"These hooks run
  automatically after each transcription completes. A hook failure is logged but never fails
  the transcription itself."*), and the whole Privacy tab. Against that, unexplained jargon
  is shipped to the front of the core loop: `Word-level timings`, `Extract speech`,
  `Initial Prompt:`, `Hugging Face model`, `Silence threshold:`, `Transcription step:`,
  `Merge speaker sentences`, and a fourteen-entry model-size list whose names encode nothing
  a non-specialist can act on. Roughly: the strings humm *wrote* read at about a 9th-grade
  level; the strings it *inherited* assume domain knowledge.
  *Fix:* one pass adding a caption under each jargon control, in the house style the Folder
  Watch tab already established. No new mechanism, ~15 strings.
- **The meter caption reports raw RMS to four decimals** —
  `"Average volume: 0.0123  Queue: 3"` (`views/audio_meter.rs:80-88`). Neither number maps to
  anything a speaker can act on. *Fix:* replace with a plain judgement
  (*"Input level: good"* / *"too quiet — move closer or raise input gain"*), keeping the raw
  figure behind a tooltip for debugging.
- **Consistency (3.2.4):** `Ok` vs `OK` casing differs between dialogs by deliberate
  inheritance (`humm.ftl:98-104` documents the choice). Harmless, but it is inconsistency
  shipped on purpose; I'd normalize it and note the deviation.

**Robust**
- `eframe` 0.35 is used with default features, so AccessKit is compiled in and the app does
  expose an accessibility tree. However, the table's clickable regions are raw
  `ui.interact` rects with no widget semantics, so what VoiceOver receives for the app's
  central control is a grid of unlabelled hit targets next to loose text. I did not run
  VoiceOver against a build (out of scope — the repo is read-only and the Tech Lead's report
  records that the app cannot currently launch off the dev machine), so treat this as a
  **source-level concern, not a measured result**. It should be verified once a launchable
  build exists.

**If I could fund one accessibility item:** R2 (keyboard operation of the task table). It is
a Level A failure, it blocks every downstream verb, and the selection model it needs is
already written and tested.

---

##### 5. Ethics

**Monetization: none present.** There is no payment path, no licence check, no trial timer,
no paywalled feature, no account, no "upgrade to Pro" surface anywhere in the codebase. The
app is free of commercial pressure by construction.

**Retention mechanics: none present.** No streaks, no daily-use nudges, no notifications, no
badges, no gamification, no re-engagement email. The only recurring surface is the update
check (below). This is a tool that does not want your attention when you are not using it,
and that is worth saying out loud as a positive.

**Dark patterns: none found.** I looked specifically for confirmshaming, bait-and-switch,
manipulative timers, pay-to-skip-pain, roach motel (hard-to-exit flows), forced continuity,
and pre-checked consent traps. None are present. The one destructive confirm uses neutral
language (*"Are you sure you want to delete the selected transcription(s)? This action cannot
be undone."*, `humm.ftl:107`) with plainly labelled Ok/Cancel — no shaming, no colour
weighting against the safe choice.

**Two things that are not dark patterns but are disclosure gaps, and I'd fix both:**

1. **Telemetry is on by default, with no first-run notice.**
   `PrivacySettings::telemetry_enabled` defaults to `true`
   (`crates/humm-core/src/settings.rs:625-632`, and the doc comment says so: "must default to
   `true` (Buzz parity — telemetry is on unless the user or an env var turns it off)"). The
   opt-out lives on the **sixth** Preferences tab (`humm.ftl:268`) — which, per R11, can be
   off-screen at large font sizes. The mitigations are real and substantial: the payload is
   genuinely minimal and fully enumerated in the UI, the identifier is a random UUID with no
   PII, debug builds are no-ops, an absent build key compiles the whole thing away, and
   `HUMM_DISABLE_TELEMETRY` works (`crates/humm-core/src/telemetry.rs:205-214`). So this is
   an honest, well-built, minimal analytics implementation — with an opt-*out* default and no
   moment where the user is told it exists.
   *Alternative:* a one-line first-run notice in the hero empty state —
   *"humm sends one anonymous launch event. [What's in it] [Turn it off]"* — dismissed
   forever on first import. Or flip the default to off and ask once. Either satisfies
   informed consent without a consent-fatigue modal.

2. **AI features send user content to third parties on a single unannotated click.**
   The viewer's `Translate` button (`views/viewer.rs:329-335`) ships transcript text to a
   configured LLM endpoint; `Enable AI Summary` in Preferences → Post-processing runs
   **automatically after every transcription** once enabled (`humm.ftl:350`: "These hooks run
   automatically after each transcription completes"), sending the transcript to its own
   configured base URL. Neither surface names the destination host at the point of action or
   states that content leaves the machine. The only gate is the presence of an API key, and
   the only error copy is a dead end: `viewer-translate-key-required-body = Please enter
   OpenAI API Key in preferences` — which doesn't offer to take you there.
   For a tool whose whole positioning is "it runs on your machine", the boundary where that
   stops being true should be visible.
   *Alternative:* (i) name the destination inline — *"Translate · sends 412 segments to
   api.openai.com"* as the button's tooltip and as a one-line confirm the first time per
   transcript; (ii) add an "Open Preferences" button to the key-required modal; (iii) put a
   persistent caption on the AI Summary section stating that enabling it uploads every
   completed transcript. Credentials are already handled exemplarily — keyring only, never
   `settings.toml`, never logs, with redaction unit-tested — so this is about disclosure of
   *content* flow, not of secrets.

**Also worth noting as a positive:** the update mechanism is notify-only by decision
(Q-7/ADR-015 — "version check + notify with release link; no self-install in v1"). No silent
background installs, no nag-on-every-launch. The user keeps control.

---

##### 6. Open questions for the founder

1. **Who is the intended user — a researcher/journalist/editor, or an engineer?** Almost
   every finding above (R6, R9, R10, R13, the reading-level note) resolves differently
   depending on the answer. Today the product's defaults and vocabulary say "engineer" while
   its polish and locale coverage say "everyone". I can write the onboarding and copy pass
   either way, but not both.

2. **Is byte-exact string parity with Buzz still a requirement, or was it scaffolding for
   the port?** It currently blocks translating the Status column for fourteen locales (R4)
   and has shipped a typo into fifteen catalogs (R15). If the answer is "it was
   scaffolding", I'd like to propose an ADR amendment: byte-exact for *export formats and
   file contents*, freely rewritten for *UI prose*.

3. **Should humm ever be operable without a mouse?** R2 is a Level A accessibility failure
   and also a power-user complaint waiting to happen. It is a contained piece of work, but
   it is work, and it needs a yes.

4. **Which single moment do you want to feel best?** My recommendation is the *completion*
   moment — it is currently the weakest point in the loop (an anonymous count toast, R3) and
   the one the Peak-End rule says users will remember. If you agree, R3 + R7 + R1(b) are one
   coherent piece of work: "the transcription finished, here's where it is, and you can undo
   anything you do to it next."

5. **Is the live-recording + presentation path a real use case you want to invest in, or a
   parity obligation?** It is the most feature-dense area of the product (device picker,
   three display modes, silence thresholds, export, upload URL, presentation theming, CSS
   override) and simultaneously the least onboarded (R6's missing realtime warning, R16's
   contrast). If it matters, it deserves its own design pass. If it is parity ballast, we
   should stop polishing it.

6. **Does the "local-first" framing matter to you commercially?** If yes, the AI/translation
   disclosure work in §5 is not optional hygiene, it is brand protection — and I'd also
   recommend making the local/cloud distinction visible in the options sheet's summary line
   (which currently gets it actively wrong, R8).

7. **Is there an appetite for a visual design pass at all?** This review is deliberately
   about behavior, because humm currently has no visual identity beyond egui's defaults —
   no colour system, no type scale, no spacing rhythm, no icons, and no `.app` icon. That is
   a legitimate choice for a utility. If it is ever meant to be *sold* rather than *used*,
   it is the next conversation after the ones above.

---

##### 7. Deliberately out of scope

Per my remit, I have not assessed engineering feasibility, implementation cost, architecture,
or build/packaging — those belong to the Tech Lead ([DIG-14](DIG-14.md)). Where I
have suggested an alternative, I have tried to point at an existing pattern in the codebase
so the Tech Lead can cost it quickly, but the cost judgement is theirs. I have also not
verified any finding in a running build: the repository was read-only for this task, and the
Tech Lead's report records that the GUI cannot currently launch off the original dev machine
(ADR-024 / OpenBLAS). Every claim above is traceable to source, shipped copy, or the
project's own windowed acceptance record, and I have flagged the one place where I am
reasoning from source rather than observation (the AccessKit note in §4).


## Comments (1)

<a id="comment-934b2a40-e3ec-4a20-bb39-b4c91fbeb20d"></a>

### 2026-09-25T14:27:46.802Z — GameExperienceDesigner

Comment id `934b2a40-e3ec-4a20-bb39-b4c91fbeb20d`; written by run `d1155f20-4fad-4d90-b17f-9e3973d7ad8a`.

Design review of **humm** complete — see the `review` document on this issue.

DEEP pass: read the actual GUI source (`crates/humm-gui/src/**`), the shipped English string catalog, `docs/design/UX_SPEC.md`, and the windowed acceptance record `docs/gui-manual/M8.md`. Repository untouched — nothing built, run, or modified.

**Top 5 risks** (all 17 have evidence + at least one concrete alternative):

1. **No undo anywhere**, while the transcript editor autosaves every committed edit — a stray keystroke or nudge is permanent.
2. **The task table is pointer-only** — no keyboard path to selection, which transitively blocks Open / Cancel / Clear / Rename / Restart. WCAG 2.1.1 Level A failure on the primary surface.
3. **"Where did my file go?"** — no destination control in the GUI, and the completion toast reports a count, not a path. For URL imports the exports land inside `~/Library/Application Support/humm/media/<uuid>/`.
4. **15 locales shipped, Status column hardcoded English** — byte-exact-parity rule applied to user-facing prose (`format.rs:102-125`).
5. **⌘S = Clear History** (destructive) and ⌘X = Cancel, shipped as defaults on macOS.

Also covered: accessibility (two measured contrast failures, text-resize breakage at the sizes low-vision users actually pick, remappable-input assessment, reduced-motion) and ethics — **no monetization, no retention mechanics, no dark patterns found**, but two disclosure gaps worth fixing (telemetry on by default with no first-run notice; AI translation/summary send content to third parties with no destination named).

Seven open questions for the founder are at the end of the document; the most load-bearing is #1 (who the intended user is), since most copy and onboarding findings resolve differently depending on the answer.

Per the task, findings are **not** posted to [DIG-7](DIG-7.md) — the roll-up happens there. No review needed on this issue, so marking `done`.


## Agent runs (2)

Every Paperclip heartbeat run recorded against this issue. A run's results are the comments, documents, attachments and commits it wrote (comment headers above name the run). Raw run transcripts are not archived, except the runs listed with a transcript copy below; see [inventory.md](../inventory.md#exclusions).

| Run | Agent | Status | Started | Finished | Log bytes | Transcript copy |
|---|---|---|---|---|---|---|
| <a id="run-fa793dbc-dcfa-408c-8192-383ccfd8ef83"></a>`fa793dbc-dcfa-408c-8192-383ccfd8ef83` | GameExperienceDesigner | succeeded | 2026-09-25T14:14:16.736Z | 2026-09-25T14:20:56.694Z | 265172 | — |
| <a id="run-d1155f20-4fad-4d90-b17f-9e3973d7ad8a"></a>`d1155f20-4fad-4d90-b17f-9e3973d7ad8a` | GameExperienceDesigner | succeeded | 2026-09-25T14:16:47.750Z | 2026-09-25T14:28:03.859Z | 249864 | — |

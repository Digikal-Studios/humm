# Buzz `main` gaps after v1.4.5

Compared Buzz `origin/main` `298dbf003505cf945bdfd1eb8e705348b59a5b34` with every commit after `aaf98634cd354be207e00cc5aaa1bfbd9e19cf22` (v1.4.5, "Prevent system sleep while transcriptions are queued"). Buzz was read from a shallow clone of `https://github.com/chidiwilliams/buzz` after `git fetch origin main`. It is not vendored.

**humm baseline (this doc’s tip):** `171c72eb2bd80ea2db313cd6d423e63c73cfbd90` (`origin/main`, includes PRs #3–#9). Earlier drafts of this file cited `1bbe893` (post-#4) and the `cursor/buzz-main-parity-f2bf` branch; behaviors marked **implemented** below landed via that branch as PR #5 (`acff1d6`) and are on `main` now.

**Production carve-outs (public):** NeMo MSDD/Sortformer/`--speaker-count`, FunASR, transformers/Parakeet/MMS/Faster-Whisper, and multi-custom-models are **named exceptions**, not silent gaps — see the README “Named exceptions” table. A row is implemented only when the difference is a behavior a test can show.

## Speaker identification

Buzz `8d0812a243afeda5201c416b6e3c680af18fc88e` (`buzz/cli.py`) adds `--identify-speakers`, `--speaker-count`, and `--speaker-diarizer` (`msdd` | `sortformer`). Any of the three turns identification on. `--speaker-count` must be an integer. The diarizer string defaults to `msdd`. `buzz/transcriber/speaker_identifier.py` passes `num_speakers` only when `diarizer == "msdd"` (around line 100). `buzz/widgets/transcription_viewer/speaker_identification_widget.py` runs NeMo: `_SortformerDiarizer.diarize(waveform)` or `_MSDDDiarizer.diarize(waveform, num_speakers=...)` (around lines 345–355). Folder watch stores the same three fields plus `merge_speaker_sentences` (`221659720db809da441d62e2cd031bbd5a174f42`).

humm already runs `--identify-speakers` on decoded PCM through `OrtDiarizer` (`speakrs` 0.5.0, ADR-024). `OwnedDiarizationPipeline::run` takes `&[f32]`. `PipelineConfig` has binarize, AHC (`AhcConfig.threshold` only), and VBx settings. There is no speaker-count field. Passing `--speaker-count 4` or `--speaker-diarizer sortformer` into that call would ignore the value. Those flags stay a usage error (`humm-cli` `validate_add_args`).

**Blocked, not faked:**

- **MSDD and Sortformer** need NeMo (`speaker_identification_widget.py`). That is a new native engine. ADR-011 and ADR-024 keep diarization on the `Diarizer` trait and `speakrs`. No new non-Rust dependency was added.
- **`--speaker-count`** is an MSDD argument (`num_speakers` on `_MSDDDiarizer.diarize`). `speakrs` 0.5.0 cannot apply it. Post-hoc merging to N speakers would be a different algorithm.
- **FunASR** (`851c8b359343db8c9460aa15ee59a40f2ab66aa8`) is `is_fun_asr_model` in `buzz/model_loader.py` and the FunASR transcription path in `buzz/transformers_whisper.py`. ADR-008 and `docs/OPEN_QUESTIONS.md` Q-2 name it as out of scope. No FunASR engine, in-process Python, or `transformers` dependency was added.
- **Folder-watch speaker controls** (`2216597`, `folder_watch_preferences.py`) select MSDD or Sortformer and an optional count. The GUI task runner's diarizer is unset (`services.rs` `diarizer: None`); identification in the GUI is the separate Speaker-ID window. A checkbox that ran `OrtDiarizer` while the stored choice said `msdd` would drop the algorithm the user picked. The controls were not added.

**Implemented on the path that already runs:** `identify_speakers` returns the original segments when identification fails, when the result has no speakers, and immediately when there are no segments (`speaker_identifier.py`). `identify_with_diarizer` does the same: a diarizer `Err` other than cancellation, and `Ok` with an empty turn list, both return the input segments unchanged (no `Speaker 0:` prefix, no merge, translations kept). Cancellation still fails the task.

## Commit status

| Commit | Behavior | humm status |
| --- | --- | --- |
| `35c381c` Python upgrade (#1594) | Python 3 / lockfile / CI toolchain bump. | Packaging. No humm behavior. |
| `56f1d49` m4b, m4v, aac, and folder drag-and-drop (#1595) | Extensions, plus `dropEvent` replaces a directory with the media files inside it and keeps a dropped file as-is. | Extensions already on `main`. **Implemented:** `expand_dropped_paths`, used by `drain_dropped_files`. |
| `9775971` French translation (#1598) | `fr` gettext catalog. | **Implemented (HUM-28):** Fluent catalog `assets/i18n/fr/humm.ftl`, registry code `fr`, Buzz order after `nl`. Buzz `.po` strings used where msgids match; humm-only keys may still be English inside the catalog. |
| `4c18d46` URL trailing-dot titles on Windows (#1602) | Sanitize titles that end in `.` so Windows can store the download. | Already on `main` (`safe_filename_component`). |
| `98b7993` Parakeet / MP4/M4A (#1604) | Hugging Face transformers backend decodes MP4/M4A for Parakeet. | **Named exception.** ADR-008 does not ship the transformers backend. |
| `2af05d4` Multi-file CLI (#1609) | `add` processes every positional file. | Already on `main` (`humm-cli` `run.rs` loops `args.files`). |
| `710870d` demucs RAM (#1611) | In-process demucs `Separator` streams fixed windows instead of holding the whole mix. | Not ported. humm runs `demucs` once per file (ADR-023). The windowed separator is inside Buzz's Python API, not a flag on that subprocess. |
| `f70a02f` Multiple custom Whisper.cpp models (#1614) | JSON list of custom ggml models, legacy id `legacy-custom`. | **Named exception** (ledger P-008 / ADR-008). humm keeps one `ggml-custom.bin` slot. Renaming the cache file would orphan downloads. |
| `2d7d9dd` Per-segment plugin review reasons (#1600) | Plugins attach a review reason stored on the segment. | Not ported. ADR-010 ships the built-in hooks as native features and does not carry the plugin review-reason field. |
| `2216597` Folder watch and speaker identification (#1617) | Folder-watch identify-speakers option; `identify_speakers` helper; removal of the DeepFilterNet plugin package (the feature moved). | Speaker engines blocked (section above). DeepFilterNet is already `humm-dfn` (F-112). Failure-keeps-transcript behavior of `identify_speakers`, including a successful result with no speakers, is **implemented**. |
| `7d1929c` Unbundle CUDA (#1439) | Optional CUDA download instead of a bundled toolkit. | Packaging for Buzz's PyTorch/whisper stack. humm's local backend is whisper-rs (ADR-008). |
| `81fdc26` Orukeet on the HF backend (#1623) | Documents Orukeet as an existing Hugging Face ggml repo. | No new engine. A ggml repo id still goes through `resolve_hf_repo` (ADR-008). |
| `a61bc18` Application close shortcut (#1624) | Qt event filter closes the active window on the platform Close key (Cmd+W / Ctrl+W) because Qt does not bind it. | Not ported. humm closes windows through the viewport close path. No test showed a humm window ignoring that key. |
| `d1c5646` Stream download progress (#1625) | Progress when the total size is missing must not crash. | Already on `main` (`humm-media` `fetch.rs` / queue download progress). |
| `1cc58f4` Speaker-identification playback (#1627) | Re-initialize PortAudio when the default output device changes, and skip that while a stream is open. | Not ported. humm plays samples with cpal (`humm-audio`). The workaround is specific to sounddevice's cached PortAudio device list. |
| `4e3eb63` Recording transcriber (#1629) | Stop passing `--host 0.0.0.0` to whisper-server. | Not applicable. humm does not spawn whisper-server; live transcription is in-process whisper-rs. |
| `838a191` GPU support instructions (#1634) | README wording. | Docs for Buzz's CUDA build. No humm behavior. |
| `8d0812a` Speaker-identification CLI (#1640) | `--identify-speakers`, `--speaker-count`, `--speaker-diarizer`. | Bare `--identify-speakers` already runs `OrtDiarizer`. Count and diarizer stay rejected (section above). |
| `100589f` One speaker-identification dialog (#1641) | A second Identify click brings the running dialog forward instead of starting another pipeline. | Partial. One window refuses a second run while `Running` (`SpeakerIdVm::can_identify`). There is no process-wide "raise the other transcript's dialog". Not extended in this change; the GUI test harness does not drive two real speaker windows. |
| `5082ca4` PyPI demucs (#1650) | Drop the vendored demucs submodule and import the PyPI package. | Already the humm shape: `demucs` on `PATH` (ADR-023). |
| `851c8b3` FunASR (#1651) | Hugging Face FunASR models. | **Named exception** (README + ADR-008 / Q-2). |
| `d05a48a` Unreadable folder-watch path (#1653) | A media path whose metadata cannot be read must not crash the watcher. | Already on `main` (`folderwatch.rs` drops a failed `metadata`). |
| `c02e63f` `/SUPPRESSMSGBOXES` (#1654) | Inno Setup flag. | Windows installer. No humm behavior. |
| `c6c0a69` Merge-by-gap in the resizer (#1658) | Gap merge was dividing milliseconds by 100. | Already on `main`. humm's resizer does not do that division. |
| `f1b943c` XDG secret-portal freeze (#1663) | Reading the secret service through the portal blocked the UI for about five seconds. | Not ported as a code change. humm uses the `keyring` crate. The freeze is in Buzz's Qt/portal secret read. |
| `053b388` Skip names with square brackets (#1659) | `glob.escape` the stem, and run `safe_filename_component` so `what?` matches the exported `what_`. | Brackets were already literal (`starts_with`, covered by a test). **Implemented:** the result-file search sanitizes the stem before matching. |
| `6186d09` Short timestamp fractions (#1660) | `00:00:05.5` is 5500 ms; longer fractions keep three digits. | Already on `main` (`viewer::timestamp::parse_timestamp`). |
| `23d848b` Download URL audio before speech extraction (#1667) | URL tasks download before speech extraction and plugins, and a late download signal after cancel is ignored. | Order already on `main`: `Runner::run_job` fetches the URL before skip and isolation. The Qt thread handoff was not re-created. |
| `ee01182` CUDA prompt on a deleted window (#1668) | Do not show the CUDA dialog if the main window is gone. | Buzz CUDA setup UI. No humm behavior. |
| `efd6e2d` Lazy ML imports (#1669) | Import torch/transformers on first use so startup is faster. | Python startup. humm does not import those libraries. |
| `95c91e4` Turkish translation (#1670) | `tr_TR` gettext catalog. | **Implemented (HUM-28):** Fluent catalog `assets/i18n/tr/humm.ftl`, registry code `tr_TR`, Buzz order after `ru`. Same provenance rule as French. |
| `bc2882b` Skip imports (#1672) | URL tasks match a prior transcript by stored URL, not by the temp download name. Also copies a non-ANSI Windows path to an ASCII temp file before calling whisper-cli. | URL skip already on `main`. The whisper-cli path copy is for Windows ANSI `argv`. humm opens files through whisper-rs, not whisper-cli. |
| `40f6a78` Folder-watch duplicates on network shares (#1673) | Queue a file when first seen; transcribe only after size and mtime are unchanged for 30 seconds; a disappeared file is dropped. | Already on `main` (`SettleSet`, `FOLDER_WATCH_SETTLE`). |
| `eadf4ec` Malformed UTF-8 transcripts (#1674) | Strip a trailing U+FFFD, and drop whisper-cli JSON fragments that are empty, reversed, or a ≤150 ms stub whose start equals the next chunk. | Trailing U+FFFD already on `main` (`segment_text` / word flush). The chunk filter reads whisper-cli's JSON list. humm's segments come from whisper-rs callbacks, which do not produce that JSON, so the filter was not copied onto the callback path. |
| `298dbf0` Preference window size (#1677) | Qt preferences dialog minimum height. | Already checked on `main`. humm's preferences window is egui and does not use that Qt minimum. |

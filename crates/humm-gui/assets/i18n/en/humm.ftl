## humm-gui — English (en) catalog.
##
## Every user-visible string in humm-gui is keyed here and resolved through
## `i18n::tr`/`i18n::tr_args`. Keys are kebab-case, grouped by the view that
## renders them.
##
## This is the source catalog: `en` is `AVAILABLE_LOCALES[0]`, every
## negotiation's final fallback, and the key set every other catalog is
## tested against (`i18n::tests::every_locale_has_exactly_the_en_key_set`,
## both directions). Fourteen sibling catalogs are embedded alongside it as
## of F-008b/M8 Task 15 -- `../ca`, `../da`, `../de`, `../es`, `../it`,
## `../ja`, `../lv`, `../nl`, `../pl`, `../pt-BR`, `../ru`, `../uk`,
## `../zh-CN`, `../zh-TW` -- all 15 of Buzz's own `ui_locales` codes are now
## ported, so **adding or renaming a key here is not a local edit**: it must
## land in all fifteen catalogs in the same commit, or the completeness
## tests fail.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = English
locale-name-ca-es = Catalan
locale-name-da-dk = Danish
locale-name-nl = Dutch
locale-name-de-de = German
locale-name-es-es = Spanish
locale-name-it-it = Italian
locale-name-ja-jp = Japanese
locale-name-lv-lv = Latvian
locale-name-pl-pl = Polish
locale-name-pt-br = Portuguese (Brazil)
locale-name-ru = Russian
locale-name-uk-ua = Ukrainian
locale-name-zh-cn = Chinese (Simplified)
locale-name-zh-tw = Chinese (Traditional)

## Empty-state hero (views/main_window.rs)

hero-drop = Drop media here, or ⌘O
hero-import-button = Import files…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = File Name / URL
table-col-model = Model
table-col-task = Task
table-col-status = Status
table-col-date-completed = Date Completed
table-col-date-added = Date Added
table-col-notes = Notes

## Task table — header/row context menus

table-menu-reset-column-order = Reset Column Order
table-menu-restart = Restart Transcription
table-menu-rename = Rename
table-menu-notes = Add/Edit Notes

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transcribe
task-translate = Translate to English

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = isolating speech

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Record
toolbar-new-file = New File Transcription
toolbar-new-url = New URL Transcription
toolbar-open-transcript = Open Transcript
toolbar-open-transcript-tooltip = Select one or more completed transcriptions to open
toolbar-cancel = Cancel Transcription
toolbar-clear = Clear History

## Menu bar (views/main_window.rs, F-004)

menu-file = File
menu-file-import-file = Import File…
menu-file-import-url = Import URL…
menu-file-import-folder = Import Folder…
menu-help-title = Help
menu-help-about = About humm
menu-help-docs = Help
menu-help-docs-tooltip = Documentation arrives with the first release
menu-help-preferences = Preferences…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Clear History
clear-confirm-body = Are you sure you want to delete the selected transcription(s)? This action cannot be undone.
clear-confirm-ok = Ok
clear-confirm-cancel = Cancel
clear-error = Could not delete the selected transcription(s)

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Rename Transcription
rename-label = Enter new name:
rename-error = Could not rename
notes-title = Notes
notes-label = Enter some relevant notes for this transcription:
notes-error = Could not save notes
dialog-ok = OK
dialog-cancel = Cancel

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transcription complete
toast-completed-suffix = file(s) written
toast-canceled = Transcription canceled
toast-detail-title = Transcription Failed
toast-detail-open-log = Open log folder
toast-detail-retry = Retry
toast-detail-close = Close
retry-error-unavailable = Could not restart: saved options are unavailable

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm couldn't record history updates — exports still complete; see the log file

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Drop to import

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp missing — install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Import URL
url-dialog-label = URL:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = The URL you entered is invalid.
url-dialog-ok = Ok
url-dialog-cancel = Cancel

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Model:
options-task-label = Task:
options-language-label = Language:
language-detect = Detect Language
options-word-timestamps = Word-level timings
options-extract-speech = Extract speech
options-extract-speech-missing-tooltip = demucs not found — install it (e.g. `pipx install demucs`) to enable speech extraction
options-export-label = Export:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = no export selected
options-advanced = Advanced
options-initial-prompt-label = Initial Prompt:
options-hf-repo-label = Hugging Face model
options-run = Run

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Api Key:
options-llm-heading = Translation settings
options-llm-enable = Enable AI translation
options-llm-model-label = AI model:
options-llm-model-presets-button = Presets
options-llm-prompt-label = Instructions for AI:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Watching { $name }
folderwatch-pill-paused = Paused
folderwatch-pill-pause-tip = Pause folder watching
folderwatch-pill-resume-tip = Resume folder watching

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Update Available
update-dialog-title = Update Available
update-dialog-header = A new version of humm is available!
update-dialog-current-version = Current version:
update-dialog-new-version = New version:
update-dialog-release-notes = Release Notes:
update-dialog-open-release-page = Open release page
update-dialog-close = Close

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Version
about-check-button = Check for updates
about-show-logs-button = Show logs
about-close-button = Close
about-up-to-date = You're up to date!
about-check-failed = Update check failed — see the log file

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Preferences
prefs-tab-general = General
prefs-tab-models = Models
prefs-tab-shortcuts = Shortcuts
prefs-tab-folder-watch = Folder Watch
prefs-tab-postproc = Post-processing
prefs-tab-privacy = Privacy

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Ui Language
prefs-general-font-size-label = Font Size
prefs-general-font-size-custom-label = Custom font size
prefs-general-openai-key-label = OpenAI API key
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Show
prefs-general-key-hide = Hide
prefs-general-test-button = Test
prefs-general-test-title = OpenAI API Key Test
prefs-general-test-success-body = Your API key is valid. humm will use this key to perform Whisper API transcriptions and AI translations.
prefs-general-invalid-key-title = Invalid API key
prefs-general-invalid-key-body = API supports only base64 characters (A-Za-z0-9+/=_-). Other characters in API key may cause errors.
prefs-general-key-save-error-title = Error
prefs-general-key-save-error-body = Unable to save OpenAI API key to keyring
prefs-general-base-url-label = OpenAI base url
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI API model
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Default export file name
prefs-general-force-cpu-row-label = Disable GPU
prefs-general-force-cpu-label = Use only CPU and disable GPU acceleration
prefs-general-force-cpu-tooltip = Set this if larger models do not fit your GPU memory and humm crashes

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Send anonymous launch telemetry
privacy-telemetry-disabled-caption = This build has telemetry disabled at compile time.
privacy-telemetry-items-intro = Each launch event includes:
privacy-telemetry-item-app-version = App version
privacy-telemetry-item-system-name = System name
privacy-telemetry-item-os-version = OS version
privacy-telemetry-item-kernel-release = Kernel release
privacy-telemetry-item-cpu-architecture = CPU architecture
privacy-telemetry-item-system-locale = System locale
privacy-telemetry-item-identifier = A random anonymous identifier
privacy-update-checkbox = Check for updates automatically

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Downloaded
models-section-available = Available for Download
models-download-button = Download
models-delete-button = Delete
models-show-location-button = Show file location
models-cancel-button = Cancel
models-custom-url-label = Custom ggml model URL:
models-delete-confirm-title = Delete Model
models-delete-confirm-body = Are you sure you want to delete the selected model?
models-download-error-title = Error
models-download-failed = Download failed
models-speed-badge = { $ratio }× realtime
models-speed-badge-tip = Measured realtime speed from this machine's live recording sessions

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Open Record Window
shortcut-desc-import-file = Import File
shortcut-desc-import-url = Import URL
shortcut-desc-open-preferences-window = Open Preferences Window
shortcut-desc-view-transcript-text = View Transcript Text
shortcut-desc-view-transcript-translation = View Transcript Translation
shortcut-desc-view-transcript-timestamps = View Transcript Timestamps
shortcut-desc-search-transcript = Search Transcript
shortcut-desc-search-next = Go to Next Transcript Search Result
shortcut-desc-search-previous = Go to Previous Transcript Search Result
shortcut-desc-scroll-to-current-text = Scroll to Current Text
shortcut-desc-play-pause-audio = Play/Pause Audio
shortcut-desc-replay-current-segment = Replay Current Segment
shortcut-desc-toggle-playback-controls = Toggle Playback Controls
shortcut-desc-decrease-segment-start = Decrease Segment Start Time
shortcut-desc-increase-segment-start = Increase Segment Start Time
shortcut-desc-decrease-segment-end = Decrease Segment End Time
shortcut-desc-increase-segment-end = Increase Segment End Time
shortcut-desc-clear-history = Clear History
shortcut-desc-stop-transcription = Cancel Transcription
prefs-shortcuts-press = Press shortcut…
prefs-shortcuts-clear-tooltip = Reset to default
prefs-shortcuts-conflict = Conflicts with { $other }
prefs-shortcuts-reset = Reset to Defaults

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Enable folder watch
prefs-fw-input-folder-label = Input folder
prefs-fw-input-folder-placeholder = /path/to/input/folder
prefs-fw-output-folder-label = Output folder
prefs-fw-output-folder-placeholder = /path/to/output/folder
prefs-fw-browse = Browse
prefs-fw-delete-checkbox = Delete processed files
prefs-fw-disposition-note = Once transcribed, the source file is moved into the output folder — or deleted, if Delete processed files is checked.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = These hooks run automatically after each transcription completes. A hook failure is logged but never fails the transcription itself.
prefs-postproc-skip-title = Skip Already Transcribed
prefs-postproc-skip-enable = Enable Skip Already Transcribed
prefs-postproc-skip-check-files = Check for existing result files
prefs-postproc-skip-check-files-desc = Skip if a .txt, .srt, or .vtt file matching the audio filename is found next to it
prefs-postproc-skip-check-db = Check in transcription database
prefs-postproc-skip-check-db-desc = Skip if this file has already been transcribed and the record exists in the database
prefs-postproc-dfn-title = DeepFilterNet Noise Reduction
prefs-postproc-dfn-enable = Enable DeepFilterNet Noise Reduction
prefs-postproc-dfn-keep-file = Keep denoised file after transcription
prefs-postproc-langdetect-title = Enhanced language detection
prefs-postproc-langdetect-enable = Enable Enhanced language detection
prefs-postproc-langdetect-download-tiny = Download tiny model if none available
prefs-postproc-langdetect-download-tiny-desc = If no whisper.cpp model is downloaded, download the tiny model to use for language detection.
prefs-postproc-resizer-title = Automatic Transcript Resizer
prefs-postproc-resizer-enable = Enable Automatic Transcript Resizer
prefs-postproc-resizer-merge-by-gap = Merge by gap
prefs-postproc-resizer-merge-gap-seconds = Merge gap (seconds)
prefs-postproc-resizer-split-by-punctuation = Split by punctuation
prefs-postproc-resizer-punctuation = Punctuation
prefs-postproc-resizer-split-by-max-length = Split by max length
prefs-postproc-resizer-max-length = Maximum subtitle length
prefs-postproc-summary-title = AI Summary
prefs-postproc-summary-enable = Enable AI Summary
prefs-postproc-summary-api-url = API base URL
prefs-postproc-summary-api-key-label = API key
prefs-postproc-summary-model = Model
prefs-postproc-summary-prompt = Summarization prompt
prefs-postproc-summary-save-to-notes = Save summary to Notes
prefs-postproc-summary-save-to-file = Save summary to a file
prefs-postproc-summary-output-folder = Output folder (for file)
prefs-postproc-summary-key-save-error-body = Unable to save AI Summary API key to keyring
prefs-postproc-docx-title = Export to DOCX
prefs-postproc-docx-enable = Enable Export to DOCX
prefs-postproc-docx-output-folder = Output folder
prefs-postproc-docx-include-timestamps = Include timestamps
prefs-postproc-output-folder-hint = Leave empty to save next to the source file.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = View
viewer-mode-text = Text
viewer-mode-translation = Translation
viewer-mode-timestamps = Timestamps
viewer-export = Export
viewer-translate = Translate
viewer-resize = Resize
viewer-identify-speakers = Identify Speakers
viewer-find = Find
viewer-find-label = Find:
viewer-find-placeholder = Enter text to find...
viewer-find-clear = Clear
viewer-find-prev-tooltip = Previous match ({ $shortcut })
viewer-find-next-tooltip = Next match ({ $shortcut })
viewer-empty = No segments — the input may be silence-only
viewer-open-error = Could not open the transcript
viewer-col-start = Start
viewer-col-end = End
viewer-col-text = Text
viewer-col-translation = Translation
viewer-saved = edited · saved
viewer-unsaved-warning = An edit could not be saved — it is kept in memory but not written to disk
viewer-player-loading = Loading audio…
viewer-player-failed = Playback unavailable
viewer-playback-controls = Playback Controls:
viewer-loop-segment = Loop Segment
viewer-loop-segment-tooltip = Enable/disable looping when clicking on transcript segments
viewer-follow-audio = Follow Audio
viewer-follow-audio-tooltip = Enable/disable following the current audio position in the transcript. When enabled, automatically scrolls to current text.
viewer-speed = Speed:
viewer-scroll-to-current = Scroll to Current
viewer-scroll-to-current-tooltip = Scroll to the currently spoken text

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = API Key Required
viewer-translate-key-required-body = Please enter OpenAI API Key in preferences
viewer-translate-settings-title = AI Translation Settings

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Text
viewer-export-translation = Translation
viewer-export-dialog-title = Save File
viewer-export-filter = Text files
viewer-export-failed = The export could not be written — see the log for details
export-written = Written to { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Settings
resizer-create-new = Create new transcript
resizer-extend-heading = Extend end time
resizer-extend-label = Extend endings by up to (seconds)
resizer-extend-button = Extend endings
resizer-resize-heading = Resize Options
resizer-resize-label = Desired subtitle length
resizer-resize-button = Resize
resizer-resize-disabled-tooltip = Available only if word level timings were disabled during transcription
resizer-merge-heading = Merge Options
resizer-merge-by-gap = Merge by gap
resizer-split-by-punctuation = Split by punctuation
resizer-split-by-max-length = Split by max length
resizer-merge-button = Merge
resizer-merge-disabled-tooltip = Available only if word level timings were enabled during transcription
resizer-merging = Merging…
resizer-merge-error = The merge could not run:
resizer-save-error = The changes could not be saved:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Step 1: Identify speakers
speaker-id-step2-heading = Step 2: Name speakers
speaker-id-ready-label = Ready to identify speakers
speaker-id-missing-audio-label = Audio file not found
speaker-id-identify-button = Identify
speaker-id-cancel-button = Cancel
speaker-id-cancelling = Cancelling...
speaker-id-decoding = Decoding audio…
speaker-id-downloading-models = Downloading models…
speaker-id-identifying = Identifying speakers…
speaker-id-play-sample = Play sample
speaker-id-merge-checkbox = Merge speaker sentences
speaker-id-save-button = Save
speaker-id-save-error = The changes could not be saved:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Live Recording
recording-model-load-note = First time use of a model may take up to several minutest to load.
recording-advanced-button = Advanced...
recording-microphone-label = Microphone:
recording-record-button = Record
recording-stop-button = Stop
recording-realtime-warning = { $size } transcribed at { $ratio }× realtime last run — live transcription may fall behind
recording-chunk-skipped-warning = A chunk failed and was skipped: { $detail }
recording-panel-placeholder = Click Record to begin...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Waiting for AI translation...
recording-model-downloading = Downloading model… { $percent }%
recording-model-downloading-indeterminate = Downloading model…
recording-model-loading = Loading model…
recording-error-prefix = An error occurred while starting a new recording:
recording-error-suffix = Please check your audio devices or check the application logs for more information.
recording-thread-crashed = The recording session ended unexpectedly. Please check the application logs for more information.
meter-average-volume = Average volume
meter-queue = Queue

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Advanced Settings
recording-advanced-speech-heading = Speech recognition settings
recording-advanced-initial-prompt-label = Initial Prompt:
recording-advanced-translation-heading = Translation settings
recording-advanced-llm-enable = Enable AI translation
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Recording settings
recording-advanced-silence-threshold-label = Silence threshold:
recording-advanced-mode-label = Live recording mode:
recording-advanced-mode-append-below = Append below
recording-advanced-mode-append-above = Append above
recording-advanced-mode-append-and-correct = Append and correct
recording-advanced-separator-label = Line separator:
recording-advanced-step-label = Transcription step:
recording-advanced-hide-unconfirmed = Hide unconfirmed
recording-advanced-export-enable = Enable live recording export
recording-advanced-export-folder-label = Export folder:
recording-advanced-export-browse = Browse
recording-advanced-export-file-name-label = Export file name:
recording-advanced-export-file-type-label = Export file type:
recording-advanced-export-file-type-txt = Text file (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Limit export entries
    (0 = export all):
recording-advanced-upload-url-label = Upload URL:
recording-advanced-ok = Ok

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Show
presentation-show-tooltip = Show in new window
presentation-text-size-label = Text Size:
presentation-theme-label = Theme
presentation-theme-light = Light
presentation-theme-dark = Dark
presentation-theme-custom = Custom
presentation-text-color-label = Text Color
presentation-background-color-label = Background Color
presentation-fullscreen-button = Fullscreen
presentation-window-title = Live Transcript Presentation

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Copy
recording-copy-tooltip = Copy transcription to clipboard
recording-copy-copied = Copied!
recording-copy-nothing = Nothing to copy!

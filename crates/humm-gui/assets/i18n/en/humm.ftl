## humm-gui — English (en) catalog.
##
## Every user-visible string in humm-gui is keyed here and resolved through
## `i18n::tr`/`i18n::tr_args`. Locale negotiation and live switching land
## this milestone (F-008a); only this `en` catalog is embedded so far --
## Task 12 ports `de`/`es`/`zh` catalogs alongside it. Keys are kebab-case,
## grouped by the view that renders them.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; Task 12 adds the rest as each catalog
## ports. Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50).

locale-name-en-us = English

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
options-extract-speech-tooltip = Speech extraction arrives in a later release
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

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = View
viewer-mode-text = Text
viewer-mode-translation = Translation
viewer-mode-timestamps = Timestamps
viewer-export = Export
viewer-translate = Translate
viewer-resize = Resize
viewer-identify-speakers = Identify Speakers
viewer-identify-speakers-tooltip = Speaker identification arrives in a later release
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

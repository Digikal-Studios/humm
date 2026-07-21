## humm-gui — English (en) catalog.
##
## Every user-visible string in humm-gui is keyed here and resolved through
## `i18n::tr`/`i18n::tr_args` (F-008 scaffold — locale negotiation lands with
## locale ports in a later milestone; this catalog is the only one loaded for
## now). Keys are kebab-case, grouped by the view that renders them.

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

## Toolbar (views/main_window.rs, F-004)

toolbar-record = Record
toolbar-record-tooltip = Live recording arrives in a later release
toolbar-new-file = New File Transcription
toolbar-new-url = New URL Transcription
toolbar-open-transcript = Open Transcript
toolbar-open-transcript-tooltip = The transcript viewer arrives in the next milestone
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
prefs-tab-models = Models
prefs-tab-privacy = Privacy

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

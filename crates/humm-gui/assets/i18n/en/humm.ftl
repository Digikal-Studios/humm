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

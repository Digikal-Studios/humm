## humm-gui — Latvian (lv) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 15, design spec
## §7.2, batch 3 of 3). Section comments stay in English on purpose: they
## are developer-facing notes about *where* a key renders, identical in
## every catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own lv_LV translation is used
## verbatim (`buzz/locale/lv_LV/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh, matching
## the register the Buzz-sourced strings already establish. Buzz's own
## lv_LV po consistently uses the formal plural-imperative register
## throughout its own translations (e.g. "Vai tiešām vēlaties dzēst...",
## "Ievadiet jauno nosaukumu...") — this catalog stays formal consistently,
## including for the fresh humm-only strings. Buzz's own lv_LV translations
## render "transcribe"/"transcription" as "atpazīt"/"atpazīšana"
## (recognize/recognition) for the live *action*, but "transkripts"/
## "transkripcija" for the transcript/transcription *object* (e.g.
## `clear-confirm-body`'s "dzēst izvēlētos transkriptus"); this catalog
## follows both halves of that same split consistently in its own fresh
## strings.
##
## **`lv_LV` is the Lumii (Latvian whisper.cpp) model-size gate locale**
## (`i18n::lumii_locale_requested`, ported from `models_preferences_
## widget.py:214-215` and design spec §9.3) — but that gate reads the *raw*
## `settings.ui.locale`/system-locale string, never a negotiated catalog or
## this file's own content, so landing this catalog changes nothing about
## the gate's logic; it only means `negotiate(Some("lv_LV"))` now resolves
## to this catalog instead of falling back to `en_US` (see `i18n.rs`'s own
## `lv_lv_catalog_present_and_lumii_gate_reads_raw_setting_not_negotiated`
## test, updated by this same task from its pre-Task-15 form).
##
## Buzz's own lv_LV po translates the msgid "Ok" (its own relabelled dialog
## button text) as "Labi". Following the es/zh-CN/M8-Task-13/14 precedent
## (Buzz's own lineage, not a translation choice made here), all four of
## clear-confirm-ok, dialog-ok, url-dialog-ok and recording-advanced-ok
## collapse to that same "Labi".
##
## Six Buzz msgids are known (from the M8 Task 13 batch) to sometimes carry
## two translation variants concatenated with no separator, an upstream Buzz
## artifact rather than a translation choice. One of the six is affected in
## lv_LV's own po and is fixed here to the second (revised) variant alone:
## `prefs-general-force-cpu-tooltip` (doubles "Iestatiet, ja..." then
## "Aktivizējiet šo, ja..." with no separator — the second, revised
## imperative is kept). The other five are clean (single variant, no
## concatenation) in lv_LV's own po and are used as-is:
## `resizer-resize-disabled-tooltip`, `resizer-merge-disabled-tooltip`,
## `recording-model-load-note`, `viewer-loop-segment-tooltip`,
## `prefs-general-key-save-error-body`.
##
## A **seventh, newly found** doubled-variant defect, on a msgid outside
## the known six (caught by a length-ratio outlier scan over every matched
## value, not by any of the six's own known symptoms): lv_LV's po doubles
## msgid "Initial Prompt:" (feeding both `options-initial-prompt-label` and
## `recording-advanced-initial-prompt-label`) as "Sākotnējais
## uzaicinājums:" then "Sākotnējais\nvaicājums:" concatenated with no
## separator — the second variant even carries a stray embedded newline of
## its own (an artifact of the po's own line-wrapping; "Initial Prompt:" is
## single-line in en). Fixed here to the second (revised) variant alone,
## newline artifact removed, joined as "Sākotnējais vaicājums:".
##
## Two further Buzz msgids match humm strings exactly but carry a truly
## *empty* msgstr in lv_LV's po (untranslated upstream, gettext falls back
## silently): `prefs-postproc-skip-check-files-desc` and `prefs-postproc-
## skip-check-db-desc`. Both are translated fresh here rather than left
## blank — the same two keys every prior M8 locale batch has found empty.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in lv_LV (API, GPU, CPU,
## URL, Whisper, TXT/SRT/VTT) are left alone here too, matching Buzz's own
## lv_LV po (e.g. "OpenAI API atslēga", "Fails / URL") — "Backend" likewise
## stays the Latin loanword "Backend:" (no matching Buzz msgid to anchor a
## translated alternative, and every other Latin-script catalog in this
## workspace made the same choice for the same reason).

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Angļu
locale-name-ca-es = Katalāņu
locale-name-da-dk = Dāņu
locale-name-nl = Holandiešu
locale-name-fr = Franču
locale-name-de-de = Vācu
locale-name-es-es = Spāņu
locale-name-it-it = Itāļu
locale-name-ja-jp = Japāņu
locale-name-lv-lv = Latviešu
locale-name-pl-pl = Poļu
locale-name-pt-br = Portugāļu (Brazīlijas)
locale-name-ru = Krievu
locale-name-tr-tr = Turku
locale-name-uk-ua = Ukraiņu
locale-name-zh-cn = Ķīniešu (vienkāršotā)
locale-name-zh-tw = Ķīniešu (tradicionālā)

## Empty-state hero (views/main_window.rs)

hero-drop = Nometiet multivides failu šeit vai nospiediet ⌘O
hero-import-button = Importēt failus…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Fails / URL
table-col-model = Modelis
table-col-task = Uzdevums
table-col-status = Statuss
table-col-date-completed = Pabeigts
table-col-date-added = Pievienots
table-col-notes = Piezīmes

## Task table — header/row context menus

table-menu-reset-column-order = Atjaunot kolonas
table-menu-restart = Sāk atpazīšanu
table-menu-rename = Pārddēvēt
table-menu-notes = Rediģēt piezīmes

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Atpazīt
task-translate = Tulkot angliski

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = izolē runu

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Ierakstīt
toolbar-new-file = Jauna faila atpazīšana
toolbar-new-url = Jauna saites atpazīšana
toolbar-open-transcript = Atvērt transkriptu
toolbar-open-transcript-tooltip = Atlasiet vienu vai vairākus pabeigtus transkriptus, lai tos atvērtu
toolbar-cancel = Atcelt atpazīšanu
toolbar-clear = Notīrīt vēsturi

## Menu bar (views/main_window.rs, F-004)

menu-file = Fails
menu-file-import-file = Importēt failu…
menu-file-import-url = Importēt URL…
menu-file-import-folder = Importēt mapi…
menu-help-title = Palīdzība
menu-help-about = Par humm
menu-help-docs = Palīdzība
menu-help-docs-tooltip = Dokumentācija būs pieejama ar pirmo laidienu
menu-help-preferences = Iestatījumi…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Notīrīt vēsturi
clear-confirm-body = Vai tiešām vēlaties dzēst izvēlētos transkriptus? Šī ir neatgriezeniska darbība.
clear-confirm-ok = Labi
clear-confirm-cancel = Atcelt
clear-error = Neizdevās dzēst atlasītos transkriptus

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Pārdēvēt
rename-label = Ievadiet jauno nosaukumu šim atpazīšanas ierakstam:
rename-error = Neizdevās pārdēvēt
notes-title = Piezīmes
notes-label = Ievadiet noderīgas piezīmēs par šo ierakstu:
notes-error = Neizdevās saglabāt piezīmes
dialog-ok = Labi
dialog-cancel = Atcelt

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Atpazīšana pabeigta
toast-completed-suffix = fails(-i) ierakstīts(-i)
toast-canceled = Atpazīšana atcelta
toast-detail-title = Atpazīšana neizdevās
toast-detail-open-log = Atvērt žurnāla mapi
toast-detail-retry = Mēģināt vēlreiz
toast-detail-close = Aizvērt
retry-error-unavailable = Neizdevās atsākt: saglabātie iestatījumi nav pieejami

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm nevarēja saglabāt vēstures atjauninājumus — eksportēšana tomēr tiek pabeigta; skatiet žurnāla failu

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Nometiet, lai importētu

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = Trūkst yt-dlp — instalējiet to (piem., `brew install yt-dlp`) vai iestatiet HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Importēt URL
url-dialog-label = URL:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = Jūsu ievadītā URL adrese nav derīga.
url-dialog-ok = Labi
url-dialog-cancel = Atcelt

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Modelis:
options-task-label = Uzdevums:
options-language-label = Valoda:
language-detect = Noteikt valodu
options-word-timestamps = Dalīt pa vārdiem
options-extract-speech = Atdalīt runu
options-extract-speech-missing-tooltip = demucs nav atrasts — instalējiet to (piem., `pipx install demucs`), lai iespējotu runas atdalīšanu
options-export-label = Eksportēt:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = eksports nav izvēlēts
options-advanced = Papildu
options-initial-prompt-label = Sākotnējais vaicājums:
options-hf-repo-label = Hugging Face modelis
options-run = Apstrādāt

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = API atslēga:
options-llm-heading = Tulkojuma iestatījumi
options-llm-enable = Tulkot ar MI
options-llm-model-label = AI modelis:
options-llm-model-presets-button = Sagataves
options-llm-prompt-label = Norādes MI:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Vēro { $name }
folderwatch-pill-paused = Pauzēts
folderwatch-pill-pause-tip = Pauzēt mapes vērošanu
folderwatch-pill-resume-tip = Atsākt mapes vērošanu

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Pieejams atjauninājums
update-dialog-title = Pieejams atjauninājums
update-dialog-header = Pieejama jauna humm versija!
update-dialog-current-version = Instalētā versija:
update-dialog-new-version = Jaunā versija:
update-dialog-release-notes = Jaunās versijas piezīmes:
update-dialog-open-release-page = Atvērt laidiena lapu
update-dialog-close = Aizvērt

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Versija
about-check-button = Pārbaudīt atjauninājumus
about-show-logs-button = Parādīt sistēmas žurnālu
about-close-button = Aizvērt
about-up-to-date = Jums ir jaunākā versija!
about-check-failed = Neizdevās pārbaudīt atjauninājumus — skatiet žurnāla failu

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Iestatījumi
prefs-tab-general = Vispārīgi
prefs-tab-models = Modeļi
prefs-tab-shortcuts = Īsinājumi
prefs-tab-folder-watch = Mapes vērošana
prefs-tab-postproc = Pēcapstrāde
prefs-tab-privacy = Privātums

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Programmas valoda
prefs-general-font-size-label = Fonta izmērs
prefs-general-font-size-custom-label = Pielāgots fonta izmērs
prefs-general-openai-key-label = OpenAI API atslēga
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Rādīt
prefs-general-key-hide = Slēpt
prefs-general-test-button = Pārbaudīt
prefs-general-test-title = OpenAI API atslēgas pārbaude
prefs-general-test-success-body = Jūsu API atslēga ir derīga. humm izmantos to runas atpazīšanai ar Whisper API un tulkošanai.
prefs-general-invalid-key-title = Nederīga API atslēga
prefs-general-invalid-key-body = API atbalsta tikai base64 simbolus (A-Za-z0-9+/=_-). Citi simboli API atslēgā var radīt kļūdas.
prefs-general-key-save-error-title = Kļūda
prefs-general-key-save-error-body = Neizdevās saglabāt OpenAI API atslēgu atslēgu saišķī
prefs-general-base-url-label = OpenAI adrese
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI modelis
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Eksporta fails
prefs-general-force-cpu-row-label = Deaktivēt GPU
prefs-general-force-cpu-label = Izmantot tikai CPU un deaktivēt GPU paātrināšanu
prefs-general-force-cpu-tooltip = Aktivizējiet šo, ja lielāki modeļi neietilpst jūsu video kartes atmiņā un humm avarē

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Sūtīt anonīmu palaišanas telemetriju
privacy-telemetry-disabled-caption = Šajā izlaidumā telemetrija ir atspējota kompilēšanas laikā.
privacy-telemetry-items-intro = Katrs palaišanas notikums ietver:
privacy-telemetry-item-app-version = Lietotnes versija
privacy-telemetry-item-system-name = Sistēmas nosaukums
privacy-telemetry-item-os-version = OS versija
privacy-telemetry-item-kernel-release = Kodola versija
privacy-telemetry-item-cpu-architecture = CPU arhitektūra
privacy-telemetry-item-system-locale = Sistēmas lokalizācija
privacy-telemetry-item-identifier = Nejaušs anonīms identifikators
privacy-update-checkbox = Automātiski pārbaudīt atjauninājumus

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Lejupielādēts
models-section-available = Pieejams lejupielādei
models-download-button = Lejupielādēt
models-delete-button = Dzēst
models-show-location-button = Rādīt faila atrašanās vietu
models-cancel-button = Atcelt
models-custom-url-label = Pielāgots ggml modeļa URL:
models-delete-confirm-title = Dzēst modeli
models-delete-confirm-body = Vai tiešām dzēst izvēlēto modeli?
models-download-error-title = Kļūda
models-download-failed = Lejupielāde neizdevās
models-speed-badge = { $ratio }× reālajā laikā
models-speed-badge-tip = Šīs ierīces dzīvās ierakstīšanas sesijās izmērītais reālā laika ātrums

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Atvērt ieraksta logu
shortcut-desc-import-file = Importēt failu
shortcut-desc-import-url = Importēt URL
shortcut-desc-open-preferences-window = Atvērt iestatījumu logu
shortcut-desc-view-transcript-text = Aplūkot atpazīto tekstu
shortcut-desc-view-transcript-translation = Aplūkot tulkojumu
shortcut-desc-view-transcript-timestamps = Aplūkot atpazīšanas laikus
shortcut-desc-search-transcript = Meklēt tekstā
shortcut-desc-search-next = Pāriet uz nākamo meklēšanas rezultātu
shortcut-desc-search-previous = Pāriet uz iepriekšējo meklēšanas rezultātu
shortcut-desc-scroll-to-current-text = Pāriet uz atskaņojamo tesktu
shortcut-desc-play-pause-audio = Atskaņot/Apturēt audio
shortcut-desc-replay-current-segment = Atskaņot segmentu no sākuma
shortcut-desc-toggle-playback-controls = Pārslēgt atskaņošanas iespējas
shortcut-desc-decrease-segment-start = Samazināt segmenta sākuma laiku
shortcut-desc-increase-segment-start = Palielināt segmenta sākuma laiku
shortcut-desc-decrease-segment-end = Samazināt segmenta beigu laiku
shortcut-desc-increase-segment-end = Palielināt segmenta beigu laiku
shortcut-desc-clear-history = Notīrīt vēsturi
shortcut-desc-stop-transcription = Atcelt atpazīšanu
prefs-shortcuts-press = Nospiediet saīsni…
prefs-shortcuts-clear-tooltip = Atjaunot noklusējumu
prefs-shortcuts-conflict = Konflikts ar { $other }
prefs-shortcuts-reset = Atjaunot noklusētos

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Ieslēgt mapes vērošanu
prefs-fw-input-folder-label = Vērojamā mape
prefs-fw-input-folder-placeholder = /ceļš/uz/ievades/mapi
prefs-fw-output-folder-label = Rezultātu mape
prefs-fw-output-folder-placeholder = /ceļš/uz/izvades/mapi
prefs-fw-browse = Izvēlēties
prefs-fw-delete-checkbox = Dzēst apstrādātos failus
prefs-fw-disposition-note = Pēc atpazīšanas pabeigšanas avota fails tiek pārvietots uz izvades mapi — vai dzēsts, ja atzīmēts Dzēst apstrādātos failus.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Šie āķi tiek automātiski palaisti pēc katras atpazīšanas pabeigšanas. Āķa kļūda tiek ierakstīta žurnālā, bet nekad neizraisa pašas atpazīšanas neizdošanos.
prefs-postproc-skip-title = Izlaist jau transkribētos
prefs-postproc-skip-enable = Iespējot Izlaist jau atpazīto
prefs-postproc-skip-check-files = Pārbaudīt esošos rezultātu failus
prefs-postproc-skip-check-files-desc = Izlaist, ja blakus tiek atrasts .txt, .srt vai .vtt fails ar audio faila nosaukumu
prefs-postproc-skip-check-db = Pārbaudīt transkripciju datubāzē
prefs-postproc-skip-check-db-desc = Izlaist, ja šis fails jau ir atpazīts un ieraksts pastāv datubāzē
prefs-postproc-dfn-title = DeepFilterNet trokšņu samazināšana
prefs-postproc-dfn-enable = Iespējot DeepFilterNet trokšņu slāpēšanu
prefs-postproc-dfn-keep-file = Saglabāt attīrīto failu pēc transkribēšanas
prefs-postproc-langdetect-title = Uzlabota valodas noteikšana
prefs-postproc-langdetect-enable = Iespējot uzlaboto valodas noteikšanu
prefs-postproc-langdetect-download-tiny = Lejupielādēt tiny modeli, ja neviens nav pieejams
prefs-postproc-langdetect-download-tiny-desc = Ja nav lejupielādēts neviens whisper.cpp modelis, lejupielādē tiny modeli valodas noteikšanai.
prefs-postproc-resizer-title = Automātiska garuma maiņa
prefs-postproc-resizer-enable = Iespējot automātisko transkripta pārveidotāju
prefs-postproc-resizer-merge-by-gap = Apvienot pēc atstarpes
prefs-postproc-resizer-merge-gap-seconds = Apvienošanas atstarpe (sekundes)
prefs-postproc-resizer-split-by-punctuation = Dalīt pie pieturzīmēm
prefs-postproc-resizer-punctuation = Pieturzīmes
prefs-postproc-resizer-split-by-max-length = Dalīt pie maksimālā garuma
prefs-postproc-resizer-max-length = Maksimālais subtitru garums
prefs-postproc-summary-title = MI kopsavilkums
prefs-postproc-summary-enable = Iespējot AI kopsavilkumu
prefs-postproc-summary-api-url = API bāzes URL
prefs-postproc-summary-api-key-label = API atslēga
prefs-postproc-summary-model = Modelis
prefs-postproc-summary-prompt = MI instrikcija
prefs-postproc-summary-save-to-notes = Saglabāt piezīmēs
prefs-postproc-summary-save-to-file = Saglabāt failā
prefs-postproc-summary-output-folder = Izvades mape
prefs-postproc-summary-key-save-error-body = Neizdevās saglabāt AI kopsavilkuma API atslēgu atslēgu saišķī
prefs-postproc-docx-title = Eksportēt uz DOCX
prefs-postproc-docx-enable = Iespējot eksportu uz DOCX
prefs-postproc-docx-output-folder = Rezultātu mape
prefs-postproc-docx-include-timestamps = Iekļaut laika atzīmes
prefs-postproc-output-folder-hint = Atstājiet tukšu, lai saglabātu blakus avota failam.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Skats
viewer-mode-text = Teksts
viewer-mode-translation = Tulkojums
viewer-mode-timestamps = Laiks
viewer-export = Eksportēt
viewer-translate = Tulkot
viewer-resize = Mainīt garumu
viewer-identify-speakers = Noteikt runātājus
viewer-find = Meklēt
viewer-find-label = Meklēt:
viewer-find-placeholder = Ievadiet meklējamo...
viewer-find-clear = Notīrīt
viewer-find-prev-tooltip = Iepriekšējais atbilstošais ({ $shortcut })
viewer-find-next-tooltip = Nākamais atbilstošais ({ $shortcut })
viewer-empty = Nav segmentu — iespējams, ierakstā ir tikai klusums
viewer-open-error = Neizdevās atvērt transkriptu
viewer-col-start = Sākums
viewer-col-end = Beigas
viewer-col-text = Teksts
viewer-col-translation = Tulkojums
viewer-saved = rediģēts · saglabāts
viewer-unsaved-warning = Izmaiņas neizdevās saglabāt — tās ir paturētas atmiņā, bet nav ierakstītas diskā
viewer-player-loading = Ielādē audio…
viewer-player-failed = Atskaņošana nav pieejama
viewer-playback-controls = Atskaņošana:
viewer-loop-segment = Atkārtot segmentu
viewer-loop-segment-tooltip = Nosaka vai atkārtot izvēlēto segmentu
viewer-follow-audio = Sekot audio
viewer-follow-audio-tooltip = Nosaka vai atskaņojot audio iezīmētajam segmentam vajadzētu automātiski sekot tam kas tiek atskaņots.
viewer-speed = Ātrums:
viewer-scroll-to-current = Pāriet uz tekošo
viewer-scroll-to-current-tooltip = Pāriet uz šobrīd atskaņojamo tesktu

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = API atslēgas kļūda
viewer-translate-key-required-body = Lūdzu ievadiet OpenAI API atslēgu iestatījumos
viewer-translate-settings-title = AI tulkošanas iestatījumi

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Teksts
viewer-export-translation = Tulkojums
viewer-export-dialog-title = Saglabāt failu
viewer-export-filter = Teksta faili
viewer-export-failed = Neizdevās ierakstīt eksportu — sīkāku informāciju skatiet žurnālā
export-written = Ierakstīts { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Iestatījumi
resizer-create-new = Izveidot jaunu transkripciju
resizer-extend-heading = Pagarināt beigu laiku
resizer-extend-label = Palielināt beigu laiku par (sekundes)
resizer-extend-button = Palielināt
resizer-resize-heading = Garuma maiņas iestatījumi
resizer-resize-label = Vēlamais teksta garums
resizer-resize-button = Mainīt garumu
resizer-resize-disabled-tooltip = Pieejami tikai, ierakstiem, kas atpazīti bez dalīšanas pa vārdiem
resizer-merge-heading = Apvienošanas iestatījumi
resizer-merge-by-gap = Apvienot pēc atstarpes
resizer-split-by-punctuation = Dalīt pie pieturzīmēm
resizer-split-by-max-length = Dalīt pie maksimālā garuma
resizer-merge-button = Apvienot
resizer-merge-disabled-tooltip = Pieejami tikai, ierakstiem, kas atpazīti ar dalīšanu pa vārdiem
resizer-merging = Apvieno…
resizer-merge-error = Neizdevās veikt apvienošanu:
resizer-save-error = Neizdevās saglabāt izmaiņas:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = 1. solis: Runātāju noteikšana
speaker-id-step2-heading = 2. solis: Runātāju identifikācija
speaker-id-ready-label = Gatavs noteikt runātājus
speaker-id-missing-audio-label = Audio datne nav atrasta
speaker-id-identify-button = Noteikt
speaker-id-cancel-button = Atcelt
speaker-id-cancelling = Atceļ...
speaker-id-decoding = Dekodē audio…
speaker-id-downloading-models = Lejupielādē modeļus…
speaker-id-identifying = Identificē runātājus…
speaker-id-play-sample = Atskaņot paraugu
speaker-id-merge-checkbox = Apvienot secīgus runātāja teikumus
speaker-id-save-button = Saglabāt
speaker-id-save-error = Neizdevās saglabāt izmaiņas:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Dzīvā ierakstīšana
recording-model-load-note = Pirmā modeļa ielādes reize var aizņemt pat vairākas minūtes.
recording-advanced-button = Papildu iestatījumi...
recording-microphone-label = Mikrofons:
recording-record-button = Ierakstīt
recording-stop-button = Apturēt
recording-realtime-warning = { $size } pēdējā reizē atpazīts ar { $ratio }× reālā laika ātrumu — dzīvā atpazīšana var atpalikt
recording-chunk-skipped-warning = Fragments neizdevās un tika izlaists: { $detail }
recording-panel-placeholder = Klikšķiniet Ierakstīt, lai sāktu...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Gaida MI tulkojumu...
recording-model-downloading = Lejupielādē modeli… { $percent }%
recording-model-downloading-indeterminate = Lejupielādē modeli…
recording-model-loading = Ielādē modeli…
recording-error-prefix = Sākot jaunu ierakstu notikusi kļūda:
recording-error-suffix = Lūdzu pārbaudiet savas audio ierīces vai pārbaudiet lietotnes ziņojumu žurnālus, lai iegūtu papildu informāciju.
recording-thread-crashed = Ieraksta sesija negaidīti beidzās. Lūdzu, pārbaudiet lietotnes žurnālus, lai iegūtu vairāk informācijas.
meter-average-volume = Vidējais skaļums
meter-queue = Rinda

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Papildu iestatījumi
recording-advanced-speech-heading = Runas atpazīšanas iestatījumi
recording-advanced-initial-prompt-label = Sākotnējais vaicājums:
recording-advanced-translation-heading = Tulkojuma iestatījumi
recording-advanced-llm-enable = Tulkot ar MI
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Dzīvā ieraksta iestatījumi
recording-advanced-silence-threshold-label = Klusuma slieksnis:
recording-advanced-mode-label = Dzīvā ieraksta režīms:
recording-advanced-mode-append-below = Jaunie teikumi apakšā
recording-advanced-mode-append-above = Jaunie teikumi augšā
recording-advanced-mode-append-and-correct = Papildināt un labot esošo
recording-advanced-separator-label = Rindiņu atdalītājs:
recording-advanced-step-label = Atpazīšanas solis:
recording-advanced-hide-unconfirmed = Slēpt mainīgos fragmentus
recording-advanced-export-enable = Eksportēt dzīvā ieraksta transkriptus
recording-advanced-export-folder-label = Eksportēt mapē:
recording-advanced-export-browse = Izvēlēties
recording-advanced-export-file-name-label = Eksporta fails:
recording-advanced-export-file-type-label = Eksporta faila tips:
recording-advanced-export-file-type-txt = Teksta fails (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Eksportēt tikai pēdējos
    (0 = eksportēt visus):
recording-advanced-upload-url-label = Augšupielādes URL:
recording-advanced-ok = Labi

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Rādīt
presentation-show-tooltip = Rādīt jaunā logā
presentation-text-size-label = Teksta izmērs:
presentation-theme-label = Stils
presentation-theme-light = Gaišais
presentation-theme-dark = Tumšais
presentation-theme-custom = Pielāgots
presentation-text-color-label = Teksta krāsa
presentation-background-color-label = Fona krāsa
presentation-fullscreen-button = Pilnekrāns
presentation-window-title = Dzīvais ieraksts

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Kopēt
recording-copy-tooltip = Kopēt tekstu starpliktuvē
recording-copy-copied = Nokopēts!
recording-copy-nothing = Nav nekā ko kopēt!

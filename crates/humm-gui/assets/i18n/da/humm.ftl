## humm-gui — Danish (da) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 13, design spec §9.2).
## Section comments stay in English on purpose: they are developer-facing
## notes about *where* a key renders, identical in every catalog, not
## translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own da_DK translation is used
## verbatim (`buzz/locale/da_DK/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh in a
## natural, internally consistent register. Buzz's da_DK po carries no
## formal/informal header marker, and modern Danish rarely distinguishes
## address forms at all (du is standard even in formal contexts), so this
## catalog does not distinguish one either.
##
## Buzz's own da_DK po translates the msgid "Ok" (its own relabelled dialog
## button text) as "OK". Following the es/zh-CN precedent (Buzz's own
## lineage, not a translation choice made here), all four of clear-confirm-ok,
## dialog-ok, url-dialog-ok and recording-advanced-ok collapse to that same
## "OK" — the en catalog's deliberate Ok/OK casing split is English-source-
## specific Qt lineage and does not survive translation.
##
## Six Buzz msgids carry two translation variants concatenated with no
## separator, an upstream Buzz artifact rather than a translation choice
## (already documented for de/es). Three of the six are affected in da_DK's
## own po and are fixed here to the second (revised) variant alone:
## `prefs-general-force-cpu-tooltip`, `resizer-resize-disabled-tooltip`,
## `resizer-merge-disabled-tooltip`. The other three are clean (single
## variant, no concatenation) in da_DK's po and are used as-is:
## `recording-model-load-note`, `viewer-loop-segment-tooltip`,
## `prefs-general-key-save-error-body`.
##
## Two further Buzz msgids match humm strings exactly but carry an *empty*
## msgstr in da_DK's po (untranslated upstream, gettext falls back silently):
## `prefs-postproc-skip-check-files-desc` and `prefs-postproc-skip-check-db-
## desc`. Both are translated fresh here rather than left blank.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in da (API, GPU, CPU,
## Whisper, URL, TXT/SRT/VTT) are left alone here too; "Backend" is likewise
## kept untranslated, matching de/es's own choice for the same term.
##
## A handful of Buzz's own da_DK strings carry minor typos or idiosyncratic
## wording of their own (e.g. "Downloadded", "Skriftypestørrelse", "guve" for
## "give", "mereinformation" missing a space) — these are not among the six
## concatenation artifacts above, so per the byte-verbatim sourcing rule they
## are carried through unchanged rather than silently corrected.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Engelsk
locale-name-ca-es = Catalansk
locale-name-da-dk = Dansk
locale-name-nl = Nederlandsk
locale-name-de-de = Tysk
locale-name-es-es = Spansk
locale-name-it-it = Italiensk
locale-name-ja-jp = Japansk
locale-name-pl-pl = Polsk
locale-name-pt-br = Portugisisk (Brasilien)
locale-name-ru = Russisk
locale-name-zh-cn = Kinesisk (forenklet)

## Empty-state hero (views/main_window.rs)

hero-drop = Slip medier her, eller ⌘O
hero-import-button = Importer filer…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Filnavn / URL
table-col-model = Model
table-col-task = Opgave
table-col-status = Status
table-col-date-completed = Dato for færdiggørelse
table-col-date-added = Dato for tilføjelse
table-col-notes = Noter

## Task table — header/row context menus

table-menu-reset-column-order = Nulstil kolonnerækkefølge
table-menu-restart = Genstart transkription
table-menu-rename = Omdøb
table-menu-notes = Tilføj/rediger noter

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transkriber
task-translate = Oversæt til engelsk

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = isolerer tale

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Optag
toolbar-new-file = Ny filtranskription
toolbar-new-url = Ny URL-transkription
toolbar-open-transcript = Åben transkription
toolbar-open-transcript-tooltip = Vælg en eller flere færdige transskriptioner, der skal åbnes
toolbar-cancel = Afbryd transkription
toolbar-clear = Ryd historik

## Menu bar (views/main_window.rs, F-004)

menu-file = Fil
menu-file-import-file = Importer Fil…
menu-file-import-url = Importer URL…
menu-file-import-folder = Importer Mappe…
menu-help-title = Hjælp
menu-help-about = Om humm
menu-help-docs = Hjælp
menu-help-docs-tooltip = Dokumentationen kommer med den første udgivelse
menu-help-preferences = Indstillinger…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Ryd historik
clear-confirm-body = Er du sikker på at du vil slette den valgte transkription? Denne handling kan ikke fortrydes.
clear-confirm-ok = OK
clear-confirm-cancel = Afbryd
clear-error = Kunne ikke slette de valgte transskriptioner

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Omdøb transkription
rename-label = Indtast nyt navn:
rename-error = Kunne ikke omdøbe
notes-title = Noter
notes-label = Indtast relevante noter til denne transkription:
notes-error = Kunne ikke gemme noterne
dialog-ok = OK
dialog-cancel = Afbryd

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transskription fuldført
toast-completed-suffix = fil(er) skrevet
toast-canceled = Transskription afbrudt
toast-detail-title = Transskription mislykkedes
toast-detail-open-log = Åbn logmappe
toast-detail-retry = Prøv igen
toast-detail-close = Luk
retry-error-unavailable = Kunne ikke genstarte: de gemte indstillinger er ikke tilgængelige

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm kunne ikke registrere historikopdateringer — eksporter fuldføres stadig; se logfilen

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Slip for at importere

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp mangler — installer det (f.eks. `brew install yt-dlp`) eller angiv HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Importer fra URL
url-dialog-label = URL:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = Den URL du har angivet er ikke gyldig.
url-dialog-ok = OK
url-dialog-cancel = Afbryd

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Model:
options-task-label = Opgave:
options-language-label = Sprog:
language-detect = Detekter sprog
options-word-timestamps = Ord tidsniveau
options-extract-speech = Eksakt tale
options-extract-speech-missing-tooltip = demucs blev ikke fundet — installer det (f.eks. `pipx install demucs`) for at aktivere taleudtrækning
options-export-label = Eksporter:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = ingen eksport valgt
options-advanced = Advanceret
options-initial-prompt-label = Start prompt:
options-hf-repo-label = Hugging Face-model
options-run = Kør

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = API-nøgle:
options-llm-heading = Oversættelsesindstillinger
options-llm-enable = Brug AI oversættelse
options-llm-model-label = AI model:
options-llm-model-presets-button = Forudindstillinger
options-llm-prompt-label = Instruktioner for AI:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Overvåger { $name }
folderwatch-pill-paused = Sat på pause
folderwatch-pill-pause-tip = Sæt mappeovervågning på pause
folderwatch-pill-resume-tip = Genoptag mappeovervågning

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Opdatering tilgængelig
update-dialog-title = Opdatering tilgængelig
update-dialog-header = En ny version af humm er tilgængelig!
update-dialog-current-version = Nuværende version:
update-dialog-new-version = Ny version:
update-dialog-release-notes = Udgivelsesnoter:
update-dialog-open-release-page = Åbn udgivelsesside
update-dialog-close = Luk

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Version
about-check-button = Tjek for opdateringer
about-show-logs-button = Vis logfiler
about-close-button = Luk
about-up-to-date = Du er opdateret!
about-check-failed = Opdateringstjek mislykkedes — se logfilen

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Indstillinger
prefs-tab-general = Generelt
prefs-tab-models = Modeller
prefs-tab-shortcuts = Genveje
prefs-tab-folder-watch = Mappeovervågning
prefs-tab-postproc = Efterbehandling
prefs-tab-privacy = Privatliv

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Brugerfladesprog
prefs-general-font-size-label = Skriftypestørrelse
prefs-general-font-size-custom-label = Brugerdefineret skriftstørrelse
prefs-general-openai-key-label = OpenAI API-nøgle
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Vis
prefs-general-key-hide = Skjul
prefs-general-test-button = Test
prefs-general-test-title = OpenAI API Nøgle test
prefs-general-test-success-body = Din API nøgle er gyldig. humm vil benytte nøglen til at anvende Whisper API transkription og AI oversættelser.
prefs-general-invalid-key-title = Ugyldig API-nøgle
prefs-general-invalid-key-body = API supporterer kun base64 tegn (A-Za-z0-9+/=_-). Andre tegn i API-nøglen kan guve fejl.
prefs-general-key-save-error-title = Fejl
prefs-general-key-save-error-body = Kan ikke gemme OpenAI API-nøgle i nøgleringen
prefs-general-base-url-label = OpenAI base-URL
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI API-model
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Standard eksport filnavn
prefs-general-force-cpu-row-label = Deaktiver GPU
prefs-general-force-cpu-label = Brug kun CPU og deaktiver GPU-acceleration
prefs-general-force-cpu-tooltip = Aktivér dette hvis større modeller ikke passer i GPU-hukommelsen og humm crasher

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Send anonym opstartstelemetri
privacy-telemetry-disabled-caption = Denne build har telemetri deaktiveret ved kompilering.
privacy-telemetry-items-intro = Hver opstartshændelse indeholder:
privacy-telemetry-item-app-version = App-version
privacy-telemetry-item-system-name = Systemnavn
privacy-telemetry-item-os-version = OS-version
privacy-telemetry-item-kernel-release = Kernel-version
privacy-telemetry-item-cpu-architecture = CPU-arkitektur
privacy-telemetry-item-system-locale = Systemlokalitet
privacy-telemetry-item-identifier = En tilfældig anonym identifikator
privacy-update-checkbox = Tjek automatisk for opdateringer

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Downloadded
models-section-available = Tilgængelige til download
models-download-button = Download
models-delete-button = Slet
models-show-location-button = Vis fil-lokation
models-cancel-button = Afbryd
models-custom-url-label = Brugerdefineret ggml-model-URL:
models-delete-confirm-title = Slet model
models-delete-confirm-body = Er du sikker på at du vil slette den valgte model?
models-download-error-title = Fejl
models-download-failed = Download mislykkedes
models-speed-badge = { $ratio }× realtid
models-speed-badge-tip = Målt realtidshastighed fra denne maskines liveoptagelsessessioner

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Åben optagevinduet
shortcut-desc-import-file = Import fil
shortcut-desc-import-url = Importer fra URL
shortcut-desc-open-preferences-window = Åben indstillingsvinduet
shortcut-desc-view-transcript-text = Vis transkriberede tekst
shortcut-desc-view-transcript-translation = Vis transkriberede oversættelse
shortcut-desc-view-transcript-timestamps = Vis transkriptionstidstempler
shortcut-desc-search-transcript = Søg i transskription
shortcut-desc-search-next = Gå til næste søgeresultat i transskription
shortcut-desc-search-previous = Gå til forrige søgeresultat i transskription
shortcut-desc-scroll-to-current-text = Rul til aktuel tekst
shortcut-desc-play-pause-audio = Afspil/Pause lyd
shortcut-desc-replay-current-segment = Afspil nuværende segment igen
shortcut-desc-toggle-playback-controls = Skift afspilningskontroller
shortcut-desc-decrease-segment-start = Reducer segmentets starttidspunkt
shortcut-desc-increase-segment-start = Øg segmentets starttidspunkt
shortcut-desc-decrease-segment-end = Reducer segmentets sluttidspunkt
shortcut-desc-increase-segment-end = Øg segmentets sluttidspunkt
shortcut-desc-clear-history = Ryd historik
shortcut-desc-stop-transcription = Afbryd transkription
prefs-shortcuts-press = Tryk genvejstast…
prefs-shortcuts-clear-tooltip = Nulstil til standard
prefs-shortcuts-conflict = Er i konflikt med { $other }
prefs-shortcuts-reset = Gendan standard-indstillinger

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Aktiver mappeovervågning
prefs-fw-input-folder-label = Inputmappe
prefs-fw-input-folder-placeholder = /sti/til/inputmappe
prefs-fw-output-folder-label = Outputmappe
prefs-fw-output-folder-placeholder = /sti/til/outputmappe
prefs-fw-browse = Gennemse
prefs-fw-delete-checkbox = Slet behandlede filer
prefs-fw-disposition-note = Når transskriptionen er fuldført, flyttes kildefilen til outputmappen — eller slettes, hvis Slet behandlede filer er markeret.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Disse hooks køres automatisk, efter hver transskription er fuldført. En hook-fejl logges, men får aldrig selve transskriptionen til at mislykkes.
prefs-postproc-skip-title = Spring allerede transkriberede over
prefs-postproc-skip-enable = Aktiver Spring allerede transkriberede over
prefs-postproc-skip-check-files = Tjek for eksisterende resultatfiler
prefs-postproc-skip-check-files-desc = Spring over, hvis der findes en .txt-, .srt- eller .vtt-fil med samme navn som lydfilen ved siden af den
prefs-postproc-skip-check-db = Tjek i transskriptionsdatabasen
prefs-postproc-skip-check-db-desc = Spring over, hvis denne fil allerede er transskriberet, og posten findes i databasen
prefs-postproc-dfn-title = DeepFilterNet støjreduktion
prefs-postproc-dfn-enable = Aktiver DeepFilterNet støjreduktion
prefs-postproc-dfn-keep-file = Behold den støjreducerede fil efter transskription
prefs-postproc-langdetect-title = Forbedret sprogregistrering
prefs-postproc-langdetect-enable = Aktiver Forbedret sprogregistrering
prefs-postproc-langdetect-download-tiny = Download tiny-modellen, hvis ingen er tilgængelig
prefs-postproc-langdetect-download-tiny-desc = Hvis der ikke er downloadet nogen whisper.cpp-model, downloades tiny-modellen til brug for sprogregistrering.
prefs-postproc-resizer-title = Automatisk tilpasning af transskription
prefs-postproc-resizer-enable = Aktiver Automatisk tilpasning af transskription
prefs-postproc-resizer-merge-by-gap = Flet efter mellemrum
prefs-postproc-resizer-merge-gap-seconds = Fletteinterval (sekunder)
prefs-postproc-resizer-split-by-punctuation = Opdel efter tegnsætning
prefs-postproc-resizer-punctuation = Tegnsætning
prefs-postproc-resizer-split-by-max-length = Opdel efter maksimal længde
prefs-postproc-resizer-max-length = Maksimal undertekstlængde
prefs-postproc-summary-title = AI-resumé
prefs-postproc-summary-enable = Aktiver AI-resumé
prefs-postproc-summary-api-url = API-basis-URL
prefs-postproc-summary-api-key-label = API-nøgle
prefs-postproc-summary-model = Model
prefs-postproc-summary-prompt = Resuméprompt
prefs-postproc-summary-save-to-notes = Gem resumé i noter
prefs-postproc-summary-save-to-file = Gem resumé i en fil
prefs-postproc-summary-output-folder = Outputmappe (til fil)
prefs-postproc-summary-key-save-error-body = Kan ikke gemme API-nøglen til AI-resumé i nøgleringen
prefs-postproc-docx-title = Eksportér til DOCX
prefs-postproc-docx-enable = Aktiver Eksportér til DOCX
prefs-postproc-docx-output-folder = Outputmappe
prefs-postproc-docx-include-timestamps = Inkludér tidsstempler
prefs-postproc-output-folder-hint = Lad stå tom for at gemme ved siden af kildefilen.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Vis
viewer-mode-text = Tekst
viewer-mode-translation = Oversættelse
viewer-mode-timestamps = Tidsstempler
viewer-export = Eksporter
viewer-translate = Oversæt
viewer-resize = Behandel størrelse
viewer-identify-speakers = Identificer talere
viewer-find = Find
viewer-find-label = Find:
viewer-find-placeholder = Indtast tekst at søge efter...
viewer-find-clear = Ryd
viewer-find-prev-tooltip = Forrige match ({ $shortcut })
viewer-find-next-tooltip = Næste match ({ $shortcut })
viewer-empty = Ingen segmenter — inputtet indeholder muligvis kun stilhed
viewer-open-error = Kunne ikke åbne transskriptionen
viewer-col-start = Start
viewer-col-end = Slut
viewer-col-text = Tekst
viewer-col-translation = Oversættelse
viewer-saved = redigeret · gemt
viewer-unsaved-warning = En redigering kunne ikke gemmes — den bevares i hukommelsen, men er ikke skrevet til disken
viewer-player-loading = Indlæser lyd…
viewer-player-failed = Afspilning ikke tilgængelig
viewer-playback-controls = Afspilningskontroller:
viewer-loop-segment = Gentag segment
viewer-loop-segment-tooltip = Aktiver/deaktiver gentagelse ved klik på transkriptionssegmenter
viewer-follow-audio = Følg lyd
viewer-follow-audio-tooltip = Aktiver/deaktiver at følge den aktuelle lydposition i transkriptionen. Når aktiveret, scrolles der automatisk til den aktuelle tekst.
viewer-speed = Hastighed:
viewer-scroll-to-current = Rul til aktuel
viewer-scroll-to-current-tooltip = Rul til den tekst der tales i øjeblikket

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = API-nøgle påkrævet
viewer-translate-key-required-body = Indtast venligst OpenAI API-nøgle i indstillinger
viewer-translate-settings-title = Indstillinger for AI-oversættelse

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Tekst
viewer-export-translation = Oversættelse
viewer-export-dialog-title = Gem fil
viewer-export-filter = Tekst filer
viewer-export-failed = Eksporten kunne ikke skrives — se loggen for detaljer
export-written = Skrevet til { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Indstillinger
resizer-create-new = Opret ny transkription
resizer-extend-heading = Forlæng sluttidspunkt
resizer-extend-label = Forlæng slutninger med op til (sekunder)
resizer-extend-button = Forlæng slutninger
resizer-resize-heading = Størrelsesindstillinger
resizer-resize-label = Ønskede undertekst længde
resizer-resize-button = Behandel størrelse
resizer-resize-disabled-tooltip = Kun tilgængeligt hvis ordniveau-tidsstempler var deaktiveret under transkription
resizer-merge-heading = Sammenfletningsindstillinger
resizer-merge-by-gap = Flet efter mellemrum
resizer-split-by-punctuation = Opdel efter tegnsætning
resizer-split-by-max-length = Opdel efter maksimal længde
resizer-merge-button = Sammenflet
resizer-merge-disabled-tooltip = Kun tilgængeligt hvis ordniveau-tidsstempler var aktiveret under transkription
resizer-merging = Sammenfletter…
resizer-merge-error = Sammenfletningen kunne ikke køre:
resizer-save-error = Ændringerne kunne ikke gemmes:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Trin 1: Identificer talere
speaker-id-step2-heading = Trin 2: Navngiv talere
speaker-id-ready-label = Klar til at identificere talere
speaker-id-missing-audio-label = Lydfil ikke fundet
speaker-id-identify-button = Identificer
speaker-id-cancel-button = Afbryd
speaker-id-cancelling = Annullerer...
speaker-id-decoding = Dekoder lyd…
speaker-id-downloading-models = Downloader modeller…
speaker-id-identifying = Identificerer talere…
speaker-id-play-sample = Afspil eksempel
speaker-id-merge-checkbox = Sammenflet talerens sætninger
speaker-id-save-button = Gem
speaker-id-save-error = Ændringerne kunne ikke gemmes:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Live optagelse
recording-model-load-note = Først gang kan brug af en model tage flere minutter at indlæse.
recording-advanced-button = Advanceret...
recording-microphone-label = Mikrofon:
recording-record-button = Optag
recording-stop-button = Stop
recording-realtime-warning = { $size } blev transskriberet med { $ratio }× realtid i sidste kørsel — live-transskription kan sakke bagud
recording-chunk-skipped-warning = En blok mislykkedes og blev sprunget over: { $detail }
recording-panel-placeholder = Klik Optage for at begynde...
recording-panel-translation-placeholder = Venter på AI oversættelse...
recording-model-downloading = Downloader model… { $percent }%
recording-model-downloading-indeterminate = Downloader model…
recording-model-loading = Indlæser model…
recording-error-prefix = Der skete en fejl ved opstart af en ny optagelse:
recording-error-suffix = Tjek venligst dine audioenheder eller tjek applikationens logs for mereinformation.
recording-thread-crashed = Optagelsessessionen afsluttedes uventet. Tjek venligst applikationens logs for mere information.
meter-average-volume = Gennemsnitlig lydstyrke
meter-queue = Kø

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Advancerede indstillinger
recording-advanced-speech-heading = Talegenkendelsesindstillinger
recording-advanced-initial-prompt-label = Start prompt:
recording-advanced-translation-heading = Oversættelsesindstillinger
recording-advanced-llm-enable = Brug AI oversættelse
recording-advanced-recording-heading = Optagelsesindstillinger
recording-advanced-silence-threshold-label = Tærskelværdi for stilhed:
recording-advanced-mode-label = Live optagelsestilstand:
recording-advanced-mode-append-below = Tilføj herunder
recording-advanced-mode-append-above = Tilføj herover
recording-advanced-mode-append-and-correct = Tilføj og ret
recording-advanced-separator-label = Linjeseparator:
recording-advanced-step-label = Transskriberingstrin:
recording-advanced-hide-unconfirmed = Skjul ubekræftet
recording-advanced-export-enable = Aktiver eksport af live optagelse
recording-advanced-export-folder-label = Eksportmappe:
recording-advanced-export-browse = Gennemse
recording-advanced-export-file-name-label = Eksport filnavn:
recording-advanced-export-file-type-label = Eksport filtype:
recording-advanced-export-file-type-txt = Tekstfil (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Begræns eksportposter
    (0 = eksporter alle):
recording-advanced-upload-url-label = Upload-URL:
recording-advanced-ok = OK

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Vis
presentation-show-tooltip = Vis i nyt vindue
presentation-text-size-label = Tekststørrelse:
presentation-theme-label = Tema
presentation-theme-light = Lys
presentation-theme-dark = Mørk
presentation-theme-custom = Brugerdefineret
presentation-text-color-label = Tekstfarve
presentation-background-color-label = Baggrundsfarve
presentation-fullscreen-button = Fuldskærm
presentation-window-title = Live transkriptionspræsentation

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Kopiér
recording-copy-tooltip = Kopiér transkription til udklipsholder
recording-copy-copied = Kopieret!
recording-copy-nothing = Intet at kopiere!

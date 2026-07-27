## humm-gui — Italian (it) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 13, design spec
## §9.2). Section comments stay in English on purpose: they are
## developer-facing notes about *where* a key renders, identical in every
## catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own it_IT translation is used
## verbatim (`buzz/locale/it_IT/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh, matching
## the register the Buzz-sourced strings already establish. Buzz's it_IT po
## has no formal/informal header marker, but its own translator consistently
## used the informal register (tu) throughout — imperatives like "Inserisci",
## "Scarica", "Controlla" and second-person forms like "Sei sicuro di voler
## eliminare…" rather than "Lei" — so this catalog stays informal (tu)
## consistently, including for the fresh humm-only strings. A handful of
## verbatim Buzz strings also reuse two of the po's obsolete (`#~`) entries
## ("Live recording mode:", "Close") -- still genuine Buzz it_IT translations,
## just for msgids the current Buzz source no longer references.
##
## "Ok" collapse: Buzz's it_IT po translates its own relabelled dialog button
## msgid "Ok" as "Ok" (unchanged casing). Following the es/zh-CN precedent,
## clear-confirm-ok/dialog-ok/url-dialog-ok/recording-advanced-ok all collapse
## to that same "Ok" here — Buzz's own it lineage, not a translation choice.
## This happens to coincide with en's own "Ok" spelling for three of those
## four keys (dialog-ok is "OK" in en) — a coincidence of this locale, not
## intent; en's own Ok/OK split is English-source-specific and does not
## survive translation, exactly as de/es's headers explain.
##
## Six Buzz msgids carry two merged translation variants concatenated with no
## separator, an upstream Buzz artifact rather than a translation choice.
## Unlike de (six affected) and es (five affected, one clean), **all six are
## affected in it_IT** — this is the only locale of this batch where the
## defect hits every one of them:
## `prefs-general-force-cpu-tooltip`, `recording-model-load-note`,
## `viewer-loop-segment-tooltip`, `resizer-resize-disabled-tooltip`,
## `resizer-merge-disabled-tooltip`, `prefs-general-key-save-error-body`.
## Each takes the second (revised) variant alone (Buzz→humm substitution
## already applied where relevant).
##
## Separately, `recording-advanced-title` ("Advanced Settings") carries an
## unrelated upstream Buzz it_IT typo -- msgstr "Impostazion avanzate" (missing
## the final "i") -- kept byte-verbatim since it is not one of the six
## documented concatenation defects above and the sourcing rule calls for
## verbatim reuse, not silent correction of arbitrary Buzz typos.
##
## Two Buzz it_IT msgids exist for this catalog's plugin-description pair
## (`prefs-postproc-skip-check-files-desc`/`-db-desc`) but ship with an empty
## msgstr (untranslated upstream) -- treated as no-Buzz-translation-available
## and translated fresh instead, matching this catalog's own register.
##
## `prefs-general-invalid-key-body`'s Buzz it_IT msgstr truncates the base64
## character class to "(A-Za-z0-9+/=)" (missing "_-"), silently dropping two
## characters the English source's own class explicitly lists
## ("A-Za-z0-9+/=_-"). Since this string states an actual technical
## constraint rather than describing free-form UI copy, the truncation is
## corrected here to the character class the English source (and the real
## validation rule) describes, rather than kept as a verbatim upstream typo.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in it (API, GPU, CPU,
## Whisper, URL, TXT/SRT/VTT) are left alone here too; "Backend" and "Privacy"
## are additionally left as standard Italian tech/software usage, the same
## way de/es keep their own locale-appropriate loanwords.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Inglese
locale-name-ca-es = Catalano
locale-name-da-dk = Danese
locale-name-nl = Olandese
locale-name-de-de = Tedesco
locale-name-es-es = Spagnolo
locale-name-it-it = Italiano
locale-name-ja-jp = Giapponese
locale-name-pl-pl = Polacco
locale-name-pt-br = Portoghese (Brasiliano)
locale-name-ru = Russo
locale-name-zh-cn = Cinese (semplificato)

## Empty-state hero (views/main_window.rs)

hero-drop = Trascina qui i file multimediali, oppure ⌘O
hero-import-button = Importa file…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Nome file / URL
table-col-model = Modello
table-col-task = Compito
table-col-status = Stato
table-col-date-completed = Data completata
table-col-date-added = Data aggiunta
table-col-notes = Note

## Task table — header/row context menus

table-menu-reset-column-order = Ripristina ordine colonne
table-menu-restart = Riavvia trascrizione
table-menu-rename = Rinomina
table-menu-notes = Aggiungi/modifica note

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Trascrivere
task-translate = Traduci in inglese

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = isolamento del parlato

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Registra
toolbar-new-file = Nuova trascrizione del file
toolbar-new-url = Nuova trascrizione URL
toolbar-open-transcript = Apri trascrizione
toolbar-open-transcript-tooltip = Seleziona una o più trascrizioni completate da aprire
toolbar-cancel = Annulla trascrizione
toolbar-clear = Elimina la cronologia

## Menu bar (views/main_window.rs, F-004)

menu-file = File
menu-file-import-file = Importa file…
menu-file-import-url = Importa URL…
menu-file-import-folder = Importa cartella…
menu-help-title = Aiuto
menu-help-about = Informazioni su humm
menu-help-docs = Aiuto
menu-help-docs-tooltip = La documentazione arriverà con la prima versione
menu-help-preferences = Preferenze…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Elimina la cronologia
clear-confirm-body = Sei certo di voler eliminare le trascrizioni selezionate? Questa azione non può essere annullata.
clear-confirm-ok = Ok
clear-confirm-cancel = Annulla
clear-error = Impossibile eliminare le trascrizioni selezionate

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Rinomina trascrizione
rename-label = Inserisci nuovo nome:
rename-error = Impossibile rinominare
notes-title = Note
notes-label = Inserisci alcune note rilevanti per questa trascrizione:
notes-error = Impossibile salvare le note
dialog-ok = Ok
dialog-cancel = Annulla

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Trascrizione completata
toast-completed-suffix = file scritti
toast-canceled = Trascrizione annullata
toast-detail-title = Trascrizione non riuscita
toast-detail-open-log = Apri la cartella dei log
toast-detail-retry = Riprova
toast-detail-close = Chiudi
retry-error-unavailable = Impossibile riavviare: le opzioni salvate non sono disponibili

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm non è riuscito a registrare gli aggiornamenti della cronologia — le esportazioni vengono comunque completate; consulta il file di log

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Rilascia per importare

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp mancante — installalo (ad es. `brew install yt-dlp`) oppure imposta HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Importa URL
url-dialog-label = URL:
url-dialog-placeholder = https://esempio.com/audio.mp3
url-dialog-invalid = L'URL inserito non è valido.
url-dialog-ok = Ok
url-dialog-cancel = Annulla

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Modello:
options-task-label = Compito:
options-language-label = Lingua:
language-detect = Rileva la lingua
options-word-timestamps = Tempistiche a livello di parola
options-extract-speech = Estrai il parlato
options-extract-speech-missing-tooltip = demucs non trovato — installalo (ad es. `pipx install demucs`) per abilitare l'estrazione del parlato
options-export-label = Esporta:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = nessuna esportazione selezionata
options-advanced = Avanzate
options-initial-prompt-label = Domanda iniziale:
options-hf-repo-label = Modello Hugging Face
options-run = Avvia

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Chiave API:
options-llm-heading = Impostazioni di traduzione
options-llm-enable = Abilita la traduzione AI
options-llm-model-label = Modello AI:
options-llm-model-presets-button = Preimpostazioni
options-llm-prompt-label = Istruzioni per l'AI:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Monitoraggio di { $name }
folderwatch-pill-paused = In pausa
folderwatch-pill-pause-tip = Metti in pausa il monitoraggio della cartella
folderwatch-pill-resume-tip = Riprendi il monitoraggio della cartella

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Aggiornamento Disponibile
update-dialog-title = Aggiornamento Disponibile
update-dialog-header = È disponibile una nuova versione di humm!
update-dialog-current-version = Versione attuale:
update-dialog-new-version = Nuova versione:
update-dialog-release-notes = Note di rilascio:
update-dialog-open-release-page = Apri la pagina di rilascio
update-dialog-close = Chiudi

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Versione
about-check-button = Controlla gli aggiornamenti
about-show-logs-button = Mostra log
about-close-button = Chiudi
about-up-to-date = Il programma è aggiornato!
about-check-failed = Il controllo degli aggiornamenti non è riuscito — consulta il file di log

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Preferenze
prefs-tab-general = Generale
prefs-tab-models = Modelli
prefs-tab-shortcuts = Scorciatoie
prefs-tab-folder-watch = Guarda cartella
prefs-tab-postproc = Post-elaborazione
prefs-tab-privacy = Privacy

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Lingua UI:
prefs-general-font-size-label = Dimensione del carattere
prefs-general-font-size-custom-label = Dimensione del carattere personalizzata
prefs-general-openai-key-label = Chiave API OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Mostra
prefs-general-key-hide = Nascondi
prefs-general-test-button = Test
prefs-general-test-title = Test della chiave API OpenAI
prefs-general-test-success-body = La tua chiave API è valida. humm utilizzerà questa chiave per eseguire le trascrizioni API Whisper e le traduzioni AI.
prefs-general-invalid-key-title = Chiave API non valida
prefs-general-invalid-key-body = L'API supporta solo caratteri base64 (A-Za-z0-9+/=_-). Altri caratteri nella chiave API potrebbero causare errori.
prefs-general-key-save-error-title = Errore
prefs-general-key-save-error-body = Impossibile salvare la chiave API OpenAI nel portachiavi
prefs-general-base-url-label = URL di base di OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Modello API OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Nome file di esportazione predefinito
prefs-general-force-cpu-row-label = Disabilita GPU
prefs-general-force-cpu-label = Utilizza solo la CPU e disattiva l'accelerazione GPU
prefs-general-force-cpu-tooltip = Imposta questa opzione se i modelli più grandi non si adattano alla memoria della tua GPU e humm si blocca

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Invia telemetria di avvio anonima
privacy-telemetry-disabled-caption = Questa build ha la telemetria disabilitata in fase di compilazione.
privacy-telemetry-items-intro = Ogni evento di avvio include:
privacy-telemetry-item-app-version = Versione dell'app
privacy-telemetry-item-system-name = Nome del sistema
privacy-telemetry-item-os-version = Versione del sistema operativo
privacy-telemetry-item-kernel-release = Versione del kernel
privacy-telemetry-item-cpu-architecture = Architettura della CPU
privacy-telemetry-item-system-locale = Impostazioni internazionali del sistema
privacy-telemetry-item-identifier = Un identificatore anonimo casuale
privacy-update-checkbox = Controlla automaticamente gli aggiornamenti

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Scaricato
models-section-available = Disponibile per il download
models-download-button = Download
models-delete-button = Elimina
models-show-location-button = Mostra la posizione del file
models-cancel-button = Annulla
models-custom-url-label = URL personalizzato del modello ggml:
models-delete-confirm-title = Elimina modello
models-delete-confirm-body = Sei sicuro di voler eliminare il modello selezionato?
models-download-error-title = Errore
models-download-failed = Download non riuscito
models-speed-badge = { $ratio }× tempo reale
models-speed-badge-tip = Velocità in tempo reale misurata dalle sessioni di registrazione live su questo computer

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Apri finestra di registrazione
shortcut-desc-import-file = Importa file
shortcut-desc-import-url = Importa URL
shortcut-desc-open-preferences-window = Apri la finestra delle preferenze
shortcut-desc-view-transcript-text = Visualizza il testo della trascrizione
shortcut-desc-view-transcript-translation = Visualizza la trascrizione della traduzione
shortcut-desc-view-transcript-timestamps = Visualizza i timestamp della trascrizione
shortcut-desc-search-transcript = Cerca trascrizione
shortcut-desc-search-next = Vai al risultato della ricerca della trascrizione successiva
shortcut-desc-search-previous = Vai al risultato della ricerca della trascrizione precedente
shortcut-desc-scroll-to-current-text = Scorri fino al testo corrente
shortcut-desc-play-pause-audio = Riproduci/Pausa audio
shortcut-desc-replay-current-segment = Riproduci il segmento corrente
shortcut-desc-toggle-playback-controls = Attiva/disattiva i controlli di riproduzione
shortcut-desc-decrease-segment-start = Riduci l'ora di inizio del segmento
shortcut-desc-increase-segment-start = Aumenta l'ora di inizio del segmento
shortcut-desc-decrease-segment-end = Diminuisci l'ora di fine del segmento
shortcut-desc-increase-segment-end = Aumenta l'ora di fine del segmento
shortcut-desc-clear-history = Elimina la cronologia
shortcut-desc-stop-transcription = Annulla trascrizione
prefs-shortcuts-press = Premi la scorciatoia…
prefs-shortcuts-clear-tooltip = Ripristina il valore predefinito
prefs-shortcuts-conflict = In conflitto con { $other }
prefs-shortcuts-reset = Ripristina impostazioni predefinite

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Abilita controllo cartelle
prefs-fw-input-folder-label = Cartella di input
prefs-fw-input-folder-placeholder = /percorso/della/cartella/di/input
prefs-fw-output-folder-label = Cartella di output
prefs-fw-output-folder-placeholder = /percorso/della/cartella/di/output
prefs-fw-browse = Sfoglia
prefs-fw-delete-checkbox = Elimina file elaborati
prefs-fw-disposition-note = Una volta trascritto, il file di origine viene spostato nella cartella di output — oppure eliminato, se l'opzione Elimina file elaborati è selezionata.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Questi hook vengono eseguiti automaticamente al termine di ogni trascrizione. Un errore dell'hook viene registrato nel log, ma non fa mai fallire la trascrizione stessa.
prefs-postproc-skip-title = Salta i già trascritti
prefs-postproc-skip-enable = Abilita Salta i già trascritti
prefs-postproc-skip-check-files = Cerca file di risultato esistenti
prefs-postproc-skip-check-files-desc = Salta se accanto al file audio viene trovato un file .txt, .srt o .vtt con lo stesso nome
prefs-postproc-skip-check-db = Controlla nel database delle trascrizioni
prefs-postproc-skip-check-db-desc = Salta se questo file è già stato trascritto e il record esiste nel database
prefs-postproc-dfn-title = Riduzione del rumore DeepFilterNet
prefs-postproc-dfn-enable = Abilita la riduzione del rumore DeepFilterNet
prefs-postproc-dfn-keep-file = Mantieni il file denoisato dopo la trascrizione
prefs-postproc-langdetect-title = Rilevamento lingua avanzato
prefs-postproc-langdetect-enable = Abilita il rilevamento lingua avanzato
prefs-postproc-langdetect-download-tiny = Scarica il modello tiny se non ne è disponibile nessuno
prefs-postproc-langdetect-download-tiny-desc = Se non è stato scaricato alcun modello whisper.cpp, scarica il modello tiny da usare per il rilevamento della lingua.
prefs-postproc-resizer-title = Ridimensionamento automatico della trascrizione
prefs-postproc-resizer-enable = Abilita il ridimensionamento automatico della trascrizione
prefs-postproc-resizer-merge-by-gap = Unisci per intervallo
prefs-postproc-resizer-merge-gap-seconds = Intervallo di unione (secondi)
prefs-postproc-resizer-split-by-punctuation = Dividi per punteggiatura
prefs-postproc-resizer-punctuation = Punteggiatura
prefs-postproc-resizer-split-by-max-length = Dividi per lunghezza massima
prefs-postproc-resizer-max-length = Lunghezza massima del sottotitolo
prefs-postproc-summary-title = Riepilogo IA
prefs-postproc-summary-enable = Abilita il riepilogo IA
prefs-postproc-summary-api-url = URL base API
prefs-postproc-summary-api-key-label = Chiave API
prefs-postproc-summary-model = Modello
prefs-postproc-summary-prompt = Prompt di riepilogo
prefs-postproc-summary-save-to-notes = Salva il riepilogo nelle note
prefs-postproc-summary-save-to-file = Salva il riepilogo in un file
prefs-postproc-summary-output-folder = Cartella di output (per il file)
prefs-postproc-summary-key-save-error-body = Impossibile salvare la chiave API del Riepilogo IA nel portachiavi
prefs-postproc-docx-title = Esporta in DOCX
prefs-postproc-docx-enable = Abilita l'esportazione in DOCX
prefs-postproc-docx-output-folder = Cartella di output
prefs-postproc-docx-include-timestamps = Includi timestamp
prefs-postproc-output-folder-hint = Lascia vuoto per salvare accanto al file di origine.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Visualizza
viewer-mode-text = Testo
viewer-mode-translation = Traduzione
viewer-mode-timestamps = Timestamp
viewer-export = Esporta
viewer-translate = Tradurre
viewer-resize = Ridimensionare
viewer-identify-speakers = Identificare i relatori
viewer-find = Trova
viewer-find-label = Trova:
viewer-find-placeholder = Inserisci il testo per trovare...
viewer-find-clear = Elimina
viewer-find-prev-tooltip = Corrispondenza precedente ({ $shortcut })
viewer-find-next-tooltip = Prossima corrispondenza ({ $shortcut })
viewer-empty = Nessun segmento — l'input potrebbe contenere solo silenzio
viewer-open-error = Impossibile aprire la trascrizione
viewer-col-start = Inizio
viewer-col-end = Fine
viewer-col-text = Testo
viewer-col-translation = Traduzione
viewer-saved = modificato · salvato
viewer-unsaved-warning = Una modifica non è stata salvata — viene mantenuta in memoria ma non scritta su disco
viewer-player-loading = Caricamento audio…
viewer-player-failed = Riproduzione non disponibile
viewer-playback-controls = Controlli di riproduzione:
viewer-loop-segment = Ciclo di segmento
viewer-loop-segment-tooltip = Abilita/disabilita il loop quando si fa clic sui segmenti della trascrizione
viewer-follow-audio = Segui Audio
viewer-follow-audio-tooltip = Abilita/disabilita la lettura della posizione audio corrente nella trascrizione. Quando abilitato, scorre automaticamente fino al testo corrente.
viewer-speed = Velocità:
viewer-scroll-to-current = Scorri fino al Corrente
viewer-scroll-to-current-tooltip = Scorrere fino al testo attualmente pronunciato

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Chiave API richiesta
viewer-translate-key-required-body = Inserisci la chiave API OpenAI nelle preferenze
viewer-translate-settings-title = Impostazioni di traduzione AI

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Testo
viewer-export-translation = Traduzione
viewer-export-dialog-title = Salva file
viewer-export-filter = File di testo
viewer-export-failed = Impossibile scrivere l'esportazione — consulta il log per i dettagli
export-written = Scritto in { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Impostazioni
resizer-create-new = Crea nuova trascrizione
resizer-extend-heading = Estendi l'orario di fine
resizer-extend-label = Estendi le terminazioni fino a (secondi)
resizer-extend-button = Estendere i finali
resizer-resize-heading = Opzioni di ridimensionamento
resizer-resize-label = Lunghezza desiderata dei sottotitoli
resizer-resize-button = Ridimensionare
resizer-resize-disabled-tooltip = Disponibile solo se i tempi a livello di parola sono stati disabilitati durante la trascrizione
resizer-merge-heading = Opzioni di unione
resizer-merge-by-gap = Unisci per intervallo
resizer-split-by-punctuation = Dividi per punteggiatura
resizer-split-by-max-length = Dividi per lunghezza massima
resizer-merge-button = Unione
resizer-merge-disabled-tooltip = Disponibile solo se i tempi a livello di parola sono stati abilitati durante la trascrizione
resizer-merging = Unione in corso…
resizer-merge-error = Impossibile eseguire l'unione:
resizer-save-error = Impossibile salvare le modifiche:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Fase 1: identificare i parlanti
speaker-id-step2-heading = Fase 2: nomi dei parlanti
speaker-id-ready-label = Pronto a identificare i parlanti
speaker-id-missing-audio-label = File audio non trovato
speaker-id-identify-button = Identificare
speaker-id-cancel-button = Annulla
speaker-id-cancelling = Annullamento in corso...
speaker-id-decoding = Decodifica audio…
speaker-id-downloading-models = Download dei modelli…
speaker-id-identifying = Identificazione dei parlanti…
speaker-id-play-sample = Ascolta il campione
speaker-id-merge-checkbox = Unisci le frasi del parlante
speaker-id-save-button = Salva
speaker-id-save-error = Impossibile salvare le modifiche:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Registrazione in diretta
recording-model-load-note = Il caricamento di un modello al primo utilizzo potrebbe richiedere diversi minuti.
recording-advanced-button = Avanzate...
recording-microphone-label = Microfono:
recording-record-button = Registra
recording-stop-button = Arresta
recording-realtime-warning = { $size } ha trascritto a { $ratio }× tempo reale nell'ultima esecuzione — la trascrizione live potrebbe rimanere indietro
recording-chunk-skipped-warning = Un blocco non è riuscito ed è stato saltato: { $detail }
recording-panel-placeholder = Fai clic su Registra per iniziare...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = In attesa della traduzione AI...
recording-model-downloading = Download del modello… { $percent }%
recording-model-downloading-indeterminate = Download del modello…
recording-model-loading = Caricamento del modello…
recording-error-prefix = Si è verificato un errore durante l'avvio della nuova registrazione:
recording-error-suffix = Controlla i tuoi dispositivi audio o i registri dell'applicazione per maggiori informazioni.
recording-thread-crashed = La sessione di registrazione è terminata in modo imprevisto. Controlla i registri dell'applicazione per maggiori informazioni.
meter-average-volume = Volume medio
meter-queue = Coda

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Impostazion avanzate
recording-advanced-speech-heading = Impostazioni di riconoscimento vocale
recording-advanced-initial-prompt-label = Domanda iniziale:
recording-advanced-translation-heading = Impostazioni di traduzione
recording-advanced-llm-enable = Abilita la traduzione AI
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Impostazioni di registrazione
recording-advanced-silence-threshold-label = Soglia del silenzio:
recording-advanced-mode-label = Modalità di registrazione in diretta:
recording-advanced-mode-append-below = Aggiungere sotto
recording-advanced-mode-append-above = Aggiungere sopra
recording-advanced-mode-append-and-correct = Aggiungere e correggere
recording-advanced-separator-label = Separatore di riga:
recording-advanced-step-label = Passo di trascrizione:
recording-advanced-hide-unconfirmed = Nascondi non confermato
recording-advanced-export-enable = Abilita l'esportazione della registrazione live
recording-advanced-export-folder-label = Cartella di esportazione:
recording-advanced-export-browse = Sfoglia
recording-advanced-export-file-name-label = Nome del file di esportazione:
recording-advanced-export-file-type-label = Tipo di file di esportazione:
recording-advanced-export-file-type-txt = File di testo (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Limita le voci di esportazione
    (0 = esporta tutto):
recording-advanced-upload-url-label = URL di caricamento:
recording-advanced-ok = Ok

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Mostra
presentation-show-tooltip = Mostra in una nuova finestra
presentation-text-size-label = Dimensione testo:
presentation-theme-label = Tema
presentation-theme-light = Chiaro
presentation-theme-dark = Scuro
presentation-theme-custom = Personalizzato
presentation-text-color-label = Colore del testo
presentation-background-color-label = Colore dello sfondo
presentation-fullscreen-button = Schermo intero
presentation-window-title = Presentazione con trascrizione in diretta

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Copia
recording-copy-tooltip = Copia la trascrizione negli appunti
recording-copy-copied = Copiato!
recording-copy-nothing = Nessun testo da copiare!

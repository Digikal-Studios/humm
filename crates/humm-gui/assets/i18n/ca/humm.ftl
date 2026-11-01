## humm-gui — Catalan (ca) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b, design spec §9.2). Section
## comments stay in English on purpose: they are developer-facing notes about
## *where* a key renders, identical in every catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own ca_ES translation is used
## verbatim (`buzz/locale/ca_ES/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh. Buzz's own
## ca_ES po has no formal/informal header marker and mixes register itself:
## short button/label strings use plain singular imperative/infinitive verbs
## (Desa, Suprimeix, Cerca, Selecciona…), while full sentences and questions
## use the vós/vosaltres conjugation as the polite address form (Establiu,
## Introduïu, Comproveu, Esteu segur, Voleu…) — the house style of
## Softcatalà, Buzz's own ca_ES translator (credited in the po header).
## Verbatim Buzz strings keep whichever form Buzz used rather than being
## harmonized; fresh humm-only strings follow the same split (short
## imperative for buttons, vós for sentences) for internal consistency.
##
## "Ok" collapse: Buzz's own ca_ES po translates the relabeled-dialog msgid
## `"Ok"` as "D'acord" (a single translation, not casing-sensitive — Catalan
## has no "Ok"/"OK" casing distinction). Following the es/zh-CN precedent, all
## four of clear-confirm-ok/dialog-ok/url-dialog-ok/recording-advanced-ok
## collapse to "D'acord" here — the en catalog's deliberate Ok/OK casing split
## is Buzz's own *English* Qt lineage and does not survive translation.
##
## Two mechanical corrections to the Buzz source, both upstream artifacts
## rather than translation choices: (1) six Buzz msgids carry two merged
## translation variants concatenated with no separator, an upstream Buzz
## artifact seen across several locales — of which ca_ES's po is affected by
## four (`prefs-general-force-cpu-tooltip`, `recording-model-load-note`,
## `resizer-resize-disabled-tooltip`, `resizer-merge-disabled-tooltip`); each
## takes the second (revised) variant alone. The other two
## (`viewer-loop-segment-tooltip`, `prefs-general-key-save-error-body`) are
## clean (single variant) in ca_ES's po and are used as-is. (2) Strings
## naming the product "Buzz" are re-pointed at "humm", matching the en
## catalog's own substitution.
##
## Additional ca_ES-specific issues found and corrected while porting (not
## part of the six above, and independently verified against es/de's own po
## renderings of the same msgids): `prefs-general-invalid-key-body`'s Buzz
## msgstr garbles the base64 character class to "(A-Za-z0-9+/).-)" (missing
## "=_", stray punctuation) where es/de's own po render it correctly as
## "(A-Za-z0-9+/=_-)" — corrected to the actual character class here.
## `viewer-mode-timestamps`'s Buzz msgstr has a plain spelling typo, "Marqua
## de temps" for "Marca de temps" — corrected. Two plugin description
## msgids (behind `prefs-postproc-skip-check-files-desc` and
## `prefs-postproc-skip-check-db-desc`) have empty ca_ES msgstrs in Buzz's own
## po (left untranslated upstream) — translated fresh here instead.
## `options-hf-repo-label` has no matching Buzz msgid at all (Buzz's closest
## string mistranslates the brand name "Huggingface" itself as "la cara
## oculta", i.e. literally as the English words "hugging face") — translated
## fresh, keeping "Hugging Face" as an untranslated proper noun, matching the
## es/de precedent catalogs' own choice. `recording-advanced-mode-label` is
## sourced from a now-`#~`-obsolete-but-exact-matching msgid in the po
## (Buzz's current source no longer emits the colon-suffixed form, but the
## archived translation is still the correct byte-verbatim Catalan text for
## this exact English string, and likewise for `toast-detail-close`/
## `about-close-button`/`update-dialog-close`, sourced from the po's own
## obsolete "Close" → "Tanca" entry).
##
## Product name "humm" is never translated. Technical terms Buzz itself
## leaves untranslated in ca (API, GPU, CPU, Whisper, URL, TXT/SRT/VTT,
## Backend) are left alone here too; "Hugging Face" is likewise kept as an
## untranslated proper noun (see above).

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Anglès
locale-name-ca-es = Català
locale-name-da-dk = Danès
locale-name-nl = Holandès
locale-name-fr = Francés
locale-name-de-de = Alemany
locale-name-es-es = Castellà
locale-name-it-it = Italià
locale-name-ja-jp = Japonès
locale-name-lv-lv = Letó
locale-name-pl-pl = Polonès
locale-name-pt-br = Portuguès (Brasil)
locale-name-ru = Rus
locale-name-tr-tr = Turc
locale-name-uk-ua = Ucraïnès
locale-name-zh-cn = Xinès (simplificat)
locale-name-zh-tw = Xinès (Tradicional)

## Empty-state hero (views/main_window.rs)

hero-drop = Deixeu anar contingut multimèdia aquí, o %shortcut
hero-import-button = Importa fitxers…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Nom del fitxer / URL
table-col-model = Model
table-col-task = Tasca
table-col-status = Estat
table-col-date-completed = Data de finalització
table-col-date-added = Data d'addició
table-col-notes = Notes

## Task table — header/row context menus

table-menu-reset-column-order = Restableix l'ordre de les columnes
table-menu-restart = Reinicia la transcripció
table-menu-rename = Canvia el nom
table-menu-notes = Afegeix/Edita notes

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transcriure
task-translate = Traduir a l'anglès

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = aïllant la veu

## Status words and notices added with the UX review fixes.
status-queued = En cua
status-canceled = Cancel·lat
status-skipped = Omes
status-completed = Completat
status-failed = Ha fallat
status-in-progress = En curs
status-eta = Temps restant
status-downloading-model = S'està baixant el model
status-downloading-media = S'estan baixant els mitjans
status-reducing-noise = reduint el soroll
table-menu-show-error = Mostra l'error
toast-written-to = Escrit a
toast-postproc-written = Fitxers addicionals escrits
toast-postproc-errors = Un procés posterior ha informat d'un error. La transcripció continua completa.
toast-reduction-skipped = La reducció de soroll no s'ha executat; la transcripció ha continuat.
options-word-timestamps-tooltip = Redimensionar només està disponible més tard si els temps per paraula estaven desactivats durant la transcripció. Fusionar només està disponible més tard si els temps per paraula estaven activats.
options-model-will-download = Executar baixa primer aquest model. La finestra continua utilitzable.
prefs-postproc-dfn-runs-before = DeepFilterNet s'executa abans de la transcripció i substitueix l'àudio que sent el model.
prefs-postproc-dfn-keep-name = Conservar el fitxer sense soroll deixa un fitxer anomenat <name>_DeepFilterNet3.wav al costat de la font.
prefs-general-export-template-tokens-intro = Els espais dins de les claus formen part de cada testimoni.
prefs-general-export-template-model-type = model_type és sempre Whisper.cpp.
prefs-general-cookie-label = Fitxer de galetes
prefs-general-cookie-browse = Navega…
prefs-general-cookie-caption = HUMM_DOWNLOAD_COOKIEFILE té prioritat sobre aquest camp quan està definit. Deixeu el camp buit per esborrar-lo.
prefs-saved = Desat
prefs-save-failed = No s'han pogut desar les preferències:
viewer-saved-neighbor = s'ha ajustat un senyal veí
viewer-timestamp-unparsed = S'ha conservat el temps anterior. Allò no era un temps.
viewer-timestamp-range = S'ha conservat el temps anterior. L'inici ha de ser abans del final.
speaker-id-save-copies = Desar copia aquesta transcripció en una fila nova completada. L'original es queda com està.
speaker-id-merge-tooltip = Les línies consecutives del mateix parlant s'uneixen.
speaker-id-takes-minutes = Això pot trigar uns quants minuts. Cancel·lar descarta l'execució.
already-running = humm ja s'està executant.
already-running-with-files = humm ja s'està executant. Els fitxers passats a aquest llançament no s'han obert. Importeu-los des de la finestra que ja és oberta.
banner-buzz-imported = S'han importat %n transcripcions de Buzz.
banner-buzz-import-failed = No s'han pogut importar les transcripcions de Buzz. humm ho tornarà a provar la propera vegada que s'iniciï.

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Enregistra
toolbar-new-file = Nova transcripció de fitxers
toolbar-new-url = Nova transcripció d'URL
toolbar-open-transcript = Obre una transcripció
toolbar-open-transcript-tooltip = Seleccioneu una o més transcripcions completades per obrir-les
toolbar-cancel = Cancel·la la transcripció
toolbar-clear = Neteja l'historial

## Menu bar (views/main_window.rs, F-004)

menu-file = Fitxer
menu-file-import-file = Importa fitxer…
menu-file-import-url = Importa l'URL…
menu-file-import-folder = Importa carpeta…
menu-help-title = Ajuda
menu-help-about = Quant a humm
menu-help-docs = Ajuda
menu-help-docs-tooltip = No hi ha cap pàgina d'ajuda dins l'aplicació.
menu-help-preferences = Preferències…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Neteja l'historial
clear-confirm-body = Esteu segur que voleu suprimir les transcripcions seleccionades? Aquesta acció no es pot desfer.
clear-confirm-ok = D'acord
clear-confirm-cancel = Cancel·lar
clear-error = No s'han pogut suprimir les transcripcions seleccionades

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Canvia el nom de la transcripció
rename-label = Introduïu el nou nom:
rename-error = No s'ha pogut canviar el nom
notes-title = Notes
notes-label = Introduïu algunes notes rellevants per a aquesta transcripció:
notes-error = No s'han pogut desar les notes
dialog-ok = D'acord
dialog-cancel = Cancel·lar

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transcripció completada
toast-completed-suffix = fitxer(s) escrit(s)
toast-canceled = Transcripció cancel·lada
toast-detail-title = La transcripció ha fallat
toast-detail-open-log = Obre la carpeta de registres
toast-detail-retry = Reintenta
toast-detail-close = Tanca
retry-error-unavailable = No s'ha pogut reiniciar: les opcions desades no estan disponibles

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm no ha pogut registrar les actualitzacions de l'historial — les exportacions es continuen completant; consulteu el fitxer de registre

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Deixeu anar per importar

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = falta yt-dlp — instal·leu-lo (p. ex. `brew install yt-dlp`) o definiu HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = URL d'importació
url-dialog-label = URL:
url-dialog-placeholder = https://exemple.com/audio.mp3
url-dialog-invalid = L'URL que heu introduït no és vàlid.
url-dialog-ok = D'acord
url-dialog-cancel = Cancel·lar

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Model:
options-task-label = Tasca:
options-language-label = Idioma:
language-detect = Detecta l'idioma
options-word-timestamps = Temps amb granularitat de paraula
options-extract-speech = Extreu la veu
options-extract-speech-missing-tooltip = no s'ha trobat demucs — instal·leu-lo (p. ex. `pipx install demucs`) per habilitar l'extracció de veu
options-export-label = Exporta:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = cap exportació seleccionada
options-advanced = Avançat
options-initial-prompt-label = Pregunta inicial:
options-hf-repo-label = Model de Hugging Face
options-run = Executa

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Clau API:
options-llm-heading = Configuració de la traducció
options-llm-enable = Habilita la traducció de la IA
options-llm-model-label = Model d'IA:
options-llm-model-presets-button = Preajustos
options-llm-prompt-label = Instruccions per a la IA:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Vigilant { $name }
folderwatch-pill-paused = En pausa
folderwatch-pill-pause-tip = Posa en pausa la vigilància de carpetes
folderwatch-pill-resume-tip = Reprèn la vigilància de carpetes

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Actualització disponible
update-dialog-title = Actualització disponible
update-dialog-header = Hi ha una nova versió de humm disponible!
update-dialog-current-version = Versió actual:
update-dialog-new-version = Nova versió:
update-dialog-release-notes = Notes de la versió:
update-dialog-open-release-page = Obre la pàgina de la versió
update-dialog-close = Tanca

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Versió
about-check-button = Comprova si hi ha actualitzacions
about-show-logs-button = Mostra els registres
about-close-button = Tanca
about-up-to-date = Estàs al dia!
about-check-failed = La comprovació d'actualitzacions ha fallat — consulteu el fitxer de registre

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Preferències
prefs-tab-general = General
prefs-tab-models = Models
prefs-tab-shortcuts = Dreceres
prefs-tab-folder-watch = Vigila la carpeta
prefs-tab-postproc = Postprocessament
prefs-tab-privacy = Privadesa

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Idioma UI
prefs-general-font-size-label = Mida de la lletra
prefs-general-font-size-custom-label = Mida de la lletra personalitzada
prefs-general-openai-key-label = Clau de l'API d'OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Mostra
prefs-general-key-hide = Amaga
prefs-general-test-button = Prova
prefs-general-test-title = Prova de clau OpenAI API
prefs-general-test-success-body = La vostra clau API és vàlida. humm utilitzarà aquesta clau per realitzar transcripcions de l'API de Whisper i traduccions de la IA.
prefs-general-invalid-key-title = Clau API no vàlida
prefs-general-invalid-key-body = L'API només admet caràcters base64 (A-Za-z0-9+/=_-). Altres caràcters de la clau API poden causar errors.
prefs-general-key-save-error-title = Error
prefs-general-key-save-error-body = No s'ha pogut desar la clau OpenAI API a l'anell de claus
prefs-general-base-url-label = URL base d'OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Model de l'API d'OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Nom del fitxer d'exportació per defecte
prefs-general-force-cpu-row-label = Desactiva la GPU
prefs-general-force-cpu-label = Utilitza només la CPU i desactiveu l'acceleració de la GPU
prefs-general-force-cpu-tooltip = Establiu això si els models més grans no s'ajusten a la memòria de la GPU i humm es bloqueja

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Envia telemetria anònima d'inici
privacy-telemetry-disabled-caption = Aquesta compilació té la telemetria desactivada en temps de compilació.
privacy-telemetry-items-intro = Cada esdeveniment d'inici inclou:
privacy-telemetry-item-app-version = Versió de l'aplicació
privacy-telemetry-item-system-name = Nom del sistema
privacy-telemetry-item-os-version = Versió del sistema operatiu
privacy-telemetry-item-kernel-release = Versió del nucli
privacy-telemetry-item-cpu-architecture = Arquitectura de la CPU
privacy-telemetry-item-system-locale = Configuració regional del sistema
privacy-telemetry-item-identifier = Un identificador anònim aleatori
privacy-update-checkbox = Comprova si hi ha actualitzacions automàticament

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Descarregat
models-section-available = Disponible per descarregar
models-download-button = Descàrrega
models-delete-button = Suprimeix
models-show-location-button = Mostra la ubicació del fitxer
models-cancel-button = Cancel·lar
models-custom-url-label = URL del model ggml personalitzat:
models-delete-confirm-title = Suprimeix el model
models-delete-confirm-body = Esteu segur que voleu suprimir el model seleccionat?
models-download-error-title = Error
models-download-failed = Descàrrega fallida
models-speed-badge = { $ratio }× temps real
models-speed-badge-tip = Velocitat en temps real mesurada a partir de les sessions d'enregistrament en directe d'aquest equip

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Obre la finestra de registre
shortcut-desc-import-file = Importar arxiu
shortcut-desc-import-url = URL d'importació
shortcut-desc-open-preferences-window = Obre la finestra de preferències
shortcut-desc-view-transcript-text = Veure el text de la transcripció
shortcut-desc-view-transcript-translation = Veure la traducció de transcripció
shortcut-desc-view-transcript-timestamps = Veure les marques de temps de la transcripció
shortcut-desc-search-transcript = Cerca una transcripció
shortcut-desc-search-next = Ves al resultat de cerca de transcripció següent
shortcut-desc-search-previous = Ves al resultat de cerca de transcripció anterior
shortcut-desc-scroll-to-current-text = Desplaça't fins al text actual
shortcut-desc-play-pause-audio = Reproduir/posar en pausa l'àudio
shortcut-desc-replay-current-segment = Reprodueix el segment actual
shortcut-desc-toggle-playback-controls = Commuta els controls de reproducció
shortcut-desc-decrease-segment-start = Disminuir l'hora d'inici del segment
shortcut-desc-increase-segment-start = Augmenta l'hora d'inici del segment
shortcut-desc-decrease-segment-end = Disminueix l'hora de finalització del segment
shortcut-desc-increase-segment-end = Augmenta l'hora de finalització del segment
shortcut-desc-clear-history = Neteja l'historial
shortcut-desc-stop-transcription = Cancel·la la transcripció
prefs-shortcuts-press = Premeu una drecera…
prefs-shortcuts-clear-tooltip = Restableix al valor predeterminat
prefs-shortcuts-conflict = Entra en conflicte amb { $other }
prefs-shortcuts-reset = Restableix als valors predeterminats

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Habilita el seguiment de carpetes
prefs-fw-input-folder-label = Carpeta d'entrada
prefs-fw-input-folder-placeholder = /camí/a/la/carpeta/d'entrada
prefs-fw-output-folder-label = Carpeta de sortida
prefs-fw-output-folder-placeholder = /camí/a/la/carpeta/de/sortida
prefs-fw-browse = Navega
prefs-fw-delete-checkbox = Eliminar fitxers processats
prefs-fw-disposition-note = Un cop transcrit, el fitxer d'origen es mou a la carpeta de sortida — o se suprimeix, si Eliminar fitxers processats està marcat.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Ometre, la reducció de soroll i la detecció d'idioma s'executen abans de la transcripció. El redimensionador, el resum i l'exportació DOCX s'executen després. Una fallada es registra i mai fa fallar la transcripció.
prefs-postproc-skip-title = Omet els ja transcrits
prefs-postproc-skip-enable = Habilita Omet els ja transcrits
prefs-postproc-skip-check-files = Comprova si hi ha fitxers de resultats existents
prefs-postproc-skip-check-files-desc = Omet si es troba un fitxer .txt, .srt o .vtt amb el mateix nom que l'àudio al seu costat
prefs-postproc-skip-check-db = Comprovar a la base de dades de transcripcions
prefs-postproc-skip-check-db-desc = Omet si aquest fitxer ja s'ha transcrit i el registre existeix a la base de dades
prefs-postproc-dfn-title = Reducció de soroll DeepFilterNet
prefs-postproc-dfn-enable = Habilita la reducció de soroll DeepFilterNet
prefs-postproc-dfn-keep-file = Conserva el fitxer sense soroll després de la transcripció
prefs-postproc-langdetect-title = Detecció de llengua millorada
prefs-postproc-langdetect-enable = Habilita la detecció de llengua millorada
prefs-postproc-langdetect-download-tiny = Baixa el model tiny si no n'hi ha cap de disponible
prefs-postproc-langdetect-download-tiny-desc = Si no hi ha cap model de whisper.cpp baixat, baixa el model tiny per utilitzar-lo en la detecció de llengua.
prefs-postproc-resizer-title = Redimensionador automàtic de transcripcions
prefs-postproc-resizer-enable = Habilita el redimensionador automàtic de transcripcions
prefs-postproc-resizer-merge-by-gap = Combina per interval
prefs-postproc-resizer-merge-gap-seconds = Interval de combinació (segons)
prefs-postproc-resizer-split-by-punctuation = Divideix per puntuació
prefs-postproc-resizer-punctuation = Puntuació
prefs-postproc-resizer-split-by-max-length = Divideix per longitud màxima
prefs-postproc-resizer-max-length = Longitud màxima del subtítol
prefs-postproc-summary-title = Resum amb IA
prefs-postproc-summary-enable = Habilita el resum amb IA
prefs-postproc-summary-api-url = URL base de l'API
prefs-postproc-summary-api-key-label = Clau de l'API
prefs-postproc-summary-model = Model
prefs-postproc-summary-prompt = Indicació de resum
prefs-postproc-summary-save-to-notes = Desa el resum a les notes
prefs-postproc-summary-save-to-file = Desa el resum en un fitxer
prefs-postproc-summary-output-folder = Carpeta de sortida (per al fitxer)
prefs-postproc-summary-key-save-error-body = No s'ha pogut desar la clau de l'API del resum amb IA a l'anell de claus
prefs-postproc-docx-title = Exporta a DOCX
prefs-postproc-docx-enable = Habilita l'exportació a DOCX
prefs-postproc-docx-output-folder = Carpeta de sortida
prefs-postproc-docx-include-timestamps = Inclou marques de temps
prefs-postproc-output-folder-hint = Deixeu-ho buit per desar al costat del fitxer d'origen.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Veure
viewer-mode-text = Text
viewer-mode-translation = Traducció
viewer-mode-timestamps = Marca de temps
viewer-export = Exporta
viewer-translate = Traduir
viewer-resize = Redimensionar
viewer-identify-speakers = Identifica els parlants
viewer-find = Cerca
viewer-find-label = Cerca:
viewer-find-placeholder = Introduïu el text a cercar...
viewer-find-clear = Neteja
viewer-find-prev-tooltip = Coincidència anterior ({ $shortcut })
viewer-find-next-tooltip = Coincidència següent ({ $shortcut })
viewer-empty = Sense segments — és possible que l'entrada només contingui silenci
viewer-open-error = No s'ha pogut obrir la transcripció
viewer-col-start = Inicia
viewer-col-end = Finalitza
viewer-col-text = Text
viewer-col-translation = Traducció
viewer-saved = editat · desat
viewer-unsaved-warning = No s'ha pogut desar una edició — es manté a la memòria però no s'ha escrit al disc
viewer-player-loading = Carregant l'àudio…
viewer-player-failed = Reproducció no disponible
viewer-playback-controls = Controls de reproducció:
viewer-loop-segment = Segment de bucle
viewer-loop-segment-tooltip = Activa/desactiva el bucle en fer clic als segments de transcripció
viewer-follow-audio = Segueix l'àudio
viewer-follow-audio-tooltip = Activa/desactiva seguint la posició d'àudio actual a la transcripció. Quan està activada, es desplaça automàticament al text actual.
viewer-speed = Velocitat:
viewer-scroll-to-current = Desplaça't fins a l'actual
viewer-scroll-to-current-tooltip = Desplaçar-se fins al text que es parla actualment

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Clau API necessària
viewer-translate-key-required-body = Introduïu la clau API d'OpenAI a les preferències
viewer-translate-settings-title = Configuració de la traducció amb IA

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Text
viewer-export-translation = Traducció
viewer-export-dialog-title = Desa el fitxer
viewer-export-filter = Fitxers de text
viewer-export-failed = No s'ha pogut escriure l'exportació — consulteu el registre per obtenir més detalls
export-written = Escrit a { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Configuració
resizer-create-new = Crea una transcripció nova
resizer-extend-heading = Amplia el temps final
resizer-extend-label = Amplia els finals fins a (segons)
resizer-extend-button = Amplia els finals
resizer-resize-heading = Opcions de redimensionament
resizer-resize-label = Longitud desitjada dels subtítols
resizer-resize-button = Redimensionar
resizer-resize-disabled-tooltip = Disponible només si els temps a nivell de paraula estaven desactivats durant la transcripció
resizer-merge-heading = Opcions de fusió
resizer-merge-by-gap = Combina per interval
resizer-split-by-punctuation = Divideix per puntuació
resizer-split-by-max-length = Divideix per longitud màxima
resizer-merge-button = Fusiona
resizer-merge-disabled-tooltip = Disponible només si els temps a nivell de paraula estaven activats durant la transcripció
resizer-merging = Fusionant…
resizer-merge-error = No s'ha pogut executar la fusió:
resizer-save-error = No s'han pogut desar els canvis:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Pas 1: Identifica els parlants
speaker-id-step2-heading = Pas 2: Anomena els parlants
speaker-id-ready-label = Llest per identificar parlants
speaker-id-missing-audio-label = Fitxer d'àudio no trobat
speaker-id-identify-button = Identifica
speaker-id-cancel-button = Cancel·lar
speaker-id-cancelling = Cancel·lant...
speaker-id-decoding = Descodificant l'àudio…
speaker-id-downloading-models = Baixant models…
speaker-id-identifying = Identificant els parlants…
speaker-id-play-sample = Reprodueix la mostra
speaker-id-merge-checkbox = Fusiona les frases del parlant
speaker-id-save-button = Desa
speaker-id-save-error = No s'han pogut desar els canvis:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Enregistrament en directe
recording-model-load-note = L'ús per primera vegada d'un model pot trigar diversos minuts a carregar-se.
recording-advanced-button = Avançat...
recording-microphone-label = Micròfon:
recording-record-button = Enregistra
recording-stop-button = Atura
recording-realtime-warning = { $size } transcrit a { $ratio }× temps real en l'última execució — la transcripció en directe pot quedar-se enrere
recording-chunk-skipped-warning = Un fragment ha fallat i s'ha omès: { $detail }
recording-panel-placeholder = Feu clic a Enregistra per a començar...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Esperant la traducció de la IA...
recording-model-downloading = Descarregant el model… { $percent }%
recording-model-downloading-indeterminate = Descarregant el model…
recording-model-loading = Carregant el model…
recording-error-prefix = S'ha produït un error en iniciar un enregistrament nou:
recording-error-suffix = Comproveu els vostres dispositius d'àudio o els registres de l'aplicació per a més informació.
recording-thread-crashed = La sessió d'enregistrament ha finalitzat de manera inesperada. Comproveu els registres de l'aplicació per obtenir més informació.
meter-average-volume = Volum mitjà
meter-queue = Cua

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Configuració avançada
recording-advanced-speech-heading = Configuració del reconeixement de veu
recording-advanced-initial-prompt-label = Pregunta inicial:
recording-advanced-translation-heading = Configuració de la traducció
recording-advanced-llm-enable = Habilita la traducció de la IA
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Configuració d'enregistrament
recording-advanced-silence-threshold-label = Llindar de silenci:
recording-advanced-mode-label = Mode d'enregistrament en directe:
recording-advanced-mode-append-below = Afegeix a sota
recording-advanced-mode-append-above = Afegeix a sobre
recording-advanced-mode-append-and-correct = Afegeix i corregeix
recording-advanced-separator-label = Separador de línies:
recording-advanced-step-label = Pas de transcripció:
recording-advanced-hide-unconfirmed = Amaga el no confirmat
recording-advanced-export-enable = Activa l'exportació d'enregistrament en directe
recording-advanced-export-folder-label = Carpeta d'exportació:
recording-advanced-export-browse = Navega
recording-advanced-export-file-name-label = Nom del fitxer d'exportació:
recording-advanced-export-file-type-label = Tipus de fitxer d'exportació:
recording-advanced-export-file-type-txt = Fitxer de text (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Limita les entrades d'exportació
    (0 = exporta tot):
recording-advanced-upload-url-label = URL de pujada:
recording-advanced-ok = D'acord

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Mostra
presentation-show-tooltip = Mostra en una nova finestra
presentation-text-size-label = Mida del text:
presentation-theme-label = Tema
presentation-theme-light = Clar
presentation-theme-dark = Fosc
presentation-theme-custom = Personalitzat
presentation-text-color-label = Color del text
presentation-background-color-label = Color de fons
presentation-fullscreen-button = Pantalla completa
presentation-window-title = Presentació de transcripció en directe

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Copia
recording-copy-tooltip = Copia la transcripció al porta-retalls
recording-copy-copied = Copiat!
recording-copy-nothing = Res per copiar!

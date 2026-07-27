## humm-gui — Dutch (nl) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b, design spec §9.2, M8 Task
## 13). Section comments stay in English on purpose: they are developer-
## facing notes about *where* a key renders, identical in every catalog, not
## translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own nl translation is used
## verbatim (`buzz/locale/nl/LC_MESSAGES/buzz.po`) so terminology and muscle
## memory carry over; everything else is translated fresh in the same
## register. Buzz's own nl po carries no formal/informal header marker, but
## its translator consistently used the formal "u"/"uw" throughout (e.g.
## "Weet u zeker...", "in uw GPU-geheugen", "Controleer uw..."), never "je"
## — this catalog stays consistently formal to match, the same approach as
## de's own Sie-only register.
##
## Buzz's own locale code for Dutch is the bare `"nl"` (no region), matching
## `i18n::AVAILABLE_LOCALES` here — expected, not a mistake (Buzz's own
## `ui_locales` dict has several bare codes, e.g. `nl`/`ru`).
##
## The "Ok" collapse: Buzz's own nl po has a single msgid "Ok" (cited from
## import_url_dialog.py, preferences_dialog.py, models_preferences_widget.py,
## advanced_settings_dialog.py, plugins_dialog.py, plugin_settings_dialog.py,
## main_window.py — i.e. every one of Buzz's own relabeled-OK dialog
## buttons) translated once as "Oké". Unlike en's own lineage-based split
## between clear-confirm-ok's Buzz-relabeled "Ok" and dialog-ok's Qt-default
## "OK", Buzz's nl po has no separate translation for the Qt-default "OK"
## spelling, so all four keys — clear-confirm-ok, dialog-ok, url-dialog-ok,
## recording-advanced-ok — collapse to this same "Oké" here (same shape as
## es's own "Ok" collapse; see es's header).
##
## Six Buzz msgids carry two translation variants concatenated with no
## separator, an upstream Buzz-po artifact (not a translation choice) shared
## with de/es's own affected catalogs. Four of the six affect this catalog —
## `prefs-general-force-cpu-tooltip`, `recording-model-load-note`,
## `resizer-resize-disabled-tooltip`, `resizer-merge-disabled-tooltip` — each
## resolved to its second (revised) variant alone. The other two —
## `viewer-loop-segment-tooltip`, `prefs-general-key-save-error-body` — are
## clean (single variant, no concatenation) in nl's po and used as-is.
##
## A few more nl-po quirks worth flagging, none of them part of the six
## above: `prefs-postproc-skip-check-files-desc` and `prefs-postproc-skip-
## check-db-desc` have Buzz msgids but an *empty* nl msgstr (untranslated by
## Buzz itself) — translated fresh here instead. `prefs-postproc-dfn-keep-
## file`'s Buzz msgstr is marked `#, fuzzy` in the po (tooling flagged it as
## an uncertain match, not a translation error) and contains a soft hyphen
## inside "Geruis­verwijderd" — used byte-verbatim per the sourcing rule
## regardless, since it isn't one of the six concatenation artifacts above.
## `models-delete-button`'s Buzz string spells "Verwĳderen" with the Dutch
## ij-ligature character (U+0133), while `models-delete-confirm-title`'s Buzz
## string spells the same word normally ("verwijderen") — an inconsistency
## already present in Buzz's own po, preserved byte-verbatim in both places
## rather than harmonized. `toast-detail-close`/`about-close-button`/
## `update-dialog-close`'s "Sluiten" is sourced from an obsolete (`#~`) po
## entry ("Close" -> "Sluiten"), since no live nl entry exists for a
## standalone "Close" — still Buzz's own word choice, just a retired msgid.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution (update-dialog-header, prefs-general-test-
## success-body, prefs-general-force-cpu-tooltip, recording-model-load-note).
## Product name "humm" is never translated or declined. Technical terms
## Buzz's own nl po leaves untranslated (API, GPU, CPU, URL, TXT/SRT/VTT,
## Whisper) are left alone here too; "Backend"/"Whisper"/"OpenAI Whisper API"
## are likewise kept as standard Dutch tech loanwords, unchanged.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Engels
locale-name-ca-es = Catalaans
locale-name-da-dk = Deens
locale-name-de-de = Duits
locale-name-es-es = Spaans
locale-name-it-it = Italiaans
locale-name-nl = Nederlands
locale-name-zh-cn = Chinees (Vereenvoudigd)

## Empty-state hero (views/main_window.rs)

hero-drop = Sleep media hierheen, of ⌘O
hero-import-button = Bestanden importeren…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Bestandsnaam/Url
table-col-model = Model
table-col-task = Taak
table-col-status = Status
table-col-date-completed = Afgerond op
table-col-date-added = Toegevoegd op
table-col-notes = Notities

## Task table — header/row context menus

table-menu-reset-column-order = Kolomvolgorde herstellen
table-menu-restart = Transcriptie opnieuw starten
table-menu-rename = Hernoemen
table-menu-notes = Notities toevoegen/bewerken

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transcriberen
task-translate = Naar het Engels vertalen

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = Spraak isoleren

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Opnemen
toolbar-new-file = Nieuwe bestandstranscriptie
toolbar-new-url = Nieuwe url-transcriptie
toolbar-open-transcript = Transcriptie openen
toolbar-open-transcript-tooltip = Selecteer een of meer voltooide transcripties om te openen
toolbar-cancel = Transcriptie wissen
toolbar-clear = Geschiedenis wissen

## Menu bar (views/main_window.rs, F-004)

menu-file = Bestand
menu-file-import-file = Bestand importeren…
menu-file-import-url = Url importeren…
menu-file-import-folder = Map importeren…
menu-help-title = Hulp
menu-help-about = Over humm
menu-help-docs = Hulp
menu-help-docs-tooltip = Documentatie volgt bij de eerste release
menu-help-preferences = Instellingen…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Geschiedenis wissen
clear-confirm-body = Weet u zeker dat u de gekozen transcriptie(s) wilt verwijderen? Deze actie is onomkeerbaar.
clear-confirm-ok = Oké
clear-confirm-cancel = Annuleren
clear-error = De gekozen transcriptie(s) kon(den) niet worden verwijderd

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Transcriptie hernoemen
rename-label = Voer een nieuwe naam in:
rename-error = Hernoemen mislukt
notes-title = Notities
notes-label = Voer relevante notities in voor deze transcriptie:
notes-error = Notities konden niet worden opgeslagen
dialog-ok = Oké
dialog-cancel = Annuleren

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transcriptie afgerond
toast-completed-suffix = bestand(en) geschreven
toast-canceled = Transcriptie afgebroken
toast-detail-title = Transcriptie mislukt
toast-detail-open-log = Logmap openen
toast-detail-retry = Opnieuw proberen
toast-detail-close = Sluiten
retry-error-unavailable = Kon niet opnieuw starten: opgeslagen opties zijn niet beschikbaar

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm kon geschiedenisupdates niet vastleggen — exports worden nog steeds voltooid; zie het logbestand

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Sleep om te importeren

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp ontbreekt — installeer het (bijv. `brew install yt-dlp`) of stel HUMM_YTDLP in

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Url importeren
url-dialog-label = Url:
url-dialog-placeholder = https://voorbeeld.nl/audio.mp3
url-dialog-invalid = De ingevoerde url is ongeldig.
url-dialog-ok = Oké
url-dialog-cancel = Annuleren

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Model:
options-task-label = Taak:
options-language-label = Taal:
language-detect = Taal herkennen
options-word-timestamps = Woordherkenningstimings
options-extract-speech = Spraak extraheren
options-extract-speech-missing-tooltip = demucs niet gevonden — installeer het (bijv. `pipx install demucs`) om spraakextractie in te schakelen
options-export-label = Exporteren:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = geen export geselecteerd
options-advanced = Geavanceerd
options-initial-prompt-label = Hoofdinvoer:
options-hf-repo-label = Hugging Face-model
options-run = Uitvoeren

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Api-sleutel:
options-llm-heading = Vertaalinstellingen
options-llm-enable = AI-vertaling inschakelen
options-llm-model-label = AI-model:
options-llm-model-presets-button = Voorinstellingen
options-llm-prompt-label = AI-instructies:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Houdt { $name } bij
folderwatch-pill-paused = Gepauzeerd
folderwatch-pill-pause-tip = Map bijhouden pauzeren
folderwatch-pill-resume-tip = Map bijhouden hervatten

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Update Beschikbaar
update-dialog-title = Update Beschikbaar
update-dialog-header = Er is een nieuwe versie van humm beschikbaar!
update-dialog-current-version = Huidige versie:
update-dialog-new-version = Versie:
update-dialog-release-notes = Release-opmerkingen:
update-dialog-open-release-page = Releasepagina openen
update-dialog-close = Sluiten

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Versie
about-check-button = Controleren op updates
about-show-logs-button = Logboeken tonen
about-close-button = Sluiten
about-up-to-date = De software is actueel!
about-check-failed = De update-controle is mislukt — zie het logbestand

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Instellingen
prefs-tab-general = Algemeen
prefs-tab-models = Modellen
prefs-tab-shortcuts = Sneltoetsen
prefs-tab-folder-watch = Map bijhouden
prefs-tab-postproc = Nabewerking
prefs-tab-privacy = Privacy

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Programmataal
prefs-general-font-size-label = Tekstgrootte
prefs-general-font-size-custom-label = Aangepaste tekstgrootte
prefs-general-openai-key-label = OpenAI-api-sleutel
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Tonen
prefs-general-key-hide = Verbergen
prefs-general-test-button = Uitproberen
prefs-general-test-title = OpenAI-api-sleuteltest
prefs-general-test-success-body = De api-sleutel is geldig. humm zal deze sleutel gebruiken om transcripties en AI-vertalingen op te vragen bij Whisper.
prefs-general-invalid-key-title = Ongeldige api-sleutel
prefs-general-invalid-key-body = De api ondersteunt alleen base64-tekens (A–Za–z0–9+/=_-). Andere tekens kunnen problemen veroorzaken.
prefs-general-key-save-error-title = Foutmelding
prefs-general-key-save-error-body = De OpenAI-api-sleutel kan niet worden bewaard in de sleutelbos
prefs-general-base-url-label = OpenAI-hoofd-url
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI-api-model
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Standaardnaam van export
prefs-general-force-cpu-row-label = GPU uitschakelen
prefs-general-force-cpu-label = Alleen CPU gebruiken en GPU-versnelling uitschakelen
prefs-general-force-cpu-tooltip = Gebruik dit als grotere modellen niet in het GPU-geheugen passen en humm crasht

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Anonieme opstarttelemetrie verzenden
privacy-telemetry-disabled-caption = In deze build is telemetrie uitgeschakeld tijdens het compileren.
privacy-telemetry-items-intro = Elke opstartgebeurtenis bevat:
privacy-telemetry-item-app-version = App-versie
privacy-telemetry-item-system-name = Systeemnaam
privacy-telemetry-item-os-version = Besturingssysteemversie
privacy-telemetry-item-kernel-release = Kernelversie
privacy-telemetry-item-cpu-architecture = CPU-architectuur
privacy-telemetry-item-system-locale = Systeemlandinstelling
privacy-telemetry-item-identifier = Een willekeurige anonieme identificatie
privacy-update-checkbox = Automatisch controleren op updates

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Gedownload
models-section-available = Beschikbaar
models-download-button = Downloaden
models-delete-button = Verwĳderen
models-show-location-button = Bestandslocatie tonen
models-cancel-button = Annuleren
models-custom-url-label = Aangepaste ggml-model-url:
models-delete-confirm-title = Model verwijderen
models-delete-confirm-body = Weet u zeker dat u het gekozen model wilt verwijderen?
models-download-error-title = Foutmelding
models-download-failed = Het downloaden is mislukt
models-speed-badge = { $ratio }× realtime
models-speed-badge-tip = Gemeten realtime snelheid op basis van de live-opnamesessies op deze computer

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Opnamevenster openen
shortcut-desc-import-file = Bestand importeren
shortcut-desc-import-url = Url importeren
shortcut-desc-open-preferences-window = Instellingenvenster openen
shortcut-desc-view-transcript-text = Transcriptie bekijken
shortcut-desc-view-transcript-translation = Getranscribeerde vertaling bekijken
shortcut-desc-view-transcript-timestamps = Getranscribeerde tijdstippen bekijken
shortcut-desc-search-transcript = Transcriptie doorzoeken
shortcut-desc-search-next = Naar volgend zoekresultaat in transcriptie gaan
shortcut-desc-search-previous = Naar vorig zoekresultaat in transcriptie gaan
shortcut-desc-scroll-to-current-text = Naar huidige tekst scrollen
shortcut-desc-play-pause-audio = Audio afspelen/pauzeren
shortcut-desc-replay-current-segment = Huidig fragment opnieuw afspelen
shortcut-desc-toggle-playback-controls = Afspeelbediening in-/uitschakelen
shortcut-desc-decrease-segment-start = Begintijd van fragment verkleinen
shortcut-desc-increase-segment-start = Begintijd van fragment vergroten
shortcut-desc-decrease-segment-end = Eindtijd van fragment verkleinen
shortcut-desc-increase-segment-end = Eindtijd van fragment vergroten
shortcut-desc-clear-history = Geschiedenis wissen
shortcut-desc-stop-transcription = Transcriptie wissen
prefs-shortcuts-press = Druk op een sneltoets…
prefs-shortcuts-clear-tooltip = Terugzetten naar standaard
prefs-shortcuts-conflict = Conflicteert met { $other }
prefs-shortcuts-reset = Standaardwaarden

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Map bijhouden
prefs-fw-input-folder-label = Invoermap
prefs-fw-input-folder-placeholder = /pad/naar/invoermap
prefs-fw-output-folder-label = Uitvoermap
prefs-fw-output-folder-placeholder = /pad/naar/uitvoermap
prefs-fw-browse = Bladeren
prefs-fw-delete-checkbox = Verwerkte bestanden verwijderen
prefs-fw-disposition-note = Zodra de transcriptie is voltooid, wordt het bronbestand verplaatst naar de uitvoermap — of verwijderd, als Verwerkte bestanden verwijderen is aangevinkt.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Deze hooks worden automatisch uitgevoerd na elke voltooide transcriptie. Een mislukte hook wordt vastgelegd in het logboek, maar laat de transcriptie zelf nooit mislukken.
prefs-postproc-skip-title = Reeds getranscribeerde overslaan
prefs-postproc-skip-enable = Reeds getranscribeerde overslaan inschakelen
prefs-postproc-skip-check-files = Controleren op bestaande resultaatbestanden
prefs-postproc-skip-check-files-desc = Overslaan als er een .txt-, .srt- of .vtt-bestand met dezelfde bestandsnaam als het audiobestand ernaast wordt gevonden
prefs-postproc-skip-check-db = Controleren in de transcriptiedatabase
prefs-postproc-skip-check-db-desc = Overslaan als dit bestand al is getranscribeerd en het record in de database bestaat
prefs-postproc-dfn-title = DeepFilterNet ruisonderdrukking
prefs-postproc-dfn-enable = DeepFilterNet ruisonderdrukking inschakelen
prefs-postproc-dfn-keep-file = Geruis­verwijderd bestand bewaren na transcriptie
prefs-postproc-langdetect-title = Verbeterde taaldetectie
prefs-postproc-langdetect-enable = Verbeterde taaldetectie inschakelen
prefs-postproc-langdetect-download-tiny = Tiny-model downloaden als er geen beschikbaar is
prefs-postproc-langdetect-download-tiny-desc = Als er geen whisper.cpp-model is gedownload, wordt het tiny-model gedownload voor taaldetectie.
prefs-postproc-resizer-title = Automatisch transcriptie aanpassen
prefs-postproc-resizer-enable = Automatisch transcriptie aanpassen inschakelen
prefs-postproc-resizer-merge-by-gap = Samenvoegen op interval
prefs-postproc-resizer-merge-gap-seconds = Samenvoeginterval (seconden)
prefs-postproc-resizer-split-by-punctuation = Splitsen op interpunctie
prefs-postproc-resizer-punctuation = Interpunctie
prefs-postproc-resizer-split-by-max-length = Splitsen op maximale lengte
prefs-postproc-resizer-max-length = Maximale ondertitellengte
prefs-postproc-summary-title = AI-samenvatting
prefs-postproc-summary-enable = AI-samenvatting inschakelen
prefs-postproc-summary-api-url = API-basis-URL
prefs-postproc-summary-api-key-label = API-sleutel
prefs-postproc-summary-model = Model
prefs-postproc-summary-prompt = Samenvattingsprompt
prefs-postproc-summary-save-to-notes = Samenvatting in notities opslaan
prefs-postproc-summary-save-to-file = Samenvatting in een bestand opslaan
prefs-postproc-summary-output-folder = Uitvoermap (voor bestand)
prefs-postproc-summary-key-save-error-body = De API-sleutel voor AI-samenvatting kan niet worden bewaard in de sleutelbos
prefs-postproc-docx-title = Exporteren naar DOCX
prefs-postproc-docx-enable = Exporteren naar DOCX inschakelen
prefs-postproc-docx-output-folder = Uitvoermap
prefs-postproc-docx-include-timestamps = Tijdcodes opnemen
prefs-postproc-output-folder-hint = Laat leeg om naast het bronbestand op te slaan.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Bekijken
viewer-mode-text = Tekst
viewer-mode-translation = Vertaling
viewer-mode-timestamps = Tijdstippen
viewer-export = Exporteren
viewer-translate = Vertalen
viewer-resize = Grootte
viewer-identify-speakers = Sprekers identificeren
viewer-find = Zoeken
viewer-find-label = Zoeken:
viewer-find-placeholder = Voer te zoeken tekst in…
viewer-find-clear = Wissen
viewer-find-prev-tooltip = Vorige overeenkomst ({ $shortcut })
viewer-find-next-tooltip = Volgende overeenkomst ({ $shortcut })
viewer-empty = Geen fragmenten — de invoer bevat mogelijk alleen stilte
viewer-open-error = De transcriptie kon niet worden geopend
viewer-col-start = Begin
viewer-col-end = Einde
viewer-col-text = Tekst
viewer-col-translation = Vertaling
viewer-saved = bewerkt · opgeslagen
viewer-unsaved-warning = Een bewerking kon niet worden opgeslagen — deze blijft in het geheugen bewaard, maar is niet op schijf geschreven
viewer-player-loading = Audio laden…
viewer-player-failed = Afspelen niet beschikbaar
viewer-playback-controls = Afspeelbediening:
viewer-loop-segment = Fragment herhalen
viewer-loop-segment-tooltip = Herhaling in- of uitschakelen bij het klikken op transcriptfragmenten
viewer-follow-audio = Audio volgen
viewer-follow-audio-tooltip = De huidige audiopositie in de transcriptie in- of uitschakelen. Wanneer ingeschakeld, wordt automatisch naar de huidige tekst gescrold.
viewer-speed = Snelheid:
viewer-scroll-to-current = Naar huidig fragment scrollen
viewer-scroll-to-current-tooltip = Naar de momenteel gesproken tekst scrollen

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Api-sleutel vereist
viewer-translate-key-required-body = Voer de OpenAI-api-sleutel in in de instellingen
viewer-translate-settings-title = Instellingen voor AI-vertaling

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Tekst
viewer-export-translation = Vertaling
viewer-export-dialog-title = Bestand opslaan
viewer-export-filter = Tekstbestanden
viewer-export-failed = De export kon niet worden geschreven — zie het logboek voor meer informatie
export-written = Geschreven naar { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Instellingen
resizer-create-new = Nieuwe transcriptie maken
resizer-extend-heading = Eindtijd verlengen
resizer-extend-label = Eindes verlengen met maximaal (seconden)
resizer-extend-button = Eindes verlengen
resizer-resize-heading = Grootteopties
resizer-resize-label = Voorkeurslengte van ondertiteling
resizer-resize-button = Grootte
resizer-resize-disabled-tooltip = Alleen beschikbaar als timings op woordniveau zijn uitgeschakeld tijdens transcriptie
resizer-merge-heading = Samenvoegopties
resizer-merge-by-gap = Samenvoegen op interval
resizer-split-by-punctuation = Splitsen op interpunctie
resizer-split-by-max-length = Splitsen op maximale lengte
resizer-merge-button = Samenvoegen
resizer-merge-disabled-tooltip = Alleen beschikbaar als timings op woordniveau zijn ingeschakeld tijdens transcriptie
resizer-merging = Bezig met samenvoegen…
resizer-merge-error = Het samenvoegen kon niet worden uitgevoerd:
resizer-save-error = De wijzigingen konden niet worden opgeslagen:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Stap 1: Sprekers identificeren
speaker-id-step2-heading = Stap 2: Sprekers een naam geven
speaker-id-ready-label = Klaar om sprekers te identificeren
speaker-id-missing-audio-label = Audiobestand niet gevonden
speaker-id-identify-button = Identificeren
speaker-id-cancel-button = Annuleren
speaker-id-cancelling = Bezig met annuleren…
speaker-id-decoding = Audio decoderen…
speaker-id-downloading-models = Modellen downloaden…
speaker-id-identifying = Sprekers identificeren…
speaker-id-play-sample = Fragment afspelen
speaker-id-merge-checkbox = Zinnen van sprekers samenvoegen
speaker-id-save-button = Opslaan
speaker-id-save-error = De wijzigingen konden niet worden opgeslagen:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Live-opname
recording-model-load-note = Let op: de eerste keer kan het enkele minuten duren voordat het model geladen is.
recording-advanced-button = Geavanceerd…
recording-microphone-label = Microfoon:
recording-record-button = Opnemen
recording-stop-button = Stoppen
recording-realtime-warning = { $size } getranscribeerd met { $ratio }× realtime bij de laatste run — live transcriptie kan achterblijven
recording-chunk-skipped-warning = Een fragment is mislukt en overgeslagen: { $detail }
recording-panel-placeholder = Klik op de opnameknop om te beginnen…
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Bezig met wachten op AI-vertaling…
recording-model-downloading = Bezig met ophalen van model… { $percent }%
recording-model-downloading-indeterminate = Bezig met ophalen van model…
recording-model-loading = Model laden…
recording-error-prefix = Er is een fout opgetreden tijdens het starten van de opname:
recording-error-suffix = Controleer uw geluidsapparatuur of het programmalogboek.
recording-thread-crashed = De opnamesessie is onverwacht beëindigd. Raadpleeg de toepassingslogboeken voor meer informatie.
meter-average-volume = Gemiddeld volume
meter-queue = Wachtrij

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Geavanceerde instellingen
recording-advanced-speech-heading = Spraakherkenningsinstellingen
recording-advanced-initial-prompt-label = Hoofdinvoer:
recording-advanced-translation-heading = Vertaalinstellingen
recording-advanced-llm-enable = AI-vertaling inschakelen
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Opname-instellingen
recording-advanced-silence-threshold-label = Stiltedrempel:
recording-advanced-mode-label = Live-opnamemodus:
recording-advanced-mode-append-below = Onderaan toevoegen
recording-advanced-mode-append-above = Bovenaan toevoegen
recording-advanced-mode-append-and-correct = Toevoegen en corrigeren
recording-advanced-separator-label = Regelscheidingsteken:
recording-advanced-step-label = Transcriptiestap:
recording-advanced-hide-unconfirmed = Verberg onbevestigd
recording-advanced-export-enable = Live-opname exporteren inschakelen
recording-advanced-export-folder-label = Exportmap:
recording-advanced-export-browse = Bladeren
recording-advanced-export-file-name-label = Exportbestandsnaam:
recording-advanced-export-file-type-label = Exportbestandstype:
recording-advanced-export-file-type-txt = Tekstbestand (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Exportitems beperken
    (0 = alles exporteren):
recording-advanced-upload-url-label = Upload-url:
recording-advanced-ok = Oké

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Tonen
presentation-show-tooltip = In nieuw venster tonen
presentation-text-size-label = Tekstgrootte:
presentation-theme-label = Thema
presentation-theme-light = Licht
presentation-theme-dark = Donker
presentation-theme-custom = Aangepast
presentation-text-color-label = Tekstkleur
presentation-background-color-label = Achtergrondkleur
presentation-fullscreen-button = Volledig scherm
presentation-window-title = Live transcriptie presenteren

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Kopiëren
recording-copy-tooltip = Transcriptie naar klembord kopiëren
recording-copy-copied = Gekopieerd!
recording-copy-nothing = Niets om te kopiëren!

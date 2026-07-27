## humm-gui — German (de) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b, design spec §9.2). Section
## comments stay in English on purpose: they are developer-facing notes about
## *where* a key renders, identical in every catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own de_DE translation is used
## verbatim (`buzz/locale/de_DE/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh in the same
## register. Buzz's de catalog is `Language: de_DE@formal`, so this catalog is
## consistently formal (Sie), never du.
##
## Two mechanical corrections to the Buzz source, both upstream artifacts
## rather than translation choices: (1) six Buzz msgids carry two merged
## translation variants concatenated with no separator (e.g. "…werdenDer
## OpenAI-API-Schlüssel kann nicht…") — 11 locale-key instances across Buzz's
## de and es catalogs, of which this catalog is affected by all six
## (`prefs-general-force-cpu-tooltip`, `recording-model-load-note`,
## `viewer-loop-segment-tooltip`, `resizer-resize-disabled-tooltip`,
## `resizer-merge-disabled-tooltip`, `prefs-general-key-save-error-body` — the
## last of which is doubled in de only); each takes the second (revised)
## variant alone. (2) Strings naming the product "Buzz" are re-pointed at
## "humm", matching the en catalog's own substitution.
##
## Product name "humm" is never translated. Loanwords Buzz itself leaves
## untranslated in de (Update, Status, Text, Export, API, GPU, CPU, Whisper)
## are left alone here too.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry. Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), translated by Buzz's own po.

locale-name-en-us = Englisch
locale-name-ca-es = Katalanisch
locale-name-da-dk = Dänisch
locale-name-nl = Niederländisch
locale-name-de-de = Deutsch
locale-name-es-es = Spanisch
locale-name-it-it = Italienisch
locale-name-ja-jp = Japanisch
locale-name-pl-pl = Polnisch
locale-name-pt-br = Portugiesisch (Brasilien)
locale-name-ru = Russisch
locale-name-zh-cn = Chinesisch (vereinfacht)

## Empty-state hero (views/main_window.rs)

hero-drop = Medien hier ablegen oder ⌘O
hero-import-button = Dateien importieren…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Dateiname/URL
table-col-model = Modell
table-col-task = Aufgabe
table-col-status = Status
table-col-date-completed = Datum abgeschlossen
table-col-date-added = Datum hinzugefügt
table-col-notes = Notizen

## Task table — header/row context menus

table-menu-reset-column-order = Spaltenreihenfolge zurücksetzen
table-menu-restart = Transkription neu starten
table-menu-rename = Umbenennen
table-menu-notes = Notizen hinzufügen/bearbeiten

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transkribieren
task-translate = Ins Englische übersetzen

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition, not a Buzz string; translated fresh.

status-isolating-speech = Sprache wird isoliert

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Aufnehmen
toolbar-new-file = Neue Dateitranskription
toolbar-new-url = Neue URL-Transkription
toolbar-open-transcript = Transkript öffnen
toolbar-open-transcript-tooltip = Eine oder mehrere abgeschlossene Transkriptionen zum Öffnen auswählen
toolbar-cancel = Transkription abbrechen
toolbar-clear = Verlauf löschen

## Menu bar (views/main_window.rs, F-004)

menu-file = Datei
menu-file-import-file = Datei importieren…
menu-file-import-url = URL importieren…
menu-file-import-folder = Ordner importieren…
menu-help-title = Hilfe
menu-help-about = Über humm
menu-help-docs = Hilfe
menu-help-docs-tooltip = Die Dokumentation erscheint mit der ersten Veröffentlichung
menu-help-preferences = Einstellungen…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## Buzz's de po maps its own relabelled "Ok" button to "OK"; there is no
## separate de msgid for the "OK" spelling, so clear-confirm-ok/url-dialog-ok/
## recording-advanced-ok and dialog-ok all legitimately collapse to "OK" in
## German. The en catalog's deliberate Ok/OK casing split is Buzz's *English*
## lineage and does not survive translation.

clear-confirm-title = Verlauf löschen
clear-confirm-body = Sind Sie sicher, dass Sie die ausgewählte(n) Transkription(en) löschen möchten? Diese Aktion kann nicht rückgängig gemacht werden.
clear-confirm-ok = OK
clear-confirm-cancel = Abbrechen
clear-error = Die ausgewählte(n) Transkription(en) konnte(n) nicht gelöscht werden

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Transkription umbenennen
rename-label = Neuen Namen eingeben:
rename-error = Umbenennen nicht möglich
notes-title = Notizen
notes-label = Relevante Notizen für diese Transkription eingeben:
notes-error = Die Notizen konnten nicht gespeichert werden
dialog-ok = OK
dialog-cancel = Abbrechen

## Toasts (views/toasts.rs, app.rs, F-026)
##
## toast-completed-suffix renders after a count ("… — 3 Datei(en) geschrieben"),
## so it must read as a noun phrase, not a sentence.

toast-completed = Transkription abgeschlossen
toast-completed-suffix = Datei(en) geschrieben
toast-canceled = Transkription abgebrochen
toast-detail-title = Transkription fehlgeschlagen
toast-detail-open-log = Protokollordner öffnen
toast-detail-retry = Wiederholen
toast-detail-close = Schließen
retry-error-unavailable = Neustart nicht möglich: Die gespeicherten Optionen sind nicht verfügbar

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm konnte Verlaufsänderungen nicht speichern — Exporte werden weiterhin abgeschlossen; siehe Protokolldatei

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Zum Importieren ablegen

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## The binary name, the shell command and the env var are identifiers and stay
## verbatim; only the prose around them is translated.

url-import-unavailable-tooltip = yt-dlp fehlt — installieren Sie es (z. B. `brew install yt-dlp`) oder setzen Sie HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)

url-dialog-title = URL importieren
url-dialog-label = URL:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = Die von Ihnen eingegebene URL ist ungültig.
url-dialog-ok = OK
url-dialog-cancel = Abbrechen

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Modell:
options-task-label = Aufgabe:
options-language-label = Sprache:
language-detect = Sprache erkennen
options-word-timestamps = Zeitangaben auf Wortebene
options-extract-speech = Sprache extrahieren
options-extract-speech-missing-tooltip = demucs nicht gefunden — installieren Sie es (z. B. `pipx install demucs`), um die Sprachextraktion zu aktivieren
options-export-label = Export:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = kein Export ausgewählt
options-advanced = Erweitert
options-initial-prompt-label = Erste Anweisung:
options-hf-repo-label = Hugging Face-Modell
options-run = Ausführen

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2).
##
## "Backend" is standard German technical usage and stays as-is, the same way
## Buzz's own de catalog keeps Update/Status/Export.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = API-Schlüssel:
options-llm-heading = Übersetzungseinstellungen
options-llm-enable = KI-Übersetzung aktivieren
options-llm-model-label = KI-Modell:
options-llm-model-presets-button = Voreinstellungen
options-llm-prompt-label = Anweisung zur KI:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Überwacht { $name }
folderwatch-pill-paused = Pausiert
folderwatch-pill-pause-tip = Ordnerüberwachung pausieren
folderwatch-pill-resume-tip = Ordnerüberwachung fortsetzen

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Update verfügbar
update-dialog-title = Update verfügbar
update-dialog-header = Eine neue Version von humm ist verfügbar!
update-dialog-current-version = Aktuelle Version:
update-dialog-new-version = Neue Version:
update-dialog-release-notes = Versionshinweise:
update-dialog-open-release-page = Release-Seite öffnen
update-dialog-close = Schließen

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Version
about-check-button = Nach Updates suchen
about-show-logs-button = Protokolle anzeigen
about-close-button = Schließen
about-up-to-date = Sie sind auf dem Laufenden!
about-check-failed = Die Update-Prüfung ist fehlgeschlagen — siehe Protokolldatei

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Einstellungen
prefs-tab-general = Allgemein
prefs-tab-models = Modelle
prefs-tab-shortcuts = Tastenkombinationen
prefs-tab-folder-watch = Ordner überwachen
prefs-tab-postproc = Nachbearbeitung
prefs-tab-privacy = Datenschutz

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1)

prefs-general-ui-language-label = Sprache der Benutzeroberfläche
prefs-general-font-size-label = Schriftgröße
prefs-general-font-size-custom-label = Benutzerdefinierte Schriftgröße
prefs-general-openai-key-label = OpenAI-API-Schlüssel
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Anzeigen
prefs-general-key-hide = Ausblenden
prefs-general-test-button = Test
prefs-general-test-title = OpenAI-API-Schlüssel Test
prefs-general-test-success-body = Ihr API-Schlüssel ist gültig. humm verwendet diesen Schlüssel, um Whisper-API-Transkriptionen und KI-Übersetzungen durchzuführen.
prefs-general-invalid-key-title = Ungültiger API-Schlüssel
prefs-general-invalid-key-body = Die API unterstützt nur Base64-Zeichen (A-Za-z0-9+/=_-). Andere Zeichen im API-Schlüssel können Fehler verursachen.
prefs-general-key-save-error-title = Fehler
prefs-general-key-save-error-body = Der OpenAI-API-Schlüssel kann nicht im Schlüsselbund gespeichert werden
prefs-general-base-url-label = OpenAI-Basis-URL
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI-API-Modell
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Standardname der Exportdatei
prefs-general-force-cpu-row-label = GPU deaktivieren
prefs-general-force-cpu-label = Nur CPU verwenden und GPU-Beschleunigung deaktivieren
prefs-general-force-cpu-tooltip = Diese Option aktivieren, wenn größere Modelle nicht in den GPU-Speicher passen und humm abstürzt

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Anonyme Startdaten senden
privacy-telemetry-disabled-caption = In diesem Build ist die Telemetrie zur Kompilierzeit deaktiviert.
privacy-telemetry-items-intro = Jedes Startereignis enthält:
privacy-telemetry-item-app-version = App-Version
privacy-telemetry-item-system-name = Systemname
privacy-telemetry-item-os-version = Betriebssystemversion
privacy-telemetry-item-kernel-release = Kernel-Version
privacy-telemetry-item-cpu-architecture = CPU-Architektur
privacy-telemetry-item-system-locale = Systemsprache
privacy-telemetry-item-identifier = Eine zufällige anonyme Kennung
privacy-update-checkbox = Automatisch nach Updates suchen

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Heruntergeladen
models-section-available = Zum Herunterladen verfügbar
models-download-button = Herunterladen
models-delete-button = Löschen
models-show-location-button = Dateispeicherort anzeigen
models-cancel-button = Abbrechen
models-custom-url-label = Benutzerdefinierte ggml-Modell-URL:
models-delete-confirm-title = Modell löschen
models-delete-confirm-body = Sind Sie sicher, dass Sie das ausgewählte Modell löschen möchten?
models-download-error-title = Fehler
models-download-failed = Der Download ist fehlgeschlagen
models-speed-badge = { $ratio }× Echtzeit
models-speed-badge-tip = Gemessene Echtzeit-Geschwindigkeit aus den Live-Aufnahmesitzungen auf diesem Gerät

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `.po` translations for `shortcut.py`'s descriptions.

shortcut-desc-open-record-window = Aufnahmefenster öffnen
shortcut-desc-import-file = Datei importieren
shortcut-desc-import-url = URL importieren
shortcut-desc-open-preferences-window = Einstellungsfenster öffnen
shortcut-desc-view-transcript-text = Transkriptionstext anzeigen
shortcut-desc-view-transcript-translation = Übersetzung des Transkripts anzeigen
shortcut-desc-view-transcript-timestamps = Zeitstempel des Transkripts anzeigen
shortcut-desc-search-transcript = Transkript durchsuchen
shortcut-desc-search-next = Zum nächsten Transkript-Suchergebnis gehen
shortcut-desc-search-previous = Zum vorherigen Transkript-Suchergebnis gehen
shortcut-desc-scroll-to-current-text = Zum aktuellen Text scrollen
shortcut-desc-play-pause-audio = Audio abspielen/pausieren
shortcut-desc-replay-current-segment = Aktuelles Segment erneut abspielen
shortcut-desc-toggle-playback-controls = Wiedergabesteuerung ein-/ausblenden
shortcut-desc-decrease-segment-start = Segmentanfangszeit verringern
shortcut-desc-increase-segment-start = Segmentanfangszeit erhöhen
shortcut-desc-decrease-segment-end = Segmentendzeit verringern
shortcut-desc-increase-segment-end = Segmentendzeit erhöhen
shortcut-desc-clear-history = Verlauf löschen
shortcut-desc-stop-transcription = Transkription abbrechen
prefs-shortcuts-press = Tastenkombination drücken…
prefs-shortcuts-clear-tooltip = Auf Standard zurücksetzen
prefs-shortcuts-conflict = Kollidiert mit { $other }
prefs-shortcuts-reset = Auf Standardeinstellungen zurücksetzen

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own de_DE translations
## (`folder_watch_preferences_widget.py`), used verbatim.
## `prefs-fw-disposition-note` is a ✚ humm-only addition (design spec §3.4,
## no Buzz source) -- translated fresh in this catalog's own formal (Sie)
## register.

prefs-fw-enable = Ordner überwachen aktivieren
prefs-fw-input-folder-label = Eingabeordner
prefs-fw-input-folder-placeholder = /pfad/zum/eingabeordner
prefs-fw-output-folder-label = Ausgabeordner
prefs-fw-output-folder-placeholder = /pfad/zum/ausgabeordner
prefs-fw-browse = Durchsuchen
prefs-fw-delete-checkbox = Verarbeitete Dateien löschen
prefs-fw-disposition-note = Nach der Transkription wird die Quelldatei in den Ausgabeordner verschoben — oder gelöscht, wenn Verarbeitete Dateien löschen aktiviert ist.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16). Section titles and field labels/descriptions are
## Buzz's own de_DE plugin locale JSONs (`plugins/*/locale/de_DE.json`), used
## verbatim -- Buzz's own translations. The four "Enable …" checkbox labels
## and `prefs-postproc-header`/`prefs-postproc-summary-key-save-error-body`
## have no Buzz source (Buzz's plugin enablement lived in the unported
## Plugins management dialog, ADR-010) -- translated fresh in this
## catalog's own formal (Sie) register.

prefs-postproc-header = Diese Hooks werden automatisch nach jeder abgeschlossenen Transkription ausgeführt. Ein Hook-Fehler wird protokolliert, führt aber nie zum Scheitern der Transkription selbst.
prefs-postproc-skip-title = Bereits Transkribierte überspringen
prefs-postproc-skip-enable = Bereits Transkribierte überspringen aktivieren
prefs-postproc-skip-check-files = Nach vorhandenen Ergebnisdateien suchen
prefs-postproc-skip-check-files-desc = Überspringen, wenn eine .txt-, .srt- oder .vtt-Datei mit demselben Audiodateinamen daneben gefunden wird
prefs-postproc-skip-check-db = In der Transkriptionsdatenbank prüfen
prefs-postproc-skip-check-db-desc = Überspringen, wenn diese Datei bereits transkribiert wurde und der Eintrag in der Datenbank vorhanden ist
prefs-postproc-dfn-title = DeepFilterNet-Rauschunterdrückung
prefs-postproc-dfn-enable = DeepFilterNet-Rauschunterdrückung aktivieren
prefs-postproc-dfn-keep-file = Entrauschte Datei nach der Transkription behalten
prefs-postproc-langdetect-title = Erweiterte Spracherkennung
prefs-postproc-langdetect-enable = Erweiterte Spracherkennung aktivieren
prefs-postproc-langdetect-download-tiny = Tiny-Modell herunterladen, falls keines verfügbar ist
prefs-postproc-langdetect-download-tiny-desc = Wenn kein whisper.cpp-Modell heruntergeladen ist, wird das Tiny-Modell für die Spracherkennung heruntergeladen.
prefs-postproc-resizer-title = Automatische Transkript-Anpassung
prefs-postproc-resizer-enable = Automatische Transkript-Anpassung aktivieren
prefs-postproc-resizer-merge-by-gap = Nach Pause zusammenführen
prefs-postproc-resizer-merge-gap-seconds = Pause beim Zusammenführen (Sekunden)
prefs-postproc-resizer-split-by-punctuation = Nach Satzzeichen teilen
prefs-postproc-resizer-punctuation = Satzzeichen
prefs-postproc-resizer-split-by-max-length = Nach maximaler Länge teilen
prefs-postproc-resizer-max-length = Maximale Untertitellänge
prefs-postproc-summary-title = KI-Zusammenfassung
prefs-postproc-summary-enable = KI-Zusammenfassung aktivieren
prefs-postproc-summary-api-url = API-Basis-URL
prefs-postproc-summary-api-key-label = API-Schlüssel
prefs-postproc-summary-model = Modell
prefs-postproc-summary-prompt = Zusammenfassungs-Prompt
prefs-postproc-summary-save-to-notes = Zusammenfassung in Notizen speichern
prefs-postproc-summary-save-to-file = Zusammenfassung in einer Datei speichern
prefs-postproc-summary-output-folder = Ausgabeordner (für Datei)
prefs-postproc-summary-key-save-error-body = Der KI-Zusammenfassung-API-Schlüssel kann nicht im Schlüsselbund gespeichert werden
prefs-postproc-docx-title = Als DOCX exportieren
prefs-postproc-docx-enable = Als DOCX exportieren aktivieren
prefs-postproc-docx-output-folder = Ausgabeordner
prefs-postproc-docx-include-timestamps = Zeitstempel einschließen
prefs-postproc-output-folder-hint = Leer lassen, um neben der Quelldatei zu speichern.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Anzeigen
viewer-mode-text = Text
viewer-mode-translation = Übersetzung
viewer-mode-timestamps = Zeitstempel
viewer-export = Export
viewer-translate = Übersetzen
viewer-resize = Größe ändern
viewer-identify-speakers = Sprecher identifizieren
viewer-find = Suchen
viewer-find-label = Suchen:
viewer-find-placeholder = Suchtext eingeben...
viewer-find-clear = Löschen
viewer-find-prev-tooltip = Vorheriger Treffer ({ $shortcut })
viewer-find-next-tooltip = Nächster Treffer ({ $shortcut })
viewer-empty = Keine Segmente — die Eingabe enthält möglicherweise nur Stille
viewer-open-error = Das Transkript konnte nicht geöffnet werden
viewer-col-start = Start
viewer-col-end = Ende
viewer-col-text = Text
viewer-col-translation = Übersetzung
viewer-saved = bearbeitet · gespeichert
viewer-unsaved-warning = Eine Änderung konnte nicht gespeichert werden — sie bleibt im Arbeitsspeicher, wurde aber nicht auf die Festplatte geschrieben
viewer-player-loading = Audio wird geladen…
viewer-player-failed = Wiedergabe nicht verfügbar
viewer-playback-controls = Wiedergabesteuerung:
viewer-loop-segment = Segment wiederholen
viewer-loop-segment-tooltip = Wiederholen beim Klicken auf Transkript-Segmente aktivieren/deaktivieren
viewer-follow-audio = Audio folgen
viewer-follow-audio-tooltip = Aktuelle Audioposition im Transkript verfolgen aktivieren/deaktivieren. Bei Aktivierung wird automatisch zum aktuellen Text gescrollt.
viewer-speed = Geschwindigkeit:
viewer-scroll-to-current = Zur aktuellen Stelle scrollen
viewer-scroll-to-current-tooltip = Zum aktuell gesprochenen Text scrollen

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3)

viewer-translate-key-required-title = API-Schlüssel erforderlich
viewer-translate-key-required-body = Bitte geben Sie den OpenAI-API-Schlüssel in den Einstellungen ein
viewer-translate-settings-title = Einstellungen für KI-Übersetzung

## Export menu (views/export_menu.rs, F-084)

viewer-export-text = Text
viewer-export-translation = Übersetzung
viewer-export-dialog-title = Datei speichern
viewer-export-filter = Textdateien
viewer-export-failed = Der Export konnte nicht geschrieben werden — Einzelheiten finden Sie im Protokoll
export-written = Geschrieben nach { $path }

## Transcript resizer window (views/resizer.rs, F-086)

resizer-settings = Einstellungen
resizer-create-new = Neue Transkription erstellen
resizer-extend-heading = Endzeit verlängern
resizer-extend-label = Enden um bis zu (Sekunden) verlängern
resizer-extend-button = Enden verlängern
resizer-resize-heading = Größenänderungsoptionen
resizer-resize-label = Gewünschte Untertitellänge
resizer-resize-button = Größe ändern
resizer-resize-disabled-tooltip = Nur verfügbar, wenn Zeitangaben auf Wortebene bei der Transkription deaktiviert waren
resizer-merge-heading = Zusammenführungsoptionen
resizer-merge-by-gap = Nach Pause zusammenführen
resizer-split-by-punctuation = Nach Satzzeichen teilen
resizer-split-by-max-length = Nach maximaler Länge teilen
resizer-merge-button = Vereinigen
resizer-merge-disabled-tooltip = Nur verfügbar, wenn Zeitangaben auf Wortebene bei der Transkription aktiviert waren
resizer-merging = Wird zusammengeführt…
resizer-merge-error = Die Zusammenführung konnte nicht ausgeführt werden:
resizer-save-error = Die Änderungen konnten nicht gespeichert werden:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)

speaker-id-step1-heading = Schritt 1: Sprecher identifizieren
speaker-id-step2-heading = Schritt 2: Sprecher benennen
speaker-id-ready-label = Bereit zur Sprecheridentifikation
speaker-id-missing-audio-label = Audiodatei nicht gefunden
speaker-id-identify-button = Identifizieren
speaker-id-cancel-button = Abbrechen
speaker-id-cancelling = Wird abgebrochen...
speaker-id-decoding = Audio wird dekodiert…
speaker-id-downloading-models = Modelle werden heruntergeladen…
speaker-id-identifying = Sprecher werden identifiziert…
speaker-id-play-sample = Beispiel abspielen
speaker-id-merge-checkbox = Sprechersätze zusammenführen
speaker-id-save-button = Speichern
speaker-id-save-error = Die Änderungen konnten nicht gespeichert werden:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)

recording-window-title = Live-Aufnahme
recording-model-load-note = Bei der ersten Verwendung eines Modells kann das Laden mehrere Minuten dauern.
recording-advanced-button = Erweitert...
recording-microphone-label = Mikrofon:
recording-record-button = Aufnehmen
recording-stop-button = Stoppen
recording-realtime-warning = { $size } wurde beim letzten Lauf mit { $ratio }× Echtzeit transkribiert — die Live-Transkription könnte zurückfallen
recording-chunk-skipped-warning = Ein Abschnitt ist fehlgeschlagen und wurde übersprungen: { $detail }
recording-panel-placeholder = Auf Aufnehmen klicken um zu beginnen …
recording-panel-translation-placeholder = Warten auf KI-Übersetzung...
recording-model-downloading = Modell wird heruntergeladen… { $percent } %
recording-model-downloading-indeterminate = Modell wird heruntergeladen…
recording-model-loading = Modell wird geladen…
recording-error-prefix = Beim Starten einer neuen Aufnahme ist ein Fehler aufgetreten:
recording-error-suffix = Bitte überprüfen Sie Ihre Audiogeräte oder prüfen Sie die Anwendungsprotokolle für weitere Informationen.
recording-thread-crashed = Die Aufnahmesitzung wurde unerwartet beendet. Bitte prüfen Sie die Anwendungsprotokolle für weitere Informationen.
meter-average-volume = Durchschnittliche Lautstärke
meter-queue = Warteschlange

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060)

recording-advanced-title = Erweiterte Einstellungen
recording-advanced-speech-heading = Einstellungen für die Spracherkennung
recording-advanced-initial-prompt-label = Erste Anweisung:
recording-advanced-translation-heading = Übersetzungseinstellungen
recording-advanced-llm-enable = KI-Übersetzung aktivieren
recording-advanced-recording-heading = Aufnahmeeinstellungen
recording-advanced-silence-threshold-label = Stille-Schwellenwert:
recording-advanced-mode-label = Live-Aufnahmemodus:
recording-advanced-mode-append-below = Unten anhängen
recording-advanced-mode-append-above = Oben anhängen
recording-advanced-mode-append-and-correct = Anhängen und korrigieren
recording-advanced-separator-label = Zeilentrenner:
recording-advanced-step-label = Transkriptionsschritt:
recording-advanced-hide-unconfirmed = Unbestätigtes ausblenden
recording-advanced-export-enable = Export von Live-Aufnahmen aktivieren
recording-advanced-export-folder-label = Exportordner:
recording-advanced-export-browse = Durchsuchen
recording-advanced-export-file-name-label = Exportdateiname:
recording-advanced-export-file-type-label = Exportdateityp:
recording-advanced-export-file-type-txt = Textdatei (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Exporteinträge begrenzen
    (0 = alle exportieren):
recording-advanced-upload-url-label = Upload-URL:
recording-advanced-ok = OK

## Presentation window + bar (views/presentation.rs, F-065)

presentation-show-button = Anzeigen
presentation-show-tooltip = In neuem Fenster anzeigen
presentation-text-size-label = Textgröße:
presentation-theme-label = Design
presentation-theme-light = Hell
presentation-theme-dark = Dunkel
presentation-theme-custom = Benutzerdefiniert
presentation-text-color-label = Textfarbe
presentation-background-color-label = Hintergrundfarbe
presentation-fullscreen-button = Vollbild
presentation-window-title = Live-Transkript-Präsentation

## Copy bar (views/recording.rs, F-065 §2.7)

recording-copy-button = Kopieren
recording-copy-tooltip = Transkription in die Zwischenablage kopieren
recording-copy-copied = Kopiert!
recording-copy-nothing = Nichts zum Kopieren vorhanden!

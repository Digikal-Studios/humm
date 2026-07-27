## humm-gui — Polish (pl) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 14, design spec
## §7.2). Section comments stay in English on purpose: they are
## developer-facing notes about *where* a key renders, identical in every
## catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own pl_PL translation is used
## verbatim (`buzz/locale/pl_PL/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh, matching
## the register the Buzz-sourced strings already establish. Buzz's pl_PL po
## carries no formal/informal header marker but consistently uses direct,
## informal second-person imperatives and questions throughout its own
## translations (e.g. "Czy na pewno chcesz usunąć...", "Wprowadź nową
## nazwę:") — this catalog stays in that same direct register consistently,
## including for the fresh humm-only strings.
##
## Buzz's own pl_PL po translates the msgid "Ok" (its own relabelled dialog
## button text) as "OK". Following the es/zh-CN/M8-Task-13 precedent (Buzz's
## own lineage, not a translation choice made here), all four of
## clear-confirm-ok, dialog-ok, url-dialog-ok and recording-advanced-ok
## collapse to that same "OK".
##
## Six Buzz msgids are known (from the M8 Task 13 batch) to sometimes carry
## two translation variants concatenated with no separator, an upstream Buzz
## artifact rather than a translation choice. Three of the six are affected
## in pl_PL's own po and are fixed here: `resizer-resize-disabled-tooltip`
## and `resizer-merge-disabled-tooltip` each double two genuinely different
## variants ("znaczniki czasu na poziomie słów" vs the shorter "znaczniki dla
## słów") with no separator — the second (revised, shorter) variant is kept
## in each case. `prefs-general-force-cpu-tooltip` is different from the
## other five locales seen so far: its msgstr doubles the *exact same*
## sentence verbatim twice, not two distinct variants — deduplicated to one
## copy here rather than picking a "second, revised" half that doesn't
## exist. The other three of the six are clean (single variant, no
## concatenation) in pl_PL's po and are used as-is: `recording-model-load-
## note`, `viewer-loop-segment-tooltip`, `prefs-general-key-save-error-body`.
##
## Two further Buzz msgids match humm strings exactly but carry a truly
## *empty* msgstr in pl_PL's po (untranslated upstream, gettext falls back
## silently): `prefs-postproc-skip-check-files-desc` and `prefs-postproc-
## skip-check-db-desc`. Both are translated fresh here rather than left
## blank — the same two keys M8 Task 13 found empty in all four of its own
## locales.
##
## pl_PL's po carries no msgid "Close" at all — neither live nor obsolete —
## unlike ja_JP/pt_BR/ru in this same batch, which each reuse an obsolete
## "Close" translation for their own about-close-button/toast-detail-close/
## update-dialog-close trio. All three are translated fresh here instead
## ("Zamknij"), the same situation M8 Task 13 documented for da_DK.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in pl (API, GPU, CPU,
## Whisper, URL, TXT/SRT/VTT) are left alone here too; "Backend" is likewise
## kept untranslated, matching de/es/it/ca/da/nl's own choice for the same
## term.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Angielski
locale-name-ca-es = Kataloński
locale-name-da-dk = Duński
locale-name-nl = Niderlandzki
locale-name-de-de = Niemiecki
locale-name-es-es = Hiszpański
locale-name-it-it = Włoski
locale-name-ja-jp = Japoński
locale-name-pl-pl = Polski
locale-name-pt-br = Portugalski (Brazylia)
locale-name-ru = Rosyjski
locale-name-zh-cn = Chiński (uproszczony)

## Empty-state hero (views/main_window.rs)

hero-drop = Upuść tutaj plik multimedialny lub ⌘O
hero-import-button = Importuj pliki…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Nazwa pliku / URL
table-col-model = Model
table-col-task = Zadanie
table-col-status = Status
table-col-date-completed = Data ukończenia
table-col-date-added = Data dodania
table-col-notes = Notatki

## Task table — header/row context menus

table-menu-reset-column-order = Resetuj kolejność kolumn
table-menu-restart = Uruchom ponownie transkrypcję
table-menu-rename = Zmień nazwę
table-menu-notes = Dodaj/edytuj notatki

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transkrybuj
task-translate = Przetłumacz na angielski

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = izolowanie mowy

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Nagraj
toolbar-new-file = Nowa transkrypcja pliku
toolbar-new-url = Nowa transkrypcja z URL
toolbar-open-transcript = Otwórz transkrypt
toolbar-open-transcript-tooltip = Wybierz jedną lub więcej ukończonych transkrypcji, aby je otworzyć
toolbar-cancel = Anuluj transkrypcję
toolbar-clear = Wyczyść historię

## Menu bar (views/main_window.rs, F-004)

menu-file = Plik
menu-file-import-file = Importuj plik…
menu-file-import-url = Importuj URL…
menu-file-import-folder = Importuj folder…
menu-help-title = Pomoc
menu-help-about = O programie humm
menu-help-docs = Pomoc
menu-help-docs-tooltip = Dokumentacja pojawi się wraz z pierwszym wydaniem
menu-help-preferences = Ustawienia…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Wyczyść historię
clear-confirm-body = Czy na pewno chcesz usunąć zaznaczone transkrypcje? Tej operacji nie można cofnąć.
clear-confirm-ok = OK
clear-confirm-cancel = Anuluj
clear-error = Nie można usunąć zaznaczonych transkrypcji

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Zmień nazwę transkrypcji
rename-label = Wprowadź nową nazwę:
rename-error = Nie można zmienić nazwy
notes-title = Notatki
notes-label = Wprowadź odpowiednie notatki do tej transkrypcji:
notes-error = Nie można zapisać notatek
dialog-ok = OK
dialog-cancel = Anuluj

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transkrypcja ukończona
toast-completed-suffix = plik(i) zapisano
toast-canceled = Transkrypcja anulowana
toast-detail-title = Transkrypcja nie powiodła się
toast-detail-open-log = Otwórz folder dziennika
toast-detail-retry = Ponów
toast-detail-close = Zamknij
retry-error-unavailable = Nie można wznowić: zapisane opcje są niedostępne

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm nie mógł zapisać aktualizacji historii — eksporty mimo to zostały ukończone; sprawdź plik dziennika

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Upuść, aby zaimportować

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = Brak yt-dlp — zainstaluj je (np. `brew install yt-dlp`) lub ustaw HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Importuj URL
url-dialog-label = URL:
url-dialog-placeholder = https://przyklad.pl/audio.mp3
url-dialog-invalid = Wprowadzony URL nie jest prawidłowy
url-dialog-ok = OK
url-dialog-cancel = Anuluj

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Model:
options-task-label = Zadanie:
options-language-label = Język:
language-detect = Wykryj język
options-word-timestamps = Znaczniki dla słów
options-extract-speech = Wyodrębnij mowę
options-extract-speech-missing-tooltip = Nie znaleziono demucs — zainstaluj je (np. `pipx install demucs`), aby włączyć wydzielanie mowy
options-export-label = Eksportuj:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = nie wybrano formatu eksportu
options-advanced = Zaawansowane
options-initial-prompt-label = Wstępne instrukcje:
options-hf-repo-label = Model Hugging Face
options-run = Rozpocznij

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Klucz API:
options-llm-heading = Ustawienia tłumaczenia
options-llm-enable = Włącz tłumaczenie AI
options-llm-model-label = Model AI:
options-llm-model-presets-button = Ustawienia wstępne
options-llm-prompt-label = Instrukcje dla AI:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Obserwowanie { $name }
folderwatch-pill-paused = Wstrzymano
folderwatch-pill-pause-tip = Wstrzymaj obserwowanie folderu
folderwatch-pill-resume-tip = Wznów obserwowanie folderu

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Dostępna aktualizacja
update-dialog-title = Dostępna aktualizacja
update-dialog-header = Dostępna jest nowa wersja humm!
update-dialog-current-version = Aktualna wersja:
update-dialog-new-version = Nowa wersja:
update-dialog-release-notes = Informacje o wydaniu:
update-dialog-open-release-page = Otwórz stronę wydania
update-dialog-close = Zamknij

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Wersja
about-check-button = Sprawdź aktualizacje
about-show-logs-button = Pokaż logi
about-close-button = Zamknij
about-up-to-date = Posiadasz najnowszą wersję!
about-check-failed = Sprawdzanie aktualizacji nie powiodło się — sprawdź plik dziennika

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Ustawienia
prefs-tab-general = Główne
prefs-tab-models = Modele
prefs-tab-shortcuts = Skróty
prefs-tab-folder-watch = Przeglądanie folderu
prefs-tab-postproc = Przetwarzanie końcowe
prefs-tab-privacy = Prywatność

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Język interfejsu
prefs-general-font-size-label = Rozmiar czcionki
prefs-general-font-size-custom-label = Niestandardowy rozmiar czcionki
prefs-general-openai-key-label = Klucz API OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Pokaż
prefs-general-key-hide = Ukryj
prefs-general-test-button = Testuj
prefs-general-test-title = Test klucza API OpenAI
prefs-general-test-success-body = Twój klucz API jest prawidłowy. humm użyje tego klucza do wykonywania transkrypcji przez Whisper API oraz tłumaczeń AI.
prefs-general-invalid-key-title = Nieprawidłowy klucz API
prefs-general-invalid-key-body = API obsługuje tylko znaki base64 (A-Za-z0-9+/=_-). Inne znaki w kluczu API mogą powodować błędy.
prefs-general-key-save-error-title = Błąd
prefs-general-key-save-error-body = Nie można zapisać klucza API OpenAI w breloku kluczy
prefs-general-base-url-label = Podstawowy adres URL OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Model API OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Domyślna nazwa pliku eksportu
prefs-general-force-cpu-row-label = Wyłącz GPU
prefs-general-force-cpu-label = Używaj tylko CPU i wyłącz akcelerację GPU
prefs-general-force-cpu-tooltip = Ustaw to, jeśli większe modele nie mieszczą się w pamięci GPU i humm się zawiesza

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Wysyłaj anonimową telemetrię uruchomień
privacy-telemetry-disabled-caption = W tej wersji telemetria jest wyłączona w czasie kompilacji.
privacy-telemetry-items-intro = Każde zdarzenie uruchomienia zawiera:
privacy-telemetry-item-app-version = Wersja aplikacji
privacy-telemetry-item-system-name = Nazwa systemu
privacy-telemetry-item-os-version = Wersja systemu operacyjnego
privacy-telemetry-item-kernel-release = Wersja jądra
privacy-telemetry-item-cpu-architecture = Architektura CPU
privacy-telemetry-item-system-locale = Lokalizacja systemowa
privacy-telemetry-item-identifier = Losowy anonimowy identyfikator
privacy-update-checkbox = Automatycznie sprawdzaj aktualizacje

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Pobrany
models-section-available = Dostępne do pobrania
models-download-button = Pobierz
models-delete-button = Usuń
models-show-location-button = Pokaż lokalizacje pliku
models-cancel-button = Anuluj
models-custom-url-label = Niestandardowy URL modelu ggml:
models-delete-confirm-title = Usuń model
models-delete-confirm-body = Czy na pewno chcesz usunąć wybrany model?
models-download-error-title = Błąd
models-download-failed = Pobieranie nie powiodło się
models-speed-badge = { $ratio }× czasu rzeczywistego
models-speed-badge-tip = Zmierzona prędkość czasu rzeczywistego z sesji nagrywania na żywo na tym urządzeniu

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Otwórz okno nagrywania
shortcut-desc-import-file = Importuj plik
shortcut-desc-import-url = Importuj URL
shortcut-desc-open-preferences-window = Otwórz okno ustawień
shortcut-desc-view-transcript-text = Wyświetl tekst transkrypcji
shortcut-desc-view-transcript-translation = Wyświetl tłumaczenie transkrypcji
shortcut-desc-view-transcript-timestamps = Wyświetl znaczniki czasu transkrypcji
shortcut-desc-search-transcript = Wyszukaj w transkrypcji
shortcut-desc-search-next = Przejdź do następnego wyniku wyszukiwania
shortcut-desc-search-previous = Przejdź do poprzedniego wyniku wyszukiwania
shortcut-desc-scroll-to-current-text = Przewiń do bieżącego tekstu
shortcut-desc-play-pause-audio = Odtwórz/wstrzymaj audio
shortcut-desc-replay-current-segment = Odtwórz ponownie bieżący fragment
shortcut-desc-toggle-playback-controls = Przełącz sterowanie odtwarzaniem
shortcut-desc-decrease-segment-start = Zmniejsz czas początku fragmentu
shortcut-desc-increase-segment-start = Zwiększ czas początku fragmentu
shortcut-desc-decrease-segment-end = Zmniejsz czas końca fragmentu
shortcut-desc-increase-segment-end = Zwiększ czas końca fragmentu
shortcut-desc-clear-history = Wyczyść historię
shortcut-desc-stop-transcription = Anuluj transkrypcję
prefs-shortcuts-press = Naciśnij skrót…
prefs-shortcuts-clear-tooltip = Przywróć domyślny
prefs-shortcuts-conflict = Konflikt z { $other }
prefs-shortcuts-reset = Przywróć ustawienia domyślne

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Włącz obserwację folderu
prefs-fw-input-folder-label = Folder wejściowy
prefs-fw-input-folder-placeholder = /ścieżka/do/folderu/wejściowego
prefs-fw-output-folder-label = Folder wyjściowy
prefs-fw-output-folder-placeholder = /ścieżka/do/folderu/wyjściowego
prefs-fw-browse = Przeglądaj
prefs-fw-delete-checkbox = Usuń przetworzonych pliki
prefs-fw-disposition-note = Po zakończeniu transkrypcji plik źródłowy zostaje przeniesiony do folderu wyjściowego — lub usunięty, jeśli zaznaczono opcję Usuń przetworzone pliki.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Te hooki uruchamiają się automatycznie po zakończeniu każdej transkrypcji. Błąd hooka jest rejestrowany w dzienniku, ale nigdy nie powoduje niepowodzenia samej transkrypcji.
prefs-postproc-skip-title = Pomijaj już transkrybowane
prefs-postproc-skip-enable = Włącz Pomiń już przetranskrybowane
prefs-postproc-skip-check-files = Sprawdź istniejące pliki wyników
prefs-postproc-skip-check-files-desc = Pomiń, jeśli obok pliku audio znajduje się pasujący plik .txt, .srt lub .vtt
prefs-postproc-skip-check-db = Sprawdź w bazie danych transkrypcji
prefs-postproc-skip-check-db-desc = Pomiń, jeśli ten plik został już przetranskrybowany, a rekord istnieje w bazie danych
prefs-postproc-dfn-title = Redukcja szumów DeepFilterNet
prefs-postproc-dfn-enable = Włącz redukcję szumów DeepFilterNet
prefs-postproc-dfn-keep-file = Zachowaj plik po usunięciu szumów po transkrypcji
prefs-postproc-langdetect-title = Ulepszone wykrywanie języka
prefs-postproc-langdetect-enable = Włącz Rozszerzone wykrywanie języka
prefs-postproc-langdetect-download-tiny = Pobierz model tiny, jeśli żaden nie jest dostępny
prefs-postproc-langdetect-download-tiny-desc = Jeśli nie pobrano żadnego modelu whisper.cpp, pobiera model tiny do wykrywania języka.
prefs-postproc-resizer-title = Automatyczna zmiana rozmiaru transkrypcji
prefs-postproc-resizer-enable = Włącz Automatyczny zmieniacz rozmiaru transkrypcji
prefs-postproc-resizer-merge-by-gap = Scal według odstępu
prefs-postproc-resizer-merge-gap-seconds = Odstęp scalania (sekundy)
prefs-postproc-resizer-split-by-punctuation = Podziel według interpunkcji
prefs-postproc-resizer-punctuation = Interpunkcja
prefs-postproc-resizer-split-by-max-length = Podziel według maksymalnej długości
prefs-postproc-resizer-max-length = Maksymalna długość napisu
prefs-postproc-summary-title = Podsumowanie AI
prefs-postproc-summary-enable = Włącz Podsumowanie AI
prefs-postproc-summary-api-url = Bazowy adres URL API
prefs-postproc-summary-api-key-label = Klucz API
prefs-postproc-summary-model = Model
prefs-postproc-summary-prompt = Polecenie podsumowania
prefs-postproc-summary-save-to-notes = Zapisz podsumowanie w notatkach
prefs-postproc-summary-save-to-file = Zapisz podsumowanie w pliku
prefs-postproc-summary-output-folder = Folder wyjściowy (dla pliku)
prefs-postproc-summary-key-save-error-body = Nie można zapisać klucza API podsumowania AI w breloku kluczy
prefs-postproc-docx-title = Eksportuj do DOCX
prefs-postproc-docx-enable = Włącz eksport do DOCX
prefs-postproc-docx-output-folder = Folder wyjściowy
prefs-postproc-docx-include-timestamps = Dołącz znaczniki czasu
prefs-postproc-output-folder-hint = Pozostaw puste, aby zapisać obok pliku źródłowego.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Widok
viewer-mode-text = Tekst
viewer-mode-translation = Tłumaczenie
viewer-mode-timestamps = Znaczniki czasu
viewer-export = Eksportuj
viewer-translate = Tłumacz
viewer-resize = Zmień rozmiar
viewer-identify-speakers = Identyfikuj mówców
viewer-find = Znajdź
viewer-find-label = Znajdź:
viewer-find-placeholder = Wprowadź tekst do wyszukania...
viewer-find-clear = Wyczyść
viewer-find-prev-tooltip = Poprzednie dopasowanie ({ $shortcut })
viewer-find-next-tooltip = Następne dopasowanie ({ $shortcut })
viewer-empty = Brak segmentów — dane wejściowe mogą zawierać samą ciszę
viewer-open-error = Nie można otworzyć transkrypcji
viewer-col-start = Rozpocznij
viewer-col-end = Zakończ
viewer-col-text = Tekst
viewer-col-translation = Tłumaczenie
viewer-saved = edytowano · zapisano
viewer-unsaved-warning = Nie udało się zapisać edycji — pozostaje w pamięci, ale nie została zapisana na dysku
viewer-player-loading = Wczytywanie dźwięku…
viewer-player-failed = Odtwarzanie niedostępne
viewer-playback-controls = Sterowanie odtwarzaniem:
viewer-loop-segment = Zapętlaj fragment
viewer-loop-segment-tooltip = Włącz/wyłącz zapętlanie po kliknięciu na fragmenty transkrypcji
viewer-follow-audio = Podążaj za dźwiękiem
viewer-follow-audio-tooltip = Włącz/wyłącz śledzenie bieżącej pozycji audio w transkrypcji. Gdy włączone, automatycznie przewija do bieżącego tekstu.
viewer-speed = Szybkość:
viewer-scroll-to-current = Przewiń do bieżącego
viewer-scroll-to-current-tooltip = Przewiń do aktualnie mówionego tekstu

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Wymagany klucz API
viewer-translate-key-required-body = Wprowadź klucz API OpenAI w ustawieniach
viewer-translate-settings-title = Ustawienia tłumaczenia AI

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Tekst
viewer-export-translation = Tłumaczenie
viewer-export-dialog-title = Zapisz plik
viewer-export-filter = Pliki tekstowe
viewer-export-failed = Nie udało się zapisać eksportu — szczegóły w dzienniku
export-written = Zapisano do { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Ustawienia
resizer-create-new = Utwórz nową transkrypcję
resizer-extend-heading = Wydłuż czas końcowy
resizer-extend-label = Wydłuż zakończenia o maksymalnie (sekundy)
resizer-extend-button = Wydłuż zakończenia
resizer-resize-heading = Opcje zmiany rozmiaru
resizer-resize-label = Żądana długość napisów
resizer-resize-button = Zmień rozmiar
resizer-resize-disabled-tooltip = Dostępne tylko jeśli znaczniki dla słów były wyłączone podczas transkrypcji
resizer-merge-heading = Opcje scalania
resizer-merge-by-gap = Scal według odstępu
resizer-split-by-punctuation = Podziel według interpunkcji
resizer-split-by-max-length = Podziel według maksymalnej długości
resizer-merge-button = Scal
resizer-merge-disabled-tooltip = Dostępne tylko jeśli znaczniki dla słów były włączone podczas transkrypcji
resizer-merging = Scalanie…
resizer-merge-error = Nie można wykonać scalenia:
resizer-save-error = Nie udało się zapisać zmian:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Krok 1: Identyfikuj mówców
speaker-id-step2-heading = Krok 2: Nazwij mówców
speaker-id-ready-label = Gotowy do identyfikacji mówców
speaker-id-missing-audio-label = Nie znaleziono pliku audio
speaker-id-identify-button = Identyfikuj
speaker-id-cancel-button = Anuluj
speaker-id-cancelling = Anulowanie...
speaker-id-decoding = Dekodowanie dźwięku…
speaker-id-downloading-models = Pobieranie modeli…
speaker-id-identifying = Identyfikowanie mówców…
speaker-id-play-sample = Odtwórz próbkę
speaker-id-merge-checkbox = Scal zdania mówcy
speaker-id-save-button = Zapisz
speaker-id-save-error = Nie udało się zapisać zmian:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Nagrywanie na żywo
recording-model-load-note = Pierwsze użycie modelu może potrwać kilka minut podczas ładowania.
recording-advanced-button = Zaawansowane...
recording-microphone-label = Mikrofon:
recording-record-button = Nagraj
recording-stop-button = Zatrzymaj
recording-realtime-warning = { $size } przetranskrybowano z prędkością { $ratio }× czasu rzeczywistego przy ostatnim uruchomieniu — transkrypcja na żywo może zacząć się opóźniać
recording-chunk-skipped-warning = Fragment nie powiódł się i został pominięty: { $detail }
recording-panel-placeholder = Naciśnij Nagraj, aby zacząć...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Oczekiwanie na tłumaczenie AI...
recording-model-downloading = Pobieranie modelu… { $percent }%
recording-model-downloading-indeterminate = Pobieranie modelu…
recording-model-loading = Wczytywanie modelu…
recording-error-prefix = Wystąpił błąd podczas rozpoczęcia nowego nagrania:
recording-error-suffix = Sprawdź urządzenia audio lub przejrzyj logi aplikacji, by uzyskać więcej informacji.
recording-thread-crashed = Sesja nagrywania zakończyła się nieoczekiwanie. Sprawdź dzienniki aplikacji, aby uzyskać więcej informacji.
meter-average-volume = Średnia głośność
meter-queue = Kolejka

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Ustawienia zaawansowane
recording-advanced-speech-heading = Ustawienia rozpoznawania mowy
recording-advanced-initial-prompt-label = Wstępne instrukcje:
recording-advanced-translation-heading = Ustawienia tłumaczenia
recording-advanced-llm-enable = Włącz tłumaczenie AI
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Ustawienia nagrywania
recording-advanced-silence-threshold-label = Próg ciszy:
recording-advanced-mode-label = Tryb nagrywania na żywo:
recording-advanced-mode-append-below = Dodaj poniżej
recording-advanced-mode-append-above = Dodaj powyżej
recording-advanced-mode-append-and-correct = Dodaj i popraw
recording-advanced-separator-label = Separator wierszy:
recording-advanced-step-label = Krok transkrypcji:
recording-advanced-hide-unconfirmed = Ukryj niepotwierdzone
recording-advanced-export-enable = Włącz eksport nagrywania na żywo
recording-advanced-export-folder-label = Folder eksportu:
recording-advanced-export-browse = Przeglądaj
recording-advanced-export-file-name-label = Nazwa pliku eksportu:
recording-advanced-export-file-type-label = Typ pliku eksportu:
recording-advanced-export-file-type-txt = Plik tekstowy (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Ogranicz wpisy eksportu
    (0 = eksportuj wszystkie):
recording-advanced-upload-url-label = URL przesyłania:
recording-advanced-ok = OK

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Pokaż
presentation-show-tooltip = Pokaż w nowym oknie
presentation-text-size-label = Rozmiar tekstu:
presentation-theme-label = Motyw
presentation-theme-light = Jasny
presentation-theme-dark = Ciemny
presentation-theme-custom = Niestandardowy
presentation-text-color-label = Kolor tekstu
presentation-background-color-label = Kolor tła
presentation-fullscreen-button = Pełny ekran
presentation-window-title = Prezentacja transkrypcji na żywo

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Kopiuj
recording-copy-tooltip = Kopiuj transkrypcję do schowka
recording-copy-copied = Skopiowano!
recording-copy-nothing = Nie ma nic do skopiowania!

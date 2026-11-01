## humm-gui — Ukrainian (uk) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 15, design spec
## §7.2, batch 3 of 3). Section comments stay in English on purpose: they
## are developer-facing notes about *where* a key renders, identical in
## every catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own uk_UA translation is used
## verbatim (`buzz/locale/uk_UA/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh, matching
## the register the Buzz-sourced strings already establish. Buzz's own
## uk_UA po consistently uses the formal, capitalized "Ви" register
## throughout its own translations (e.g. "Ви впевнені, що хочете
## видалити...", "Введіть нову назву:") — this catalog stays formal
## consistently, including for the fresh humm-only strings. Buzz's own
## uk_UA translations render "transcribe" as "Розпізнати" (recognize) for
## the live action but "транскрипція"/"транскрипції" for the transcription/
## transcript object (e.g. `toolbar-cancel`'s "Скасувати транскрипцію");
## this catalog follows both halves of that split consistently in its own
## fresh strings, and uses "тека" (Buzz's own choice, e.g.
## `prefs-fw-enable`'s "стеження за текою") rather than "папка" for
## "folder" throughout.
##
## Buzz's own uk_UA po translates the msgid "Ok" (its own relabelled dialog
## button text) as "Гаразд". Following the es/zh-CN/M8-Task-13/14 precedent
## (Buzz's own lineage, not a translation choice made here), all four of
## clear-confirm-ok, dialog-ok, url-dialog-ok and recording-advanced-ok
## collapse to that same "Гаразд".
##
## Six Buzz msgids are known (from the M8 Task 13 batch) to sometimes carry
## two translation variants concatenated with no separator, an upstream Buzz
## artifact rather than a translation choice. Three of the six are affected
## in uk_UA's own po and are fixed here to the second (revised) variant
## alone: `prefs-general-force-cpu-tooltip` (doubles "Встановіть це..."
## then "Увімкніть це..." with no separator — the second, revised
## imperative is kept, and also swaps "зависає"/"падає" for "crashes"),
## `resizer-resize-disabled-tooltip` and `resizer-merge-disabled-tooltip`
## (each doubles "часові позначки на рівні слів" then the shorter
## "хронометраж на рівні слів" for "word level timings" — the second,
## revised half is kept in each case). The other three of the six are
## clean (single variant, no concatenation) in uk_UA's own po and are used
## as-is: `recording-model-load-note`, `viewer-loop-segment-tooltip`,
## `prefs-general-key-save-error-body`.
##
## Two further Buzz msgids match humm strings exactly but carry a truly
## *empty* msgstr in uk_UA's po (untranslated upstream, gettext falls back
## silently): `prefs-postproc-skip-check-files-desc` and `prefs-postproc-
## skip-check-db-desc`. Both are translated fresh here rather than left
## blank — the same two keys every prior M8 locale batch has found empty.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in uk_UA (API, GPU, CPU,
## URL, Whisper, TXT/SRT/VTT) are left alone here too, matching Buzz's own
## uk_UA po (e.g. "API-ключ OpenAI", "Базовий URL API"); "Backend" is
## transliterated as "Бекенд" (no matching Buzz msgid to anchor a
## translated alternative; a common loanword rendering in Ukrainian
## software, mirroring the zh-CN/ru catalogs' own choice to transliterate
## rather than keep the Latin word, for the same non-Latin-script reason).

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Англійська
locale-name-ca-es = Каталонська
locale-name-da-dk = Датська
locale-name-nl = Нідерландська
locale-name-fr = Французька
locale-name-de-de = Німецька
locale-name-es-es = Іспанська
locale-name-it-it = Італійська
locale-name-ja-jp = Японська
locale-name-lv-lv = Латвійська
locale-name-pl-pl = Польська
locale-name-pt-br = Португальська (Бразилія)
locale-name-ru = Російська
locale-name-tr-tr = Турецька
locale-name-uk-ua = Українська
locale-name-zh-cn = Китайська (спрощена)
locale-name-zh-tw = Китайська (традиційна)

## Empty-state hero (views/main_window.rs)

hero-drop = Перетягніть медіафайл сюди або натисніть %shortcut
hero-import-button = Імпортувати файли…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Назва файлу / посилання
table-col-model = Модель
table-col-task = Завдання
table-col-status = Стан
table-col-date-completed = Дата завершення
table-col-date-added = Дата додавання
table-col-notes = Нотатки

## Task table — header/row context menus

table-menu-reset-column-order = Скинути порядок стовпців
table-menu-restart = Перезапустити транскрипцію
table-menu-rename = Перейменувати
table-menu-notes = Додати/редагувати нотатки

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Розпізнати
task-translate = Перекласти на англійську

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = виділення мовлення

## Status words and notices added with the UX review fixes.
status-queued = У черзі
status-canceled = Скасовано
status-skipped = Пропущено
status-completed = Завершено
status-failed = Помилка
status-in-progress = Виконується
status-eta = залишилось
status-downloading-model = Завантаження моделі
status-downloading-media = Завантаження медіа
status-reducing-noise = зменшення шуму
table-menu-show-error = Показати помилку
toast-written-to = Записано в
toast-postproc-written = Записано додаткові файли
toast-postproc-errors = Крок постобробки повідомив про помилку. Транскрипцію все одно завершено.
toast-reduction-skipped = Зменшення шуму не виконано; транскрипція продовжилась.
options-word-timestamps-tooltip = Зміну розміру пізніше доступно лише якщо часові мітки слів було вимкнено під час транскрипції. Об’єднання — лише якщо їх було увімкнено.
options-model-will-download = Запуск спочатку завантажить цю модель. Вікно лишається доступним.
prefs-postproc-dfn-runs-before = DeepFilterNet виконується до транскрипції і замінює аудіо, яке чує модель.
prefs-postproc-dfn-keep-name = Збереження файлу без шуму залишає файл <name>_DeepFilterNet3.wav поруч із джерелом.
prefs-general-export-template-tokens-intro = Пробіли всередині дужок є частиною кожного токена.
prefs-general-export-template-model-type = model_type завжди дорівнює Whisper.cpp.
prefs-general-cookie-label = Файл cookie
prefs-general-cookie-browse = Огляд…
prefs-general-cookie-caption = HUMM_DOWNLOAD_COOKIEFILE має пріоритет, якщо її задано. Порожнє поле очищає значення.
prefs-saved = Збережено
prefs-save-failed = Не вдалося зберегти налаштування:
viewer-saved-neighbor = сусідню мітку скориговано
viewer-timestamp-unparsed = Попередній час збережено. Це був не час.
viewer-timestamp-range = Попередній час збережено. Початок має бути раніше за кінець.
speaker-id-save-copies = Зберегти копіює цю транскрипцію в новий завершений рядок. Оригінал лишається як є.
speaker-id-merge-tooltip = Послідовні рядки одного мовця об’єднуються.
speaker-id-takes-minutes = Це може тривати кілька хвилин. Скасування відкидає запуск.
already-running = humm уже запущено.
already-running-with-files = humm уже запущено. Файли, передані цьому запуску, не відкрито. Імпортуйте їх із вікна, яке вже відкрите.
banner-buzz-imported = Імпортовано транскрипцій із Buzz: %n.
banner-buzz-import-failed = Не вдалося імпортувати транскрипції з Buzz. humm спробує знову під час наступного запуску.

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Записати
toolbar-new-file = Нова транскрипція файлу
toolbar-new-url = Нова транскрипція за адресою
toolbar-open-transcript = Відкрити транскрипцію
toolbar-open-transcript-tooltip = Виберіть одну або кілька завершених транскрипцій, щоб відкрити
toolbar-cancel = Скасувати транскрипцію
toolbar-clear = Очистити історію

## Menu bar (views/main_window.rs, F-004)

menu-file = Файл
menu-file-import-file = Імпортувати файл…
menu-file-import-url = Імпортувати адресу…
menu-file-import-folder = Імпортувати теку…
menu-help-title = Допомога
menu-help-about = Про humm
menu-help-docs = Допомога
menu-help-docs-tooltip = У програмі немає сторінки довідки.
menu-help-preferences = Налаштування…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Очистити історію
clear-confirm-body = Ви впевнені, що хочете видалити вибрані транскрипції? Це незворотна дія.
clear-confirm-ok = Гаразд
clear-confirm-cancel = Скасувати
clear-error = Не вдалося видалити вибрані транскрипції

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Перейменувати транскрипцію
rename-label = Введіть нову назву:
rename-error = Не вдалося перейменувати
notes-title = Нотатки
notes-label = Введіть відповідні нотатки для цієї транскрипції:
notes-error = Не вдалося зберегти нотатки
dialog-ok = Гаразд
dialog-cancel = Скасувати

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Транскрипцію завершено
toast-completed-suffix = файл(и) записано
toast-canceled = Транскрипцію скасовано
toast-detail-title = Помилка транскрипції
toast-detail-open-log = Відкрити теку журналу
toast-detail-retry = Повторити
toast-detail-close = Закрити
retry-error-unavailable = Не вдалося перезапустити: збережені параметри недоступні

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm не зміг записати оновлення історії — експорт усе одно завершується; див. файл журналу

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Перетягніть, щоб імпортувати

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp відсутній — встановіть його (наприклад, `brew install yt-dlp`) або задайте HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Імпортувати адресу
url-dialog-label = Адреса:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = Адреса, яку ви ввели, є недійсною
url-dialog-ok = Гаразд
url-dialog-cancel = Скасувати

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Модель:
options-task-label = Завдання:
options-language-label = Мова:
language-detect = Визначити мову
options-word-timestamps = Хронометраж на рівні слів
options-extract-speech = Витягти мовлення
options-extract-speech-missing-tooltip = demucs не знайдено — встановіть його (наприклад, `pipx install demucs`), щоб увімкнути виділення мовлення
options-export-label = Експорт:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = формат експорту не вибрано
options-advanced = Додатково
options-initial-prompt-label = Початкова підказка:
options-hf-repo-label = Модель Hugging Face
options-run = Запуск

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Бекенд:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = API-ключ:
options-llm-heading = Налаштування перекладу
options-llm-enable = Увімкнути переклад ШІ
options-llm-model-label = Модель ШІ
options-llm-model-presets-button = Пресети
options-llm-prompt-label = Інструкції для ШІ:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Стеження за { $name }
folderwatch-pill-paused = Призупинено
folderwatch-pill-pause-tip = Призупинити стеження за текою
folderwatch-pill-resume-tip = Відновити стеження за текою

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Доступне оновлення
update-dialog-title = Доступне оновлення
update-dialog-header = Доступна нова версія humm!
update-dialog-current-version = Поточна версія:
update-dialog-new-version = Нова версія:
update-dialog-release-notes = Примітки до випуску:
update-dialog-open-release-page = Відкрити сторінку релізу
update-dialog-close = Закрити

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Версія
about-check-button = Перевірити оновлення
about-show-logs-button = Показати журнали
about-close-button = Закрити
about-up-to-date = У вас актуальна версія!
about-check-failed = Не вдалося перевірити оновлення — див. файл журналу

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Налаштування
prefs-tab-general = Загальне
prefs-tab-models = Моделі
prefs-tab-shortcuts = Клавіатурні скорочення
prefs-tab-folder-watch = Нагляд за текою
prefs-tab-postproc = Постобробка
prefs-tab-privacy = Конфіденційність

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Мова інтерфейсу
prefs-general-font-size-label = Розмір шрифту
prefs-general-font-size-custom-label = Власний розмір шрифту
prefs-general-openai-key-label = API-ключ OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Показати
prefs-general-key-hide = Приховати
prefs-general-test-button = Тест
prefs-general-test-title = Тест API-ключа OpenAI
prefs-general-test-success-body = Ваш API-ключ дійсний. humm використає цей ключ для транскрипції з Whisper API та перекладу ШІ.
prefs-general-invalid-key-title = Недійсний API-ключ
prefs-general-invalid-key-body = API підтримує лише символи base64 (A-Za-z0-9+/=_-). Інші символи в API-ключі можуть спричиняти помилки.
prefs-general-key-save-error-title = Помилка
prefs-general-key-save-error-body = Не вдається додати до звʼязки ключів API-ключ OpenAI
prefs-general-base-url-label = Базова адреса OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Модель API OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Типова назва файлу експорту
prefs-general-force-cpu-row-label = Вимкнути GPU
prefs-general-force-cpu-label = Використовувати лише CPU та вимкнути прискорення GPU
prefs-general-force-cpu-tooltip = Увімкніть це, якщо великі моделі не поміщаються в пам'ять GPU і humm падає

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Надсилати анонімну телеметрію запуску
privacy-telemetry-disabled-caption = У цій збірці телеметрія вимкнена на етапі компіляції.
privacy-telemetry-items-intro = Кожна подія запуску містить:
privacy-telemetry-item-app-version = Версія застосунку
privacy-telemetry-item-system-name = Назва системи
privacy-telemetry-item-os-version = Версія ОС
privacy-telemetry-item-kernel-release = Версія ядра
privacy-telemetry-item-cpu-architecture = Архітектура процесора
privacy-telemetry-item-system-locale = Системна локаль
privacy-telemetry-item-identifier = Випадковий анонімний ідентифікатор
privacy-update-checkbox = Автоматично перевіряти оновлення

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Завантажене
models-section-available = Доступно для завантаження
models-download-button = Завантажити
models-delete-button = Видалити
models-show-location-button = Показати розташування файлу
models-cancel-button = Скасувати
models-custom-url-label = Власний URL моделі ggml:
models-delete-confirm-title = Видалити модель
models-delete-confirm-body = Ви впевнені, що хочете видалити вибрану модель?
models-download-error-title = Помилка
models-download-failed = Невдале завантаження
models-speed-badge = { $ratio }× реального часу
models-speed-badge-tip = Виміряна швидкість реального часу за сеансами живого запису на цьому пристрої

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Відкрити вікно запису
shortcut-desc-import-file = Імпортувати файл
shortcut-desc-import-url = Імпортувати адресу
shortcut-desc-open-preferences-window = Відкрити вікно налаштувань
shortcut-desc-view-transcript-text = Переглянути текст транскрипції
shortcut-desc-view-transcript-translation = Переглянути переклад транскрипції
shortcut-desc-view-transcript-timestamps = Переглянути позначки часу в транскрипції
shortcut-desc-search-transcript = Пошук у транскрипції
shortcut-desc-search-next = Перейти до наступного результату пошуку в транскрипції
shortcut-desc-search-previous = Перейти до попереднього результату пошуку в транскрипції
shortcut-desc-scroll-to-current-text = Прокрутити до поточного тексту
shortcut-desc-play-pause-audio = Відтворити/призупинити аудіо
shortcut-desc-replay-current-segment = Повторити поточний фрагмент
shortcut-desc-toggle-playback-controls = Перемкнути елементи керування відтворенням
shortcut-desc-decrease-segment-start = Зменшити час початку фрагмента
shortcut-desc-increase-segment-start = Збільшити час початку фрагмента
shortcut-desc-decrease-segment-end = Зменшити час завершення фрагмента
shortcut-desc-increase-segment-end = Збільшити час завершення фрагмента
shortcut-desc-clear-history = Очистити історію
shortcut-desc-stop-transcription = Скасувати транскрипцію
prefs-shortcuts-press = Натисніть комбінацію клавіш…
prefs-shortcuts-clear-tooltip = Скинути до типового
prefs-shortcuts-conflict = Конфлікт з { $other }
prefs-shortcuts-reset = Типові значення

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Увімкнути стеження за текою
prefs-fw-input-folder-label = Тека введення
prefs-fw-input-folder-placeholder = /шлях/до/вхідної/теки
prefs-fw-output-folder-label = Папка виводу
prefs-fw-output-folder-placeholder = /шлях/до/вихідної/теки
prefs-fw-browse = Огляд
prefs-fw-delete-checkbox = Видалити оброблені файли
prefs-fw-disposition-note = Після транскрипції вихідний файл переміщується у вихідну теку — або видаляється, якщо позначено «Видаляти оброблені файли».

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Пропуск, зменшення шуму й визначення мови виконуються до транскрипції. Зміна розміру, зведення й експорт DOCX — після. Помилка записується в журнал і ніколи не зриває транскрипцію.
prefs-postproc-skip-title = Пропустити вже транскрибовані
prefs-postproc-skip-enable = Увімкнути пропуск уже транскрибованих
prefs-postproc-skip-check-files = Перевірити наявність файлів результатів
prefs-postproc-skip-check-files-desc = Пропустити, якщо поруч з аудіофайлом знайдено відповідний файл .txt, .srt або .vtt
prefs-postproc-skip-check-db = Перевірити в базі даних транскрипцій
prefs-postproc-skip-check-db-desc = Пропустити, якщо цей файл вже транскрибовано і запис існує в базі даних
prefs-postproc-dfn-title = Пригнічення шуму DeepFilterNet
prefs-postproc-dfn-enable = Увімкнути шумозаглушення DeepFilterNet
prefs-postproc-dfn-keep-file = Зберегти файл без шуму після транскрипції
prefs-postproc-langdetect-title = Покращене визначення мови
prefs-postproc-langdetect-enable = Увімкнути розширене визначення мови
prefs-postproc-langdetect-download-tiny = Завантажити модель tiny, якщо жодної немає
prefs-postproc-langdetect-download-tiny-desc = Якщо не завантажено жодної моделі whisper.cpp, завантажує модель tiny для визначення мови.
prefs-postproc-resizer-title = Автоматична зміна розміру транскрипції
prefs-postproc-resizer-enable = Увімкнути автоматичний засіб зміни розміру транскрипції
prefs-postproc-resizer-merge-by-gap = Об'єднувати за паузою
prefs-postproc-resizer-merge-gap-seconds = Пауза об'єднання (секунди)
prefs-postproc-resizer-split-by-punctuation = Розділяти за пунктуацією
prefs-postproc-resizer-punctuation = Пунктуація
prefs-postproc-resizer-split-by-max-length = Розділяти за максимальною довжиною
prefs-postproc-resizer-max-length = Максимальна довжина субтитра
prefs-postproc-summary-title = ШІ-резюме
prefs-postproc-summary-enable = Увімкнути AI-резюме
prefs-postproc-summary-api-url = Базовий URL API
prefs-postproc-summary-api-key-label = Ключ API
prefs-postproc-summary-model = Модель
prefs-postproc-summary-prompt = Запит для резюме
prefs-postproc-summary-save-to-notes = Зберегти резюме в нотатки
prefs-postproc-summary-save-to-file = Зберегти резюме у файл
prefs-postproc-summary-output-folder = Папка виводу (для файлу)
prefs-postproc-summary-key-save-error-body = Не вдалося зберегти API-ключ AI-резюме у зв'язці ключів
prefs-postproc-docx-title = Експорт у DOCX
prefs-postproc-docx-enable = Увімкнути експорт у DOCX
prefs-postproc-docx-output-folder = Папка виводу
prefs-postproc-docx-include-timestamps = Включити часові позначки
prefs-postproc-output-folder-hint = Залиште порожнім, щоб зберегти поруч із вихідним файлом.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Вигляд
viewer-mode-text = Текст
viewer-mode-translation = Переклад
viewer-mode-timestamps = Позначки часу
viewer-export = Експорт
viewer-translate = Перекласти
viewer-resize = Змінити розмір
viewer-identify-speakers = Визначити мовців
viewer-find = Знайти
viewer-find-label = Знайти:
viewer-find-placeholder = Введіть текст для пошуку...
viewer-find-clear = Очистити
viewer-find-prev-tooltip = Попередній збіг ({ $shortcut })
viewer-find-next-tooltip = Наступний збіг ({ $shortcut })
viewer-empty = Немає сегментів — можливо, у вхідних даних лише тиша
viewer-open-error = Не вдалося відкрити транскрипцію
viewer-col-start = Початок
viewer-col-end = Кінець
viewer-col-text = Текст
viewer-col-translation = Переклад
viewer-saved = змінено · збережено
viewer-unsaved-warning = Зміну не вдалося зберегти — вона зберігається в пам'яті, але не записана на диск
viewer-player-loading = Завантаження аудіо…
viewer-player-failed = Відтворення недоступне
viewer-playback-controls = Керування відтворенням:
viewer-loop-segment = Повторювати фрагмент
viewer-loop-segment-tooltip = Увімкнути/вимкнути повторення при натисканні на фрагменти транскрипції
viewer-follow-audio = Стежити за аудіо
viewer-follow-audio-tooltip = Увімкнути/вимкнути відстеження поточної позиції аудіо в транскрипції. Коли увімкнено, автоматично прокручує до поточного тексту.
viewer-speed = Швидкість:
viewer-scroll-to-current = Прокрутити до поточного
viewer-scroll-to-current-tooltip = Прокрутити до тексту, що зараз вимовляється

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Потрібен API-ключ
viewer-translate-key-required-body = Будь ласка, введіть API-ключ OpenAI в налаштуваннях
viewer-translate-settings-title = Налаштування AI-перекладу

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Текст
viewer-export-translation = Переклад
viewer-export-dialog-title = Зберегти файл
viewer-export-filter = Текстові файли
viewer-export-failed = Не вдалося записати експорт — деталі див. у журналі
export-written = Записано в { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Налаштування
resizer-create-new = Створити нову транскрипцію
resizer-extend-heading = Продовжити час завершення
resizer-extend-label = Продовжити закінчення до (секунд)
resizer-extend-button = Продовжити закінчення
resizer-resize-heading = Параметри зміни розміру
resizer-resize-label = Бажана довжина субтитрів
resizer-resize-button = Змінити розмір
resizer-resize-disabled-tooltip = Доступно лише якщо хронометраж на рівні слів був вимкнений під час транскрипції
resizer-merge-heading = Параметри об'єднання
resizer-merge-by-gap = Об'єднувати за паузою
resizer-split-by-punctuation = Розділяти за пунктуацією
resizer-split-by-max-length = Розділяти за максимальною довжиною
resizer-merge-button = Об'єднати
resizer-merge-disabled-tooltip = Доступно лише якщо хронометраж на рівні слів був увімкнений під час транскрипції
resizer-merging = Об'єднання…
resizer-merge-error = Не вдалося виконати об'єднання:
resizer-save-error = Не вдалося зберегти зміни:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Крок 1: Визначити мовців
speaker-id-step2-heading = Крок 2: Назвати мовців
speaker-id-ready-label = Готово до визначення мовців
speaker-id-missing-audio-label = Аудіофайл не знайдено
speaker-id-identify-button = Визначити
speaker-id-cancel-button = Скасувати
speaker-id-cancelling = Скасування...
speaker-id-decoding = Декодування аудіо…
speaker-id-downloading-models = Завантаження моделей…
speaker-id-identifying = Визначення мовців…
speaker-id-play-sample = Відтворити зразок
speaker-id-merge-checkbox = Об'єднати речення мовців
speaker-id-save-button = Зберегти
speaker-id-save-error = Не вдалося зберегти зміни:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Живий запис
recording-model-load-note = Перше використання моделі може тривати кілька хвилин для завантаження.
recording-advanced-button = Додатково...
recording-microphone-label = Мікрофон:
recording-record-button = Записати
recording-stop-button = Зупинити
recording-realtime-warning = { $size } транскрибовано зі швидкістю { $ratio }× реального часу під час останнього запуску — жива транскрипція може почати відставати
recording-chunk-skipped-warning = Фрагмент завершився помилкою і був пропущений: { $detail }
recording-panel-placeholder = Натисніть на Запис, щоб розпочати...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Очікування перекладу від ШІ...
recording-model-downloading = Завантаження моделі… { $percent }%
recording-model-downloading-indeterminate = Завантаження моделі…
recording-model-loading = Завантаження моделі…
recording-error-prefix = При старті нового запису виникла помилка:
recording-error-suffix = Будь ласка, перевірте свої аудіопристрої або пошукайте додаткову інформацію в звітах програми.
recording-thread-crashed = Сеанс запису неочікувано завершився. Перевірте журнали застосунку, щоб дізнатися більше.
meter-average-volume = Середній рівень гучності
meter-queue = Черга

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Додаткові налаштування
recording-advanced-speech-heading = Параметри розпізнавання мовлення
recording-advanced-initial-prompt-label = Початкова підказка:
recording-advanced-translation-heading = Налаштування перекладу
recording-advanced-llm-enable = Увімкнути переклад ШІ
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Налаштування запису
recording-advanced-silence-threshold-label = Поріг тиші:
recording-advanced-mode-label = Режим живого запису:
recording-advanced-mode-append-below = Додати знизу
recording-advanced-mode-append-above = Додати зверху
recording-advanced-mode-append-and-correct = Додати та виправити
recording-advanced-separator-label = Роздільник рядків:
recording-advanced-step-label = Крок транскрибування:
recording-advanced-hide-unconfirmed = Приховати непідтверджене
recording-advanced-export-enable = Увімкнути експорт живого запису
recording-advanced-export-folder-label = Тека для експорту:
recording-advanced-export-browse = Огляд
recording-advanced-export-file-name-label = Назва файлу експорту:
recording-advanced-export-file-type-label = Тип файлу експорту:
recording-advanced-export-file-type-txt = Текстовий файл (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Обмежити кількість записів експорту
    (0 = експортувати всі):
recording-advanced-upload-url-label = URL для завантаження:
recording-advanced-ok = Гаразд

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Показати
presentation-show-tooltip = Показати у новому вікні
presentation-text-size-label = Розмір тексту:
presentation-theme-label = Тема
presentation-theme-light = Світла
presentation-theme-dark = Темна
presentation-theme-custom = Власна
presentation-text-color-label = Колір тексту
presentation-background-color-label = Колір фону
presentation-fullscreen-button = Повноекранний режим
presentation-window-title = Презентація транскрипції в реальному часі

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Копіювати
recording-copy-tooltip = Копіювати транскрипцію до буфера обміну
recording-copy-copied = Скопійовано!
recording-copy-nothing = Нічого копіювати!

## humm-gui — Russian (ru) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 14, design spec
## §7.2). Section comments stay in English on purpose: they are
## developer-facing notes about *where* a key renders, identical in every
## catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own ru translation is used
## verbatim (`buzz/locale/ru/LC_MESSAGES/buzz.po`) so terminology and muscle
## memory carry over; everything else is translated fresh, matching the
## register the Buzz-sourced strings already establish. Buzz's own ru po
## consistently uses the formal, capitalized "Вы" register throughout its
## own translations (e.g. "Вы уверены, что хотите удалить...", "Введите
## новое имя:") — this catalog stays formal consistently, including for the
## fresh humm-only strings.
##
## Buzz's own ru po translates the msgid "Ok" (its own relabelled dialog
## button text) as "ОК" (Cyrillic О). Following the es/zh-CN/M8-Task-13
## precedent (Buzz's own lineage, not a translation choice made here), all
## four of clear-confirm-ok, dialog-ok, url-dialog-ok and recording-advanced-
## ok collapse to that same "ОК".
##
## Six Buzz msgids are known (from the M8 Task 13 batch) to sometimes carry
## two translation variants concatenated with no separator, an upstream Buzz
## artifact rather than a translation choice. Three of the six are affected
## in ru's own po and are fixed here to the second (revised) variant alone:
## `prefs-general-force-cpu-tooltip` (doubles "Установите это..." then
## "Включите это..." with no separator — the second, revised imperative is
## kept), `resizer-resize-disabled-tooltip` and `resizer-merge-disabled-
## tooltip` (each doubles two genuinely different phrasings, e.g. "временны́е
## метки на уровне слов" vs the shorter "тайминги на уровне слов" — the
## second, revised half is kept in each case). The other three of the six
## are clean (single variant, no concatenation) in ru's own po and are used
## as-is: `recording-model-load-note`, `viewer-loop-segment-tooltip`,
## `prefs-general-key-save-error-body`.
##
## Two further Buzz msgids match humm strings exactly but carry a truly
## *empty* msgstr in ru's po (untranslated upstream, gettext falls back
## silently): `prefs-postproc-skip-check-files-desc` and `prefs-postproc-
## skip-check-db-desc`. Both are translated fresh here rather than left
## blank — the same two keys M8 Task 13 found empty in all four of its own
## locales.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in ru (API, GPU, CPU,
## Whisper, URL, TXT/SRT/VTT) are left alone here too (transliterated where
## Buzz's own po already does so, e.g. "API-ключ"); "Backend" is
## transliterated as "Бэкенд" (a common loanword rendering in Russian
## software), matching the zh-CN catalog's own choice to transliterate
## rather than keep the Latin word, for the same reason (a non-Latin
## script). Downloading vs. loading a model are kept distinct throughout
## ("Скачивание"/"скачать" for network downloads, "Загрузка"/"загрузить" for
## reading a model into memory), mirroring en's own "Downloading model…" vs
## "Loading model…" distinction and Buzz's own ru anchor translation of
## "Download" as "Скачать".

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Английский
locale-name-ca-es = Каталанский
locale-name-da-dk = Датский
locale-name-nl = Нидерландский
locale-name-fr = Французский
locale-name-de-de = Немецкий
locale-name-es-es = Испанский
locale-name-it-it = Итальянский
locale-name-ja-jp = Японский
locale-name-lv-lv = Латышский
locale-name-pl-pl = Польский
locale-name-pt-br = Португальский (Бразилия)
locale-name-ru = Русский
locale-name-tr-tr = Турецкий
locale-name-uk-ua = Украинский
locale-name-zh-cn = Китайский (упрощённый)
locale-name-zh-tw = Китайский (традиционный)

## Empty-state hero (views/main_window.rs)

hero-drop = Перетащите медиафайл сюда или нажмите ⌘O
hero-import-button = Импортировать файлы…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Имя файла / Ссылка
table-col-model = Модель
table-col-task = Задача
table-col-status = Статус
table-col-date-completed = Дата завершения
table-col-date-added = Дата добавления
table-col-notes = Заметки

## Task table — header/row context menus

table-menu-reset-column-order = Сбросить порядок столбцов
table-menu-restart = Перезапустить транскрипцию
table-menu-rename = Переименовать
table-menu-notes = Добавить/изменить заметки

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Транскрибировать
task-translate = Перевести на английский

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = выделение речи

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Записать
toolbar-new-file = Новая транскрипция файла
toolbar-new-url = Новая транскрипция по ссылке
toolbar-open-transcript = Открыть транскрипцию
toolbar-open-transcript-tooltip = Выберите одну или несколько завершённых транскрипций, чтобы открыть их
toolbar-cancel = Отменить транскрипцию
toolbar-clear = Очистить историю

## Menu bar (views/main_window.rs, F-004)

menu-file = Файл
menu-file-import-file = Импортировать файл…
menu-file-import-url = Импортировать ссылку…
menu-file-import-folder = Импортировать папку…
menu-help-title = Справка
menu-help-about = О humm
menu-help-docs = Справка
menu-help-docs-tooltip = Документация появится с первым релизом
menu-help-preferences = Настройки…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Очистить историю
clear-confirm-body = Вы уверены, что хотите удалить выбранные транскрипции? Это действие нельзя отменить.
clear-confirm-ok = ОК
clear-confirm-cancel = Отмена
clear-error = Не удалось удалить выбранные транскрипции

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Переименовать транскрипцию
rename-label = Введите новое имя:
rename-error = Не удалось переименовать
notes-title = Заметки
notes-label = Введите соответствующие заметки для этой транскрипции:
notes-error = Не удалось сохранить заметки
dialog-ok = ОК
dialog-cancel = Отмена

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Транскрипция завершена
toast-completed-suffix = файл(ов) записано
toast-canceled = Транскрипция отменена
toast-detail-title = Ошибка транскрипции
toast-detail-open-log = Открыть папку с журналом
toast-detail-retry = Повторить
toast-detail-close = Закрыть
retry-error-unavailable = Не удалось перезапустить: сохранённые параметры недоступны

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm не смог записать обновления истории — экспорт всё равно завершён; см. файл журнала

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Перетащите для импорта

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp не найден — установите его (например, `brew install yt-dlp`) или задайте HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Импортировать ссылку
url-dialog-label = Ссылка:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = Введённая вами ссылка недействительна.
url-dialog-ok = ОК
url-dialog-cancel = Отмена

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Модель:
options-task-label = Задача:
options-language-label = Язык:
language-detect = Определить язык
options-word-timestamps = Тайминги на уровне слов
options-extract-speech = Извлечь речь
options-extract-speech-missing-tooltip = demucs не найден — установите его (например, `pipx install demucs`), чтобы включить выделение речи
options-export-label = Экспорт:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = формат экспорта не выбран
options-advanced = Дополнительно
options-initial-prompt-label = Начальная подсказка:
options-hf-repo-label = Модель Hugging Face
options-run = Запустить

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Бэкенд:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = API-ключ:
options-llm-heading = Настройки перевода
options-llm-enable = Включить перевод с помощью ИИ
options-llm-model-label = Модель ИИ:
options-llm-model-presets-button = Пресеты
options-llm-prompt-label = Инструкции для ИИ:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Отслеживание { $name }
folderwatch-pill-paused = Приостановлено
folderwatch-pill-pause-tip = Приостановить отслеживание папки
folderwatch-pill-resume-tip = Возобновить отслеживание папки

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Доступно обновление
update-dialog-title = Доступно обновление
update-dialog-header = Доступна новая версия humm!
update-dialog-current-version = Текущая версия:
update-dialog-new-version = Новая версия:
update-dialog-release-notes = Примечания к выпуску:
update-dialog-open-release-page = Открыть страницу релиза
update-dialog-close = Закрыть

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Версия
about-check-button = Проверить обновления
about-show-logs-button = Показать журналы
about-close-button = Закрыть
about-up-to-date = У вас актуальная версия!
about-check-failed = Не удалось проверить обновления — см. файл журнала

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Настройки
prefs-tab-general = Общие
prefs-tab-models = Модели
prefs-tab-shortcuts = Горячие клавиши
prefs-tab-folder-watch = Отслеживание папки
prefs-tab-postproc = Постобработка
prefs-tab-privacy = Конфиденциальность

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Язык интерфейса
prefs-general-font-size-label = Размер шрифта
prefs-general-font-size-custom-label = Свой размер шрифта
prefs-general-openai-key-label = API-ключ OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Показать
prefs-general-key-hide = Скрыть
prefs-general-test-button = Проверка
prefs-general-test-title = Проверка API-ключа OpenAI
prefs-general-test-success-body = Ваш API-ключ действителен. humm будет использовать этот ключ для транскрипции через Whisper API и перевода с помощью ИИ.
prefs-general-invalid-key-title = Недействительный API-ключ
prefs-general-invalid-key-body = API поддерживает только символы base64 (A-Za-z0-9+/=_-). Другие символы в API-ключе могут вызывать ошибки.
prefs-general-key-save-error-title = Ошибка
prefs-general-key-save-error-body = Не удалось сохранить API-ключ OpenAI в связке ключей
prefs-general-base-url-label = Базовый адрес OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Модель API OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Имя файла экспорта по умолчанию
prefs-general-force-cpu-row-label = Отключить GPU
prefs-general-force-cpu-label = Использовать только CPU и отключить ускорение GPU
prefs-general-force-cpu-tooltip = Включите это, если большие модели не помещаются в память GPU и humm вылетает

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Отправлять анонимную телеметрию запуска
privacy-telemetry-disabled-caption = В этой сборке телеметрия отключена на этапе компиляции.
privacy-telemetry-items-intro = Каждое событие запуска включает:
privacy-telemetry-item-app-version = Версия приложения
privacy-telemetry-item-system-name = Название системы
privacy-telemetry-item-os-version = Версия ОС
privacy-telemetry-item-kernel-release = Версия ядра
privacy-telemetry-item-cpu-architecture = Архитектура процессора
privacy-telemetry-item-system-locale = Системная локаль
privacy-telemetry-item-identifier = Случайный анонимный идентификатор
privacy-update-checkbox = Автоматически проверять обновления

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Скачано
models-section-available = Доступно для скачивания
models-download-button = Скачать
models-delete-button = Удалить
models-show-location-button = Показать расположение файла
models-cancel-button = Отмена
models-custom-url-label = Свой URL модели ggml:
models-delete-confirm-title = Удалить модель
models-delete-confirm-body = Вы уверены, что хотите удалить выбранную модель?
models-download-error-title = Ошибка
models-download-failed = Ошибка скачивания
models-speed-badge = { $ratio }× реального времени
models-speed-badge-tip = Измеренная скорость в реальном времени по сеансам живой записи на этом устройстве

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Открыть окно записи
shortcut-desc-import-file = Импортировать файл
shortcut-desc-import-url = Импортировать ссылку
shortcut-desc-open-preferences-window = Открыть окно настроек
shortcut-desc-view-transcript-text = Просмотреть текст транскрипции
shortcut-desc-view-transcript-translation = Просмотреть перевод транскрипции
shortcut-desc-view-transcript-timestamps = Просмотреть таймкоды транскрипции
shortcut-desc-search-transcript = Поиск в транскрипции
shortcut-desc-search-next = Перейти к следующему результату поиска в транскрипции
shortcut-desc-search-previous = Перейти к предыдущему результату поиска в транскрипции
shortcut-desc-scroll-to-current-text = Прокрутить к текущему тексту
shortcut-desc-play-pause-audio = Воспроизвести/пауза аудио
shortcut-desc-replay-current-segment = Повторить текущий фрагмент
shortcut-desc-toggle-playback-controls = Переключить элементы управления воспроизведением
shortcut-desc-decrease-segment-start = Уменьшить время начала фрагмента
shortcut-desc-increase-segment-start = Увеличить время начала фрагмента
shortcut-desc-decrease-segment-end = Уменьшить время окончания фрагмента
shortcut-desc-increase-segment-end = Увеличить время окончания фрагмента
shortcut-desc-clear-history = Очистить историю
shortcut-desc-stop-transcription = Отменить транскрипцию
prefs-shortcuts-press = Нажмите комбинацию клавиш…
prefs-shortcuts-clear-tooltip = Сбросить на значение по умолчанию
prefs-shortcuts-conflict = Конфликтует с { $other }
prefs-shortcuts-reset = Сбросить к значениям по умолчанию

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Включить отслеживание папки
prefs-fw-input-folder-label = Входная папка
prefs-fw-input-folder-placeholder = /путь/к/входной/папке
prefs-fw-output-folder-label = Папка вывода
prefs-fw-output-folder-placeholder = /путь/к/выходной/папке
prefs-fw-browse = Обзор
prefs-fw-delete-checkbox = Удалить обработанные файлы
prefs-fw-disposition-note = После транскрипции исходный файл перемещается в выходную папку — или удаляется, если включена опция «Удалять обработанные файлы».

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Эти хуки запускаются автоматически после завершения каждой транскрипции. Ошибка хука записывается в журнал, но никогда не приводит к сбою самой транскрипции.
prefs-postproc-skip-title = Пропустить уже транскрибированные
prefs-postproc-skip-enable = Включить пропуск уже транскрибированных
prefs-postproc-skip-check-files = Проверить наличие файлов результатов
prefs-postproc-skip-check-files-desc = Пропустить, если рядом с аудиофайлом найден соответствующий файл .txt, .srt или .vtt
prefs-postproc-skip-check-db = Проверить в базе данных транскрипций
prefs-postproc-skip-check-db-desc = Пропустить, если этот файл уже транскрибирован и запись существует в базе данных
prefs-postproc-dfn-title = Шумоподавление DeepFilterNet
prefs-postproc-dfn-enable = Включить шумоподавление DeepFilterNet
prefs-postproc-dfn-keep-file = Сохранить файл без шума после транскрипции
prefs-postproc-langdetect-title = Улучшенное определение языка
prefs-postproc-langdetect-enable = Включить расширенное определение языка
prefs-postproc-langdetect-download-tiny = Загрузить модель tiny, если ни одна не доступна
prefs-postproc-langdetect-download-tiny-desc = Если ни одна модель whisper.cpp не загружена, загружает модель tiny для определения языка.
prefs-postproc-resizer-title = Автоматическое изменение размера транскрипции
prefs-postproc-resizer-enable = Включить автоматический изменитель размера транскрипции
prefs-postproc-resizer-merge-by-gap = Объединять по паузе
prefs-postproc-resizer-merge-gap-seconds = Пауза объединения (секунды)
prefs-postproc-resizer-split-by-punctuation = Разделять по пунктуации
prefs-postproc-resizer-punctuation = Пунктуация
prefs-postproc-resizer-split-by-max-length = Разделять по максимальной длине
prefs-postproc-resizer-max-length = Максимальная длина субтитра
prefs-postproc-summary-title = ИИ-резюме
prefs-postproc-summary-enable = Включить ИИ-резюме
prefs-postproc-summary-api-url = Базовый URL API
prefs-postproc-summary-api-key-label = Ключ API
prefs-postproc-summary-model = Модель
prefs-postproc-summary-prompt = Запрос для резюме
prefs-postproc-summary-save-to-notes = Сохранить резюме в заметки
prefs-postproc-summary-save-to-file = Сохранить резюме в файл
prefs-postproc-summary-output-folder = Папка вывода (для файла)
prefs-postproc-summary-key-save-error-body = Не удалось сохранить API-ключ ИИ-резюме в связке ключей
prefs-postproc-docx-title = Экспорт в DOCX
prefs-postproc-docx-enable = Включить экспорт в DOCX
prefs-postproc-docx-output-folder = Папка вывода
prefs-postproc-docx-include-timestamps = Включить временные метки
prefs-postproc-output-folder-hint = Оставьте пустым, чтобы сохранить рядом с исходным файлом.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Вид
viewer-mode-text = Текст
viewer-mode-translation = Перевод
viewer-mode-timestamps = Таймкоды
viewer-export = Экспорт
viewer-translate = Перевести
viewer-resize = Изменить размер
viewer-identify-speakers = Определить говорящих
viewer-find = Найти
viewer-find-label = Найти:
viewer-find-placeholder = Введите текст для поиска...
viewer-find-clear = Очистить
viewer-find-prev-tooltip = Предыдущее совпадение ({ $shortcut })
viewer-find-next-tooltip = Следующее совпадение ({ $shortcut })
viewer-empty = Нет сегментов — возможно, во входных данных только тишина
viewer-open-error = Не удалось открыть транскрипцию
viewer-col-start = Начало
viewer-col-end = Конец
viewer-col-text = Текст
viewer-col-translation = Перевод
viewer-saved = изменено · сохранено
viewer-unsaved-warning = Изменение не удалось сохранить — оно хранится в памяти, но не записано на диск
viewer-player-loading = Загрузка аудио…
viewer-player-failed = Воспроизведение недоступно
viewer-playback-controls = Управление воспроизведением:
viewer-loop-segment = Повторять фрагмент
viewer-loop-segment-tooltip = Включить/отключить повторение при нажатии на фрагменты транскрипции
viewer-follow-audio = Следовать за аудио
viewer-follow-audio-tooltip = Включить/отключить отслеживание текущей позиции аудио в транскрипции. Когда включено, автоматически прокручивает к текущему тексту.
viewer-speed = Скорость:
viewer-scroll-to-current = Прокрутить к текущему
viewer-scroll-to-current-tooltip = Прокрутить к тексту, который сейчас произносится

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Требуется API-ключ
viewer-translate-key-required-body = Пожалуйста, введите API-ключ OpenAI в настройках
viewer-translate-settings-title = Настройки ИИ-перевода

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Текст
viewer-export-translation = Перевод
viewer-export-dialog-title = Сохранить файл
viewer-export-filter = Текстовые файлы
viewer-export-failed = Не удалось записать экспорт — подробности см. в журнале
export-written = Записано в { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Настройки
resizer-create-new = Создать новую транскрипцию
resizer-extend-heading = Продлить время окончания
resizer-extend-label = Продлить окончания до (секунд)
resizer-extend-button = Продлить окончания
resizer-resize-heading = Параметры изменения размера
resizer-resize-label = Желаемая длина субтитров
resizer-resize-button = Изменить размер
resizer-resize-disabled-tooltip = Доступно только если тайминги на уровне слов были отключены во время транскрипции
resizer-merge-heading = Параметры объединения
resizer-merge-by-gap = Объединять по паузе
resizer-split-by-punctuation = Разделять по пунктуации
resizer-split-by-max-length = Разделять по максимальной длине
resizer-merge-button = Объединить
resizer-merge-disabled-tooltip = Доступно только если тайминги на уровне слов были включены во время транскрипции
resizer-merging = Объединение…
resizer-merge-error = Не удалось выполнить объединение:
resizer-save-error = Не удалось сохранить изменения:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Шаг 1: Определить говорящих
speaker-id-step2-heading = Шаг 2: Назвать говорящих
speaker-id-ready-label = Готово к определению говорящих
speaker-id-missing-audio-label = Аудиофайл не найден
speaker-id-identify-button = Определить
speaker-id-cancel-button = Отмена
speaker-id-cancelling = Отмена...
speaker-id-decoding = Декодирование аудио…
speaker-id-downloading-models = Скачивание моделей…
speaker-id-identifying = Определение говорящих…
speaker-id-play-sample = Воспроизвести образец
speaker-id-merge-checkbox = Объединить предложения говорящих
speaker-id-save-button = Сохранить
speaker-id-save-error = Не удалось сохранить изменения:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Живая запись
recording-model-load-note = Первое использование модели может занять несколько минут для загрузки.
recording-advanced-button = Дополнительно...
recording-microphone-label = Микрофон:
recording-record-button = Записать
recording-stop-button = Остановить
recording-realtime-warning = { $size } транскрибировано со скоростью { $ratio }× реального времени при последнем запуске — живая транскрипция может начать отставать
recording-chunk-skipped-warning = Фрагмент завершился с ошибкой и был пропущен: { $detail }
recording-panel-placeholder = Нажмите «Записать», чтобы начать...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Ожидание перевода от ИИ...
recording-model-downloading = Скачивание модели… { $percent }%
recording-model-downloading-indeterminate = Скачивание модели…
recording-model-loading = Загрузка модели…
recording-error-prefix = При запуске новой записи произошла ошибка:
recording-error-suffix = Пожалуйста, проверьте ваши аудиоустройства или посмотрите журналы приложения для получения дополнительной информации.
recording-thread-crashed = Сеанс записи неожиданно завершился. Проверьте журналы приложения для получения дополнительной информации.
meter-average-volume = Средняя громкость
meter-queue = Очередь

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Дополнительные настройки
recording-advanced-speech-heading = Параметры распознавания речи
recording-advanced-initial-prompt-label = Начальная подсказка:
recording-advanced-translation-heading = Настройки перевода
recording-advanced-llm-enable = Включить перевод с помощью ИИ
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Настройки записи
recording-advanced-silence-threshold-label = Порог тишины:
recording-advanced-mode-label = Режим живой записи:
recording-advanced-mode-append-below = Добавить внизу
recording-advanced-mode-append-above = Добавить вверху
recording-advanced-mode-append-and-correct = Добавить и исправить
recording-advanced-separator-label = Разделитель строк:
recording-advanced-step-label = Шаг транскрипции:
recording-advanced-hide-unconfirmed = Скрыть неподтверждённые
recording-advanced-export-enable = Включить экспорт живой записи
recording-advanced-export-folder-label = Папка для экспорта:
recording-advanced-export-browse = Обзор
recording-advanced-export-file-name-label = Имя файла экспорта:
recording-advanced-export-file-type-label = Тип файла экспорта:
recording-advanced-export-file-type-txt = Текстовый файл (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Ограничить количество записей экспорта
    (0 = экспортировать все):
recording-advanced-upload-url-label = URL для загрузки:
recording-advanced-ok = ОК

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Показать
presentation-show-tooltip = Показать в новом окне
presentation-text-size-label = Размер текста:
presentation-theme-label = Тема
presentation-theme-light = Светлая
presentation-theme-dark = Тёмная
presentation-theme-custom = Пользовательская
presentation-text-color-label = Цвет текста
presentation-background-color-label = Цвет фона
presentation-fullscreen-button = Полноэкранный режим
presentation-window-title = Презентация транскрипции в реальном времени

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Копировать
recording-copy-tooltip = Копировать транскрипцию в буфер обмена
recording-copy-copied = Скопировано!
recording-copy-nothing = Нечего копировать!

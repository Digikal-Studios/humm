## humm-gui — Traditional Chinese (zh-TW) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 15, design spec
## §7.2, batch 3 of 3, final locale). Section comments stay in English on
## purpose: they are developer-facing notes about *where* a key renders,
## identical in every catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own zh_TW translation is used
## verbatim (`buzz/locale/zh_TW/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh in the
## same concise UI register. Buzz's zh_TW translators render
## "transcription"/"transcribe" as 轉錄 throughout (distinct from the zh-CN
## catalog's own 识别/"recognition" choice for the same concept — each
## follows its own Buzz locale's own established term, not a shared one);
## this catalog's fresh strings use 轉錄 consistently for the same reason.
## Traditional-script vocabulary is used throughout where it diverges from
## zh-CN's Simplified choices, anchored to zh_TW's own po (`資料夾` not
## `文件夹` for folder, `檔案` not `文件` for file, `記憶體` not `内存` for
## memory, `顯示`/`啟用`/`匯出`/`取消`/`尋找` for
## show/enable/export/cancel/find). Buzz's own zh_TW po is internally
## inconsistent on "import": `menu-file-import-file`/`menu-file-import-url`
## both use 匯入, but `menu-file-import-folder` uses 導入 — an upstream Buzz
## inconsistency reused byte-verbatim, not introduced or corrected here
## (parity is with Buzz's actual translated strings, not with an idealized
## consistent one). `models-speed-badge`/`recording-realtime-warning`
## (humm-only, no Buzz anchor) use 即時 for "realtime", the more common
## Taiwan-Mandarin choice for real-time software contexts (as opposed to
## the zh-CN catalog's own 实时 for the same concept).
##
## **Font note (design spec §7.1/ledger #8, closing-sweep task 1).** This
## catalog's CJK glyphs render through the single bundled Noto Sans SC font
## (`HummApp::new`'s fallback chain) — the documented divergence: this
## catalog's Traditional-Han text renders with Simplified-preferred glyph
## variants for the handful of codepoints where SC/TC glyphs differ, since
## Buzz-on-Qt used the OS's own region-correct system font per locale and
## humm bundles one pan-CJK font for all of zh_CN/zh_TW/ja_JP. The text
## itself is correct Traditional Chinese (distinct characters, not merely
## divergent glyph shapes, e.g. 轉錄 vs zh-CN's 识别); only the *glyph
## rendering* of shared codepoints is SC-shaped.
##
## Buzz's own zh_TW po translates the msgid "Ok" (its own relabelled dialog
## button text) as "確定". Following the es/zh-CN/M8-Task-13/14 precedent
## (Buzz's own lineage, not a translation choice made here), all four of
## clear-confirm-ok, dialog-ok, url-dialog-ok and recording-advanced-ok
## collapse to that same "確定".
##
## All six of the previously known doubled-msgstr-variant Buzz msgids
## (M8 Task 13) are clean (single variant, no concatenation) in zh_TW's own
## po and are used as-is: `prefs-general-force-cpu-tooltip`,
## `resizer-resize-disabled-tooltip`, `resizer-merge-disabled-tooltip`,
## `recording-model-load-note`, `viewer-loop-segment-tooltip`,
## `prefs-general-key-save-error-body`. No new doubled-variant or
## msgid/msgstr-misattribution defect (the kind M8 Task 14 found in ja_JP)
## was found in zh_TW's own po either, by the same length-ratio outlier
## scan and stray-`{`/`}` scan used across this whole batch.
##
## Unlike every other locale in this workspace, zh_TW's po carries **no**
## msgid "Close" translation at all — neither live nor obsolete (the same
## situation M8 Task 13 documented for da_DK/pl_PL, just with zero
## fallback rather than an obsolete one). `about-close-button`/
## `toast-detail-close`/`update-dialog-close` are translated fresh here
## ("關閉").
##
## Two further Buzz msgids match humm strings exactly but carry a truly
## *empty* msgstr in zh_TW's po (untranslated upstream, gettext falls back
## silently): `prefs-postproc-skip-check-files-desc` and `prefs-postproc-
## skip-check-db-desc`. Both are translated fresh here rather than left
## blank — the same two keys every prior M8 locale batch has found empty.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in zh_TW (API, GPU, CPU,
## Whisper, TXT/SRT/VTT) are left alone here too; "URL" is translated as
## 網址 wherever Buzz's own zh_TW po does so (its majority choice), matching
## that precedent for the humm-only `models-custom-url-label` too.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = 英語
locale-name-ca-es = 加泰隆尼亞語
locale-name-da-dk = 丹麥語
locale-name-nl = 荷蘭語
locale-name-de-de = 德語
locale-name-es-es = 西班牙語
locale-name-it-it = 義大利語
locale-name-ja-jp = 日語
locale-name-lv-lv = 拉脫維亞語
locale-name-pl-pl = 波蘭語
locale-name-pt-br = 葡萄牙語（巴西）
locale-name-ru = 俄語
locale-name-uk-ua = 烏克蘭語
locale-name-zh-cn = 中文（簡體）
locale-name-zh-tw = 中文（繁體）

## Empty-state hero (views/main_window.rs)

hero-drop = 將媒體檔案拖放到此處，或按 ⌘O
hero-import-button = 匯入檔案…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = 檔案名稱 / 網址
table-col-model = 模型
table-col-task = 任務
table-col-status = 狀態
table-col-date-completed = 完成日期
table-col-date-added = 新增日期
table-col-notes = 備註

## Task table — header/row context menus

table-menu-reset-column-order = 重設欄位順序
table-menu-restart = 重新開始轉錄
table-menu-rename = 重新命名
table-menu-notes = 新增／編輯備註

## Task table — Task column labels (viewmodel.rs)

task-transcribe = 轉錄
task-translate = 翻譯為英語

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = 正在分離人聲

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = 錄製
toolbar-new-file = 新增檔案轉錄
toolbar-new-url = 新增網址轉錄
toolbar-open-transcript = 打開轉換結果
toolbar-open-transcript-tooltip = 選取一或多個已完成的轉錄以開啟
toolbar-cancel = 取消錄製
toolbar-clear = 清除歷史紀錄

## Menu bar (views/main_window.rs, F-004)

menu-file = 檔案
menu-file-import-file = 匯入檔案…
menu-file-import-url = 匯入網址…
menu-file-import-folder = 導入資料夾…
menu-help-title = 幫助
menu-help-about = 關於 humm
menu-help-docs = 幫助
menu-help-docs-tooltip = 文件將隨第一個正式版本推出
menu-help-preferences = 偏好設定…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = 清除歷史紀錄
clear-confirm-body = 您確定要刪除所選錄製嗎？此操作無法撤消。
clear-confirm-ok = 確定
clear-confirm-cancel = 取消
clear-error = 無法刪除所選轉錄

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = 重新命名轉錄
rename-label = 輸入新名稱：
rename-error = 無法重新命名
notes-title = 備註
notes-label = 為此轉錄輸入相關備註：
notes-error = 無法儲存備註
dialog-ok = 確定
dialog-cancel = 取消

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = 轉錄完成
toast-completed-suffix = 個檔案已寫入
toast-canceled = 轉錄已取消
toast-detail-title = 轉錄失敗
toast-detail-open-log = 開啟日誌資料夾
toast-detail-retry = 重試
toast-detail-close = 關閉
retry-error-unavailable = 無法重新開始：已儲存的選項無法使用

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm 無法記錄歷史更新——匯出仍會正常完成；請查看日誌檔案

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = 拖放以匯入

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = 缺少 yt-dlp——請安裝它（例如 `brew install yt-dlp`）或設定 HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = 匯入網址
url-dialog-label = 網址：
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = 您輸入的網址無效。
url-dialog-ok = 確定
url-dialog-cancel = 取消

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = 模型:
options-task-label = 任務:
options-language-label = 語言:
language-detect = 檢測語言
options-word-timestamps = 單字級別的時間表達
options-extract-speech = 提取語音
options-extract-speech-missing-tooltip = 找不到 demucs——請安裝它（例如 `pipx install demucs`）以啟用語音提取
options-export-label = 匯出：
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = 未選擇匯出格式
options-advanced = 進階
options-initial-prompt-label = 初始提示：
options-hf-repo-label = Hugging Face 模型
options-run = 開始執行

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = 後端:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = API 金鑰：
options-llm-heading = 翻譯設定
options-llm-enable = 啟用 AI 翻譯
options-llm-model-label = AI 模型：
options-llm-model-presets-button = 預設
options-llm-prompt-label = AI 指令：

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = 正在監視 { $name }
folderwatch-pill-paused = 已暫停
folderwatch-pill-pause-tip = 暫停資料夾監視
folderwatch-pill-resume-tip = 繼續資料夾監視

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = 有可用更新
update-dialog-title = 有可用更新
update-dialog-header = humm 有新版本可用！
update-dialog-current-version = 目前版本：
update-dialog-new-version = 新版本：
update-dialog-release-notes = 版本說明：
update-dialog-open-release-page = 開啟發布頁面
update-dialog-close = 關閉

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = 版本
about-check-button = 檢查更新
about-show-logs-button = 顯示日誌
about-close-button = 關閉
about-up-to-date = 你是最新的！
about-check-failed = 檢查更新失敗——請查看日誌檔案

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = 偏好設定
prefs-tab-general = 一般
prefs-tab-models = 模型
prefs-tab-shortcuts = 快捷鍵
prefs-tab-folder-watch = 資料夾監視
prefs-tab-postproc = 後處理
prefs-tab-privacy = 隱私

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = 介面語言
prefs-general-font-size-label = 字體大小
prefs-general-font-size-custom-label = 自訂字型大小
prefs-general-openai-key-label = OpenAI API 金鑰
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = 顯示
prefs-general-key-hide = 隱藏
prefs-general-test-button = 測試
prefs-general-test-title = OpenAI API 金鑰測試
prefs-general-test-success-body = 您的 API 金鑰有效。humm 將使用此金鑰執行 Whisper API 轉錄和 AI 翻譯。
prefs-general-invalid-key-title = 無效的 API 金鑰
prefs-general-invalid-key-body = API 僅支援 base64 字元（A-Za-z0-9+/=_-）。API 金鑰中的其他字元可能會導致錯誤。
prefs-general-key-save-error-title = 錯誤
prefs-general-key-save-error-body = 無法將 OpenAI API 金鑰儲存至鑰匙圈
prefs-general-base-url-label = OpenAI 基礎網址
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI API 模型
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = 預設匯出檔案名稱
prefs-general-force-cpu-row-label = 停用 GPU
prefs-general-force-cpu-label = 僅使用 CPU 並停用 GPU 加速
prefs-general-force-cpu-tooltip = 若較大的模型無法放入 GPU 記憶體且 humm 當機，請啟用此選項

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = 傳送匿名啟動遙測
privacy-telemetry-disabled-caption = 此版本已在編譯時停用遙測。
privacy-telemetry-items-intro = 每次啟動事件包含：
privacy-telemetry-item-app-version = 應用程式版本
privacy-telemetry-item-system-name = 系統名稱
privacy-telemetry-item-os-version = 作業系統版本
privacy-telemetry-item-kernel-release = 核心版本
privacy-telemetry-item-cpu-architecture = CPU 架構
privacy-telemetry-item-system-locale = 系統語系
privacy-telemetry-item-identifier = 一組隨機匿名識別碼
privacy-update-checkbox = 自動檢查更新

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = 已下載
models-section-available = 可供下載
models-download-button = 下載
models-delete-button = 刪除
models-show-location-button = 顯示檔案位置
models-cancel-button = 取消
models-custom-url-label = 自訂 ggml 模型網址：
models-delete-confirm-title = 刪除模型
models-delete-confirm-body = 您確定要刪除所選模型嗎？
models-download-error-title = 錯誤
models-download-failed = 下載失敗
models-speed-badge = { $ratio }× 即時
models-speed-badge-tip = 根據本機即時錄音工作階段測得的即時速度

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = 開啟錄製視窗
shortcut-desc-import-file = 匯入檔案
shortcut-desc-import-url = 匯入網址
shortcut-desc-open-preferences-window = 開啟偏好設定視窗
shortcut-desc-view-transcript-text = 檢視轉錄文字
shortcut-desc-view-transcript-translation = 檢視轉錄翻譯
shortcut-desc-view-transcript-timestamps = 檢視轉錄時間戳記
shortcut-desc-search-transcript = 搜尋轉錄稿
shortcut-desc-search-next = 移至下一個轉錄搜尋結果
shortcut-desc-search-previous = 移至上一個轉錄搜尋結果
shortcut-desc-scroll-to-current-text = 捲動至目前文字
shortcut-desc-play-pause-audio = 播放／暫停音訊
shortcut-desc-replay-current-segment = 重播目前片段
shortcut-desc-toggle-playback-controls = 切換播放控制項
shortcut-desc-decrease-segment-start = 縮短片段開始時間
shortcut-desc-increase-segment-start = 延長片段開始時間
shortcut-desc-decrease-segment-end = 縮短片段結束時間
shortcut-desc-increase-segment-end = 延長片段結束時間
shortcut-desc-clear-history = 清除歷史紀錄
shortcut-desc-stop-transcription = 取消錄製
prefs-shortcuts-press = 按下快捷鍵…
prefs-shortcuts-clear-tooltip = 重設為預設值
prefs-shortcuts-conflict = 與 { $other } 衝突
prefs-shortcuts-reset = 重設為預設值

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = 啟用資料夾監視
prefs-fw-input-folder-label = 輸入資料夾
prefs-fw-input-folder-placeholder = /輸入資料夾/路徑
prefs-fw-output-folder-label = 輸出資料夾
prefs-fw-output-folder-placeholder = /輸出資料夾/路徑
prefs-fw-browse = 瀏覽
prefs-fw-delete-checkbox = 刪除已處理的檔案
prefs-fw-disposition-note = 轉錄完成後，來源檔案會移動到輸出資料夾——若勾選「刪除已處理的檔案」，則會改為刪除。

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = 這些掛勾會在每次轉錄完成後自動執行。掛勾失敗會被記錄，但絕不會導致轉錄本身失敗。
prefs-postproc-skip-title = 跳過已轉錄檔案
prefs-postproc-skip-enable = 啟用跳過已轉錄檔案
prefs-postproc-skip-check-files = 檢查現有結果檔案
prefs-postproc-skip-check-files-desc = 若在音訊檔案旁找到符合檔名的 .txt、.srt 或 .vtt 檔案，則跳過
prefs-postproc-skip-check-db = 在轉錄資料庫中檢查
prefs-postproc-skip-check-db-desc = 若此檔案已轉錄且資料庫中已有記錄，則跳過
prefs-postproc-dfn-title = DeepFilterNet 降噪
prefs-postproc-dfn-enable = 啟用 DeepFilterNet 降噪
prefs-postproc-dfn-keep-file = 轉錄後保留降噪檔案
prefs-postproc-langdetect-title = 增強語言偵測
prefs-postproc-langdetect-enable = 啟用增強語言偵測
prefs-postproc-langdetect-download-tiny = 若沒有可用模型則下載 tiny 模型
prefs-postproc-langdetect-download-tiny-desc = 若未下載任何 whisper.cpp 模型，則下載 tiny 模型用於語言偵測。
prefs-postproc-resizer-title = 自動轉錄文字調整
prefs-postproc-resizer-enable = 啟用自動轉錄調整工具
prefs-postproc-resizer-merge-by-gap = 依間隔合併
prefs-postproc-resizer-merge-gap-seconds = 合併間隔（秒）
prefs-postproc-resizer-split-by-punctuation = 依標點分割
prefs-postproc-resizer-punctuation = 標點符號
prefs-postproc-resizer-split-by-max-length = 依最大長度分割
prefs-postproc-resizer-max-length = 字幕最大長度
prefs-postproc-summary-title = AI 摘要
prefs-postproc-summary-enable = 啟用 AI 摘要
prefs-postproc-summary-api-url = API 基底 URL
prefs-postproc-summary-api-key-label = API 金鑰
prefs-postproc-summary-model = 模型
prefs-postproc-summary-prompt = 摘要提示詞
prefs-postproc-summary-save-to-notes = 將摘要儲存到備註
prefs-postproc-summary-save-to-file = 將摘要儲存到檔案
prefs-postproc-summary-output-folder = 輸出資料夾（用於檔案）
prefs-postproc-summary-key-save-error-body = 無法將 AI 摘要 API 金鑰儲存到鑰匙圈
prefs-postproc-docx-title = 匯出為 DOCX
prefs-postproc-docx-enable = 啟用匯出為 DOCX
prefs-postproc-docx-output-folder = 輸出資料夾
prefs-postproc-docx-include-timestamps = 包含時間戳
prefs-postproc-output-folder-hint = 留空則儲存在來源檔案旁邊。

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = 檢視
viewer-mode-text = 文字
viewer-mode-translation = 翻譯
viewer-mode-timestamps = 時間戳記
viewer-export = 匯出
viewer-translate = 翻譯
viewer-resize = 調整大小
viewer-identify-speakers = 識別說話者
viewer-find = 尋找
viewer-find-label = 尋找：
viewer-find-placeholder = 輸入要尋找的文字...
viewer-find-clear = 清除
viewer-find-prev-tooltip = 上一個相符項目（{ $shortcut }）
viewer-find-next-tooltip = 下一個相符項目（{ $shortcut }）
viewer-empty = 沒有片段——輸入內容可能僅為靜音
viewer-open-error = 無法開啟轉錄
viewer-col-start = 開始
viewer-col-end = 結束
viewer-col-text = 文字
viewer-col-translation = 翻譯
viewer-saved = 已編輯．已儲存
viewer-unsaved-warning = 有一項編輯無法儲存——已保留在記憶體中，但未寫入磁碟
viewer-player-loading = 正在載入音訊…
viewer-player-failed = 無法播放
viewer-playback-controls = 播放控制：
viewer-loop-segment = 循環片段
viewer-loop-segment-tooltip = 點擊轉錄片段時啟用／停用循環播放
viewer-follow-audio = 跟隨音訊
viewer-follow-audio-tooltip = 啟用／停用在轉錄稿中跟隨目前音訊位置。啟用後，自動捲動至目前文字。
viewer-speed = 速度：
viewer-scroll-to-current = 捲動至目前位置
viewer-scroll-to-current-tooltip = 捲動至目前正在說話的文字

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = 需要 API 金鑰
viewer-translate-key-required-body = 請在偏好設定中輸入 OpenAI API 金鑰
viewer-translate-settings-title = AI 翻譯設定

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = 文字
viewer-export-translation = 翻譯
viewer-export-dialog-title = 儲存檔案
viewer-export-filter = 文字檔案
viewer-export-failed = 無法寫入匯出檔案——詳情請查看日誌
export-written = 已寫入 { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = 設定
resizer-create-new = 建立新轉錄
resizer-extend-heading = 延長結束時間
resizer-extend-label = 最多延長結束時間（秒）
resizer-extend-button = 延長結尾
resizer-resize-heading = 調整大小選項
resizer-resize-label = 理想字幕長度
resizer-resize-button = 調整大小
resizer-resize-disabled-tooltip = 僅在轉錄時停用單字級別時間戳記時可用
resizer-merge-heading = 合併選項
resizer-merge-by-gap = 依間隔合併
resizer-split-by-punctuation = 依標點分割
resizer-split-by-max-length = 依最大長度分割
resizer-merge-button = 合併
resizer-merge-disabled-tooltip = 僅在轉錄時啟用單字級別時間戳記時可用
resizer-merging = 正在合併…
resizer-merge-error = 無法執行合併：
resizer-save-error = 無法儲存變更：

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = 步驟 1：識別說話者
speaker-id-step2-heading = 步驟 2：為說話者命名
speaker-id-ready-label = 準備識別說話者
speaker-id-missing-audio-label = 找不到音訊檔案
speaker-id-identify-button = 識別
speaker-id-cancel-button = 取消
speaker-id-cancelling = 取消中...
speaker-id-decoding = 正在解碼音訊…
speaker-id-downloading-models = 正在下載模型…
speaker-id-identifying = 正在識別說話者…
speaker-id-play-sample = 播放範例
speaker-id-merge-checkbox = 合併說話者句子
speaker-id-save-button = 儲存
speaker-id-save-error = 無法儲存變更：

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = 現場錄製
recording-model-load-note = 首次使用模型最多可能需要數分鐘載入。
recording-advanced-button = 進階...
recording-microphone-label = 麥克風:
recording-record-button = 錄製
recording-stop-button = 停止
recording-realtime-warning = 上次執行時 { $size } 以 { $ratio }× 即時速度完成轉錄——即時轉錄可能會跟不上
recording-chunk-skipped-warning = 一個區塊處理失敗並已略過：{ $detail }
recording-panel-placeholder = 點擊開始錄製
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = 等待 AI 翻譯...
recording-model-downloading = 正在下載模型…{ $percent }%
recording-model-downloading-indeterminate = 正在下載模型…
recording-model-loading = 正在載入模型…
recording-error-prefix = 開始新錄製出錯
recording-error-suffix = 請檢查您的音頻設備或檢查應用程序日誌以獲取更多信息。
recording-thread-crashed = 錄製工作階段意外結束。請查看應用程式日誌以取得更多資訊。
meter-average-volume = 平均音量
meter-queue = 佇列

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = 進階設定
recording-advanced-speech-heading = 語音識別設定
recording-advanced-initial-prompt-label = 初始提示：
recording-advanced-translation-heading = 翻譯設定
recording-advanced-llm-enable = 啟用 AI 翻譯
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = 錄製設定
recording-advanced-silence-threshold-label = 靜音閾值：
recording-advanced-mode-label = 即時錄製模式：
recording-advanced-mode-append-below = 附加至下方
recording-advanced-mode-append-above = 附加至上方
recording-advanced-mode-append-and-correct = 附加並校正
recording-advanced-separator-label = 行分隔符號：
recording-advanced-step-label = 轉錄步長:
recording-advanced-hide-unconfirmed = 隱藏未確認內容
recording-advanced-export-enable = 啟用即時錄製匯出
recording-advanced-export-folder-label = 匯出資料夾：
recording-advanced-export-browse = 瀏覽
recording-advanced-export-file-name-label = 匯出檔案名稱：
recording-advanced-export-file-type-label = 匯出檔案類型：
recording-advanced-export-file-type-txt = 文字檔案（.txt）
recording-advanced-export-file-type-csv = CSV（.csv）
recording-advanced-export-max-entries-label =
    限制匯出筆數
    （0 = 全部匯出）：
recording-advanced-upload-url-label = 上傳網址：
recording-advanced-ok = 確定

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = 顯示
presentation-show-tooltip = 在新視窗中顯示
presentation-text-size-label = 文字大小：
presentation-theme-label = 主題
presentation-theme-light = 淺色
presentation-theme-dark = 深色
presentation-theme-custom = 自訂
presentation-text-color-label = 文字顏色
presentation-background-color-label = 背景顏色
presentation-fullscreen-button = 全螢幕
presentation-window-title = 即時轉錄簡報

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = 複製
recording-copy-tooltip = 複製轉錄內容到剪貼簿
recording-copy-copied = 已複製！
recording-copy-nothing = 沒有可複製的內容！

## humm-gui — Japanese (ja) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 14, design spec
## §7.2). Section comments stay in English on purpose: they are
## developer-facing notes about *where* a key renders, identical in every
## catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own ja_JP translation is used
## verbatim (`buzz/locale/ja_JP/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh, matching
## the register the Buzz-sourced strings already establish. Buzz's ja_JP po
## consistently uses the polite (です/ます, ...してください) register
## throughout its own translations (e.g. "本当に...ですか？", "...を入力して
## ください:") — this catalog stays polite consistently, including for the
## fresh humm-only strings. "文字起こし" is Buzz's own dominant term for
## "transcription"/"transcribe" throughout its ja_JP po (toolbar, menu,
## status strings); a handful of Buzz strings elsewhere in the same po use
## the loanword "トランスクリプション" instead (e.g. "Transcription step:") —
## an inconsistency in Buzz's own upstream translation, not something this
## catalog introduces. The fresh humm-only strings here follow the dominant
## "文字起こし" convention throughout, matching the rest of the ported UI.
##
## Buzz's own ja_JP po translates the msgid "Ok" (its own relabelled dialog
## button text) as "Ok". Following the es/zh-CN/M8-Task-13 precedent (Buzz's
## own lineage, not a translation choice made here), all four of
## clear-confirm-ok, dialog-ok, url-dialog-ok and recording-advanced-ok
## collapse to that same "Ok" — coincidentally matching en's own spelling for
## three of the four (dialog-ok is "OK" in en), which is incidental to this
## locale, not intent; the en catalog's Ok/OK casing split is English-source-
## specific Qt lineage and does not survive translation.
##
## Six Buzz msgids are known (from the M8 Task 13 batch) to sometimes carry
## two translation variants concatenated with no separator, an upstream Buzz
## artifact rather than a translation choice. Only one of the six is
## affected in ja_JP's own po and is fixed here to the second (revised)
## variant alone: `prefs-general-force-cpu-tooltip` (msgstr literally
## doubles "大きなモデルがGPUメモリに収まらずBuzzがクラッシュする場合に設定"
## then "...設定してください" with no separator; the second, more complete
## half is kept). The other five are clean (single variant, no
## concatenation) in ja_JP's po and are used as-is: `recording-model-load-
## note`, `viewer-loop-segment-tooltip`, `resizer-resize-disabled-tooltip`,
## `resizer-merge-disabled-tooltip`, `prefs-general-key-save-error-body`.
##
## Two further Buzz msgids match humm strings exactly but carry a truly
## *empty* msgstr in ja_JP's po (untranslated upstream, gettext falls back
## silently): `prefs-postproc-skip-check-files-desc` and `prefs-postproc-
## skip-check-db-desc`. Both are translated fresh here rather than left
## blank — the same two keys M8 Task 13 found empty in all four of its own
## locales.
##
## A handful of Buzz ja_JP msgids use the ASCII "..." ellipsis rather than
## the Unicode "…" character the en catalog's own style uses for the same
## strings (e.g. "Import File..."); reused verbatim in content but with the
## ellipsis glyph normalized to "…" to match this catalog's (and Buzz's own
## `ui_language_combo_box`-adjacent style choices for) the rest of its
## strings — a typographic normalization, not a translation change.
##
## A seventh, differently-shaped defect (not one of the six doubled-variant
## msgids above): ja_JP's own po attaches the *same* msgstr
## ("プラグインをインストールできませんでした:\n{}", i.e. "Could not install
## the plugin:\n{}") to two different msgids — its own correct one, and
## (a genuine upstream authoring bug, not a translation choice) also to
## "Please check your audio devices or check the application logs for more
## information." (`recording_transcriber_widget.py`), which is
## `recording-error-suffix`'s own en source text. Byte-verbatim reuse would
## also have failed outright, not just been semantically wrong: the raw
## `{}` in that mismatched msgstr is Python `str.format` syntax, not valid
## Fluent (Fluent placeables are `{ $name }`), so it does not parse.
## `recording-error-suffix` is translated fresh here instead, matching the
## meaning pl_PL/pt_BR/ru's own (correct) po translations of the same msgid
## already carry.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in ja (API, GPU, CPU,
## Whisper, URL, TXT/SRT/VTT) are left alone here too; "Backend" is
## transliterated as "バックエンド" (a common loanword rendering in Japanese
## software), matching the zh-CN catalog's own choice to transliterate
## rather than keep the Latin word, for the same reason (a non-Latin
## script).

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = 英語
locale-name-ca-es = カタルーニャ語
locale-name-da-dk = デンマーク語
locale-name-nl = オランダ語
locale-name-fr = フランス語
locale-name-de-de = ドイツ語
locale-name-es-es = スペイン語
locale-name-it-it = イタリア語
locale-name-ja-jp = 日本語
locale-name-lv-lv = ラトビア語
locale-name-pl-pl = ポーランド語
locale-name-pt-br = ポルトガル語（ブラジル）
locale-name-ru = ロシア語
locale-name-tr-tr = トルコ語
locale-name-uk-ua = ウクライナ語
locale-name-zh-cn = 中国語（簡体字）
locale-name-zh-tw = 中国語（繁体字）

## Empty-state hero (views/main_window.rs)

hero-drop = ここにメディアをドロップ、または ⌘O
hero-import-button = ファイルをインポート…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = ファイル名 / URL
table-col-model = モデル
table-col-task = タスク
table-col-status = ステータス
table-col-date-completed = 完了日
table-col-date-added = 追加日
table-col-notes = メモ

## Task table — header/row context menus

table-menu-reset-column-order = 列の順序をリセット
table-menu-restart = 文字起こしを再開する
table-menu-rename = 名前を変更
table-menu-notes = メモを追加・編集

## Task table — Task column labels (viewmodel.rs)

task-transcribe = 文字起こし
task-translate = 英語に翻訳

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = 音声を分離中

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = 録音する
toolbar-new-file = 新しいファイルの文字起こし
toolbar-new-url = 新しいURLの文字起こし
toolbar-open-transcript = 文字起こしを開く
toolbar-open-transcript-tooltip = 開く文字起こしを1つ以上選択してください
toolbar-cancel = 文字起こしをキャンセルする
toolbar-clear = 履歴を削除する

## Menu bar (views/main_window.rs, F-004)

menu-file = ファイル
menu-file-import-file = ファイルをインポートする…
menu-file-import-url = URLをインポートする…
menu-file-import-folder = フォルダをインポートする…
menu-help-title = ヘルプ
menu-help-about = humm について
menu-help-docs = ヘルプ
menu-help-docs-tooltip = ドキュメントは最初のリリースで提供されます
menu-help-preferences = 設定…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = 履歴を削除する
clear-confirm-body = 本当に選択された文字起こしを削除しますか？ この操作は元に戻せません。
clear-confirm-ok = Ok
clear-confirm-cancel = キャンセル
clear-error = 選択された文字起こしを削除できませんでした

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = 文字起こしの名前を変更
rename-label = 新しい名前を入力してください:
rename-error = 名前を変更できませんでした
notes-title = メモ
notes-label = この文字起こしに関するメモを入力してください:
notes-error = メモを保存できませんでした
dialog-ok = Ok
dialog-cancel = キャンセル

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = 文字起こしが完了しました
toast-completed-suffix = 個のファイルを書き出しました
toast-canceled = 文字起こしがキャンセルされました
toast-detail-title = 文字起こしに失敗しました
toast-detail-open-log = ログフォルダを開く
toast-detail-retry = 再試行
toast-detail-close = 閉じる
retry-error-unavailable = 再開できません: 保存されたオプションが利用できません

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm は履歴の更新を記録できませんでした — 出力は完了しています。ログファイルを確認してください

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = ドロップしてインポート

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp が見つかりません — インストールしてください（例: `brew install yt-dlp`）、または HUMM_YTDLP を設定してください

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = URLをインポートする
url-dialog-label = URL:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = 入力されたURLは無効です。
url-dialog-ok = Ok
url-dialog-cancel = キャンセル

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = モデル:
options-task-label = タスク:
options-language-label = 言語:
language-detect = 自動検出
options-word-timestamps = 単語レベルでのタイミング
options-extract-speech = 音声を抽出する
options-extract-speech-missing-tooltip = demucs が見つかりません — 音声抽出を有効にするにはインストールしてください（例: `pipx install demucs`）
options-export-label = 出力形式:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = 出力形式が選択されていません
options-advanced = 詳細設定
options-initial-prompt-label = プロンプト:
options-hf-repo-label = Hugging Face モデル
options-run = 実行

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = バックエンド:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = APIキー:
options-llm-heading = 翻訳設定
options-llm-enable = AIによる翻訳を有効にする
options-llm-model-label = AIのモデル:
options-llm-model-presets-button = プリセット
options-llm-prompt-label = AIへの指示:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = { $name } を監視中
folderwatch-pill-paused = 一時停止中
folderwatch-pill-pause-tip = フォルダ監視を一時停止
folderwatch-pill-resume-tip = フォルダ監視を再開

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = アップデートあり
update-dialog-title = アップデートあり
update-dialog-header = hummの新しいバージョンが利用可能です！
update-dialog-current-version = 現在のバージョン:
update-dialog-new-version = 新しいバージョン:
update-dialog-release-notes = リリースノート:
update-dialog-open-release-page = リリースページを開く
update-dialog-close = 閉じる

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = バージョン
about-check-button = アップデートを確認する
about-show-logs-button = ログを表示
about-close-button = 閉じる
about-up-to-date = 最新の状態です!
about-check-failed = 更新の確認に失敗しました — ログファイルを確認してください

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = 設定
prefs-tab-general = 一般
prefs-tab-models = モデル
prefs-tab-shortcuts = ショートカット
prefs-tab-folder-watch = フォルダ監視
prefs-tab-postproc = 後処理
prefs-tab-privacy = プライバシー

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = UIの言語
prefs-general-font-size-label = フォントサイズ
prefs-general-font-size-custom-label = カスタムフォントサイズ
prefs-general-openai-key-label = OpenAI APIキー
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = 表示
prefs-general-key-hide = 非表示
prefs-general-test-button = テスト
prefs-general-test-title = OpenAI APIキー テスト
prefs-general-test-success-body = あなたのAPIキーは有効です。hummはこのキーを使ってWhisper APIの書き起こしとAI翻訳を行います。
prefs-general-invalid-key-title = 無効なAPIキー
prefs-general-invalid-key-body = APIはbase64文字（A-Za-z0-9+/=_-）のみをサポートしています。APIキーにその他の文字が含まれているとエラーが発生する場合があります。
prefs-general-key-save-error-title = エラー
prefs-general-key-save-error-body = OpenAI API キーをkeyringに保存できません
prefs-general-base-url-label = OpenAI ベースURL
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI APIモデル
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = デフォルトの出力ファイル名
prefs-general-force-cpu-row-label = GPUを無効にする
prefs-general-force-cpu-label = CPUのみを使用してGPUアクセラレーションを無効にする
prefs-general-force-cpu-tooltip = 大きなモデルがGPUメモリに収まらずhummがクラッシュする場合に設定してください

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = 匿名の起動テレメトリを送信する
privacy-telemetry-disabled-caption = このビルドはコンパイル時にテレメトリが無効化されています。
privacy-telemetry-items-intro = 各起動イベントには以下が含まれます:
privacy-telemetry-item-app-version = アプリバージョン
privacy-telemetry-item-system-name = システム名
privacy-telemetry-item-os-version = OSバージョン
privacy-telemetry-item-kernel-release = カーネルリリース
privacy-telemetry-item-cpu-architecture = CPUアーキテクチャ
privacy-telemetry-item-system-locale = システムロケール
privacy-telemetry-item-identifier = ランダムな匿名識別子
privacy-update-checkbox = 自動的に更新を確認する

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = ダウンロード済み
models-section-available = ダウンロード可能
models-download-button = ダウンロード
models-delete-button = 削除
models-show-location-button = ファイルの場所を表示
models-cancel-button = キャンセル
models-custom-url-label = カスタム ggml モデル URL:
models-delete-confirm-title = モデルを削除
models-delete-confirm-body = 選択したモデルを本当に削除しますか?
models-download-error-title = エラー
models-download-failed = ダウンロード失敗
models-speed-badge = { $ratio }× リアルタイム
models-speed-badge-tip = このマシンのライブ録音セッションから測定されたリアルタイム速度

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = 録音画面を開く
shortcut-desc-import-file = ファイルをインポートする
shortcut-desc-import-url = URLをインポートする
shortcut-desc-open-preferences-window = 設定画面を開く
shortcut-desc-view-transcript-text = 文字起こしを表示する
shortcut-desc-view-transcript-translation = 文字起こしの翻訳を表示する
shortcut-desc-view-transcript-timestamps = 文字起こしのタイムスタンプを表示する
shortcut-desc-search-transcript = 文字起こしを検索
shortcut-desc-search-next = 次の文字起こし検索結果へ移動
shortcut-desc-search-previous = 前の文字起こし検索結果へ移動
shortcut-desc-scroll-to-current-text = 現在のテキストにスクロール
shortcut-desc-play-pause-audio = 音声の再生・一時停止
shortcut-desc-replay-current-segment = 現在のセグメントを再再生
shortcut-desc-toggle-playback-controls = 再生コントロールの表示切替
shortcut-desc-decrease-segment-start = セグメント開始時刻を早める
shortcut-desc-increase-segment-start = セグメント開始時刻を遅らせる
shortcut-desc-decrease-segment-end = セグメント終了時刻を早める
shortcut-desc-increase-segment-end = セグメント終了時刻を遅らせる
shortcut-desc-clear-history = 履歴を削除する
shortcut-desc-stop-transcription = 文字起こしをキャンセルする
prefs-shortcuts-press = ショートカットキーを押してください…
prefs-shortcuts-clear-tooltip = デフォルトに戻す
prefs-shortcuts-conflict = { $other } と競合しています
prefs-shortcuts-reset = デフォルトに戻す

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = フォルダ監視を有効にする
prefs-fw-input-folder-label = 入力フォルダ
prefs-fw-input-folder-placeholder = /入力フォルダへのパス
prefs-fw-output-folder-label = 出力フォルダ
prefs-fw-output-folder-placeholder = /出力フォルダへのパス
prefs-fw-browse = 参照
prefs-fw-delete-checkbox = 処理済みファイルを削除
prefs-fw-disposition-note = 文字起こしが完了すると、元のファイルは出力フォルダに移動されます — 「処理済みファイルを削除」がオンの場合は削除されます。

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = これらのフックは各文字起こしが完了すると自動的に実行されます。フックが失敗してもログに記録されるだけで、文字起こし自体が失敗することはありません。
prefs-postproc-skip-title = 転写済みをスキップ
prefs-postproc-skip-enable = 「既に文字起こし済みならスキップ」を有効にする
prefs-postproc-skip-check-files = 既存の結果ファイルを確認
prefs-postproc-skip-check-files-desc = 音声ファイル名と一致する .txt、.srt、または .vtt ファイルが隣にある場合はスキップします
prefs-postproc-skip-check-db = 転写データベースを確認
prefs-postproc-skip-check-db-desc = このファイルが既に文字起こし済みで、データベースに記録がある場合はスキップします
prefs-postproc-dfn-title = DeepFilterNet ノイズ除去
prefs-postproc-dfn-enable = DeepFilterNet ノイズ低減を有効にする
prefs-postproc-dfn-keep-file = 文字起こし後にノイズ除去ファイルを保持する
prefs-postproc-langdetect-title = 強化された言語検出
prefs-postproc-langdetect-enable = 拡張言語検出を有効にする
prefs-postproc-langdetect-download-tiny = 利用できるモデルがない場合は tiny モデルをダウンロード
prefs-postproc-langdetect-download-tiny-desc = whisper.cpp モデルがダウンロードされていない場合、言語検出に使用する tiny モデルをダウンロードします。
prefs-postproc-resizer-title = 自動文字起こしリサイズ
prefs-postproc-resizer-enable = 自動文字起こしリサイザーを有効にする
prefs-postproc-resizer-merge-by-gap = 間隔で結合
prefs-postproc-resizer-merge-gap-seconds = 結合の間隔（秒）
prefs-postproc-resizer-split-by-punctuation = 句読点で分割
prefs-postproc-resizer-punctuation = 句読点
prefs-postproc-resizer-split-by-max-length = 最大長で分割
prefs-postproc-resizer-max-length = 字幕の最大長
prefs-postproc-summary-title = AI要約
prefs-postproc-summary-enable = AI要約を有効にする
prefs-postproc-summary-api-url = APIベースURL
prefs-postproc-summary-api-key-label = APIキー
prefs-postproc-summary-model = モデル
prefs-postproc-summary-prompt = 要約プロンプト
prefs-postproc-summary-save-to-notes = 要約をメモに保存
prefs-postproc-summary-save-to-file = 要約をファイルに保存
prefs-postproc-summary-output-folder = 出力フォルダ（ファイル用）
prefs-postproc-summary-key-save-error-body = AI要約のAPIキーをkeyringに保存できません
prefs-postproc-docx-title = DOCXにエクスポート
prefs-postproc-docx-enable = DOCXへのエクスポートを有効にする
prefs-postproc-docx-output-folder = 出力フォルダ
prefs-postproc-docx-include-timestamps = タイムスタンプを含める
prefs-postproc-output-folder-hint = 空欄にするとソースファイルの隣に保存します。

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = 表示
viewer-mode-text = テキスト
viewer-mode-translation = 翻訳
viewer-mode-timestamps = タイムスタンプ
viewer-export = 出力
viewer-translate = 翻訳
viewer-resize = リサイズ
viewer-identify-speakers = 話者を識別する
viewer-find = 検索
viewer-find-label = 検索:
viewer-find-placeholder = 検索するテキストを入力...
viewer-find-clear = クリア
viewer-find-prev-tooltip = 前の一致（{ $shortcut }）
viewer-find-next-tooltip = 次の一致（{ $shortcut }）
viewer-empty = セグメントがありません — 入力が無音のみの可能性があります
viewer-open-error = 文字起こしを開けませんでした
viewer-col-start = 開始
viewer-col-end = 終了
viewer-col-text = テキスト
viewer-col-translation = 翻訳
viewer-saved = 編集済み・保存済み
viewer-unsaved-warning = 編集内容を保存できませんでした — メモリ内には保持されていますが、ディスクには書き込まれていません
viewer-player-loading = 音声を読み込み中…
viewer-player-failed = 再生を利用できません
viewer-playback-controls = 再生コントロール:
viewer-loop-segment = セグメントをループ
viewer-loop-segment-tooltip = 文字起こしセグメントをクリックしたときのループを有効・無効にする
viewer-follow-audio = 音声に追従
viewer-follow-audio-tooltip = 文字起こし内で現在の音声位置への追従を有効・無効にします。有効にすると、現在のテキストに自動的にスクロールします。
viewer-speed = 速度:
viewer-scroll-to-current = 現在位置にスクロール
viewer-scroll-to-current-tooltip = 現在話されているテキストにスクロールする

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = APIキーが必要
viewer-translate-key-required-body = 設定画面でOpenAI APIキーを入力してください
viewer-translate-settings-title = AI翻訳設定

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = テキスト
viewer-export-translation = 翻訳
viewer-export-dialog-title = ファイルを保存
viewer-export-filter = テキストファイル
viewer-export-failed = エクスポートを書き出せませんでした — 詳細はログを確認してください
export-written = { $path } に書き出しました

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = 設定
resizer-create-new = 新しい文字起こしを作成
resizer-extend-heading = 終了時刻を延長
resizer-extend-label = 終了を最大（秒）まで延長
resizer-extend-button = 終了を延長
resizer-resize-heading = リサイズオプション
resizer-resize-label = 希望する字幕の長さ
resizer-resize-button = リサイズ
resizer-resize-disabled-tooltip = 文字起こし時に単語レベルのタイミングが無効だった場合のみ使用可能
resizer-merge-heading = 結合オプション
resizer-merge-by-gap = 間隔で結合
resizer-split-by-punctuation = 句読点で分割
resizer-split-by-max-length = 最大長で分割
resizer-merge-button = 結合
resizer-merge-disabled-tooltip = 文字起こし時に単語レベルのタイミングが有効だった場合のみ使用可能
resizer-merging = 結合中…
resizer-merge-error = 結合を実行できませんでした:
resizer-save-error = 変更を保存できませんでした:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = ステップ1: 話者を識別する
speaker-id-step2-heading = ステップ2: 話者に名前を付ける
speaker-id-ready-label = 話者を識別する準備ができました
speaker-id-missing-audio-label = 音声ファイルが見つかりません
speaker-id-identify-button = 識別
speaker-id-cancel-button = キャンセル
speaker-id-cancelling = キャンセル中...
speaker-id-decoding = 音声をデコード中…
speaker-id-downloading-models = モデルをダウンロード中…
speaker-id-identifying = 話者を識別中…
speaker-id-play-sample = サンプルを再生
speaker-id-merge-checkbox = 話者の文を結合
speaker-id-save-button = 保存
speaker-id-save-error = 変更を保存できませんでした:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = ライブ録音
recording-model-load-note = モデルの初回使用時は読み込みに数分かかる場合があります。
recording-advanced-button = 高度な設定...
recording-microphone-label = マイク:
recording-record-button = 録音する
recording-stop-button = 停止する
recording-realtime-warning = 前回の実行で { $size } を { $ratio }× のリアルタイム速度で文字起こししました — ライブ文字起こしが遅れる可能性があります
recording-chunk-skipped-warning = チャンクが失敗しスキップされました: { $detail }
recording-panel-placeholder = クリックで録音を開始...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = AI翻訳を待っています...
recording-model-downloading = モデルをダウンロード中… { $percent }%
recording-model-downloading-indeterminate = モデルをダウンロード中…
recording-model-loading = モデルを読み込み中…
recording-error-prefix = 新規録音開始時にエラーが発生しました:
recording-error-suffix = オーディオデバイスを確認するか、詳細についてはアプリケーションのログを確認してください。
recording-thread-crashed = 録音セッションが予期せず終了しました。詳細はアプリケーションのログを確認してください。
meter-average-volume = 平均音量
meter-queue = キュー

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = 高度な設定
recording-advanced-speech-heading = 音声認識設定
recording-advanced-initial-prompt-label = プロンプト:
recording-advanced-translation-heading = 翻訳設定
recording-advanced-llm-enable = AIによる翻訳を有効にする
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = 録音設定
recording-advanced-silence-threshold-label = 無音閾値:
recording-advanced-mode-label = ライブ録音モード:
recording-advanced-mode-append-below = 下に追加
recording-advanced-mode-append-above = 上に追加
recording-advanced-mode-append-and-correct = 追加して修正
recording-advanced-separator-label = 行区切り:
recording-advanced-step-label = トランスクリプションステップ:
recording-advanced-hide-unconfirmed = 未確認を非表示
recording-advanced-export-enable = ライブ録音の出力を有効にする
recording-advanced-export-folder-label = 出力フォルダ:
recording-advanced-export-browse = 参照
recording-advanced-export-file-name-label = 出力ファイル名:
recording-advanced-export-file-type-label = 出力ファイル形式:
recording-advanced-export-file-type-txt = テキストファイル (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    出力エントリ数の制限
    （0 = すべて出力）:
recording-advanced-upload-url-label = アップロードURL:
recording-advanced-ok = Ok

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = 表示
presentation-show-tooltip = 新しいウィンドウで表示
presentation-text-size-label = テキストサイズ:
presentation-theme-label = テーマ
presentation-theme-light = ライト
presentation-theme-dark = ダーク
presentation-theme-custom = カスタム
presentation-text-color-label = テキストの色
presentation-background-color-label = 背景の色
presentation-fullscreen-button = 全画面表示
presentation-window-title = ライブ文字起こしプレゼンテーション

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = コピー
recording-copy-tooltip = 文字起こしをクリップボードにコピー
recording-copy-copied = コピーしました！
recording-copy-nothing = コピーするものがありません！

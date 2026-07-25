## humm-gui — Simplified Chinese (zh-CN) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b, design spec §9.2). Section
## comments stay in English on purpose: they are developer-facing notes about
## *where* a key renders, identical in every catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own zh_CN translation is used
## verbatim (`buzz/locale/zh_CN/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh in the same
## concise UI register. Buzz's zh translators render "transcription" as 识别
## (recognition) rather than 转录 in most surfaces, and that term is kept
## wherever Buzz used it — including the places Buzz itself is inconsistent, so
## the lineage stays auditable rather than silently harmonized.
##
## One mechanical correction to the Buzz source: strings naming the product
## "Buzz" are re-pointed at "humm", matching the en catalog's own substitution.
## (The concatenated-variant artifacts that affect Buzz's de/es catalogs do not
## occur in the zh strings this catalog draws on.)
##
## Typography: full-width punctuation (：？！（）) is used where Buzz's own zh
## catalog uses it, and Buzz's half-width choices are likewise kept verbatim —
## the mix below is Buzz's, not an oversight. A single space separates CJK from
## latin identifiers and placeables where one aids legibility (humm, GPU,
## { $path }), per common zh UI practice.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry. Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), translated by Buzz's own po.

locale-name-en-us = 英语
locale-name-de-de = 德语
locale-name-es-es = 西班牙语
locale-name-zh-cn = 中文（简体）

## Empty-state hero (views/main_window.rs)

hero-drop = 将媒体文件拖放到此处，或按 ⌘O
hero-import-button = 导入文件…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = 文件名称/URL
table-col-model = 模型
table-col-task = 任务
table-col-status = 状态
table-col-date-completed = 完成时间
table-col-date-added = 添加日期
table-col-notes = 备注

## Task table — header/row context menus

table-menu-reset-column-order = 重置列顺序
table-menu-restart = 重新开始识别
table-menu-rename = 重命名
table-menu-notes = 添加/编辑备注

## Task table — Task column labels (viewmodel.rs)

task-transcribe = 识别
task-translate = 翻译成英语

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = 录制
toolbar-new-file = 新增文件识别
toolbar-new-url = 新增URL识别
toolbar-open-transcript = 打开识别结果
toolbar-open-transcript-tooltip = 选择一个或多个已完成的识别结果以打开
toolbar-cancel = 取消识别
toolbar-clear = 清除历史纪录

## Menu bar (views/main_window.rs, F-004)

menu-file = 文件
menu-file-import-file = 导入文件…
menu-file-import-url = 导入URL…
menu-file-import-folder = 导入文件夹…
menu-help-title = 帮助
menu-help-about = 关于 humm
menu-help-docs = 帮助
menu-help-docs-tooltip = 文档将随首个版本一同发布
menu-help-preferences = 偏好设置…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## Buzz's zh po keeps the English "Ok" spelling for its relabelled dialog
## buttons, so clear-confirm-ok/url-dialog-ok/recording-advanced-ok/dialog-ok
## stay "Ok" here — Buzz's own zh lineage, not untranslated English.
## clear-confirm-body is Buzz's string verbatim, including its 录制 wording.

clear-confirm-title = 清除历史纪录
clear-confirm-body = 您确定要删除所选录制吗？此操作无法撤消。
clear-confirm-ok = Ok
clear-confirm-cancel = 取消
clear-error = 无法删除所选识别结果

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = 重命名识别
rename-label = 输入新名称：
rename-error = 无法重命名
notes-title = 备注
notes-label = 为此识别输入相关备注：
notes-error = 无法保存备注
dialog-ok = Ok
dialog-cancel = 取消

## Toasts (views/toasts.rs, app.rs, F-026)
##
## toast-completed-suffix renders after a count ("… — 3 个文件已写入"), so it
## carries the measure word and reads as a noun phrase, not a sentence.

toast-completed = 识别完成
toast-completed-suffix = 个文件已写入
toast-canceled = 识别已取消
toast-detail-title = 识别失败
toast-detail-open-log = 打开日志文件夹
toast-detail-retry = 重试
toast-detail-close = 关闭
retry-error-unavailable = 无法重新开始：已保存的选项不可用

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm 无法记录历史更新——导出仍会正常完成；请查看日志文件

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = 拖放到此处以导入

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## The binary name, the shell command and the env var are identifiers and stay
## verbatim; only the prose around them is translated.

url-import-unavailable-tooltip = 缺少 yt-dlp——请安装它（例如 `brew install yt-dlp`）或设置 HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)

url-dialog-title = 导入URL
url-dialog-label = 网址:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = 输入的网址无效
url-dialog-ok = Ok
url-dialog-cancel = 取消

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = 模型:
options-task-label = 任务:
options-language-label = 语言:
language-detect = 检测语言
options-word-timestamps = 逐词识别
options-extract-speech = 提取语音
options-extract-speech-tooltip = 语音提取将在后续版本中推出
options-export-label = 导出:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = 未选择导出格式
options-advanced = 高级
options-initial-prompt-label = 初始提示：
options-hf-repo-label = Hugging Face 模型
options-run = 开始执行

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2).
##
## options-api-key-label keeps Buzz's own zh rendering, which leaves "Api Key"
## in latin script (Buzz's zh catalog does this consistently for API keys).

options-backend-label = 后端:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Api Key:
options-llm-heading = 翻译设置
options-llm-enable = 启用AI翻译
options-llm-model-label = AI 模型:
options-llm-model-presets-button = 预设
options-llm-prompt-label = AI说明:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = 正在监控 { $name }
folderwatch-pill-paused = 已暂停
folderwatch-pill-pause-tip = 暂停文件夹监控
folderwatch-pill-resume-tip = 恢复文件夹监控

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = 有可用更新
update-dialog-title = 有可用更新
update-dialog-header = humm 有新版本可用！
update-dialog-current-version = 当前版本：
update-dialog-new-version = 新版本：
update-dialog-release-notes = 发行说明：
update-dialog-open-release-page = 打开发布页面
update-dialog-close = 关闭

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = 版本
about-check-button = 检查更新
about-show-logs-button = 显示日志
about-close-button = 关闭
about-up-to-date = 已经是最新版本
about-check-failed = 更新检查失败——请查看日志文件

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = 偏好设置
prefs-tab-general = 通用
prefs-tab-models = 模型
prefs-tab-privacy = 隐私

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1)

prefs-general-ui-language-label = 界面语言
prefs-general-font-size-label = 字体大小
prefs-general-font-size-custom-label = 自定义字体大小
prefs-general-openai-key-label = OpenAI API key
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = 显示
prefs-general-key-hide = 隐藏
prefs-general-test-button = 测试
prefs-general-test-title = 测试OpenAI API Key
prefs-general-test-success-body = 您的API密钥有效。humm 将使用此密钥执行 Whisper API 识别和 AI 翻译。
prefs-general-invalid-key-title = 无效的API key
prefs-general-invalid-key-body = API只支持 base64字符（A-Za-z0-9+/=_-）。其他字符在API密钥中可能导致错误。
prefs-general-key-save-error-title = 错误
prefs-general-key-save-error-body = 无法将OpenAI API密钥保存到密钥串
prefs-general-base-url-label = OpenAI 基于 url
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI API 模型
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = 默认输出文件名
prefs-general-force-cpu-row-label = 禁用 GPU
prefs-general-force-cpu-label = 仅使用 CPU 并禁用 GPU 加速
prefs-general-force-cpu-tooltip = 如果较大的模型无法适配您的 GPU 内存且 humm 崩溃，请启用此选项

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = 发送匿名启动遥测数据
privacy-telemetry-disabled-caption = 此版本在编译时已禁用遥测。
privacy-telemetry-items-intro = 每次启动事件包含：
privacy-telemetry-item-app-version = 应用版本
privacy-telemetry-item-system-name = 系统名称
privacy-telemetry-item-os-version = 操作系统版本
privacy-telemetry-item-kernel-release = 内核版本
privacy-telemetry-item-cpu-architecture = CPU 架构
privacy-telemetry-item-system-locale = 系统区域设置
privacy-telemetry-item-identifier = 一个随机匿名标识符
privacy-update-checkbox = 自动检查更新

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = 已下载
models-section-available = 可用的下载
models-download-button = 下载
models-delete-button = 删除
models-show-location-button = 查看文件位置
models-cancel-button = 取消
models-custom-url-label = 自定义 ggml 模型 URL：
models-delete-confirm-title = 删除模型
models-delete-confirm-body = 您确定要删除所选模型吗？
models-download-error-title = 错误
models-download-failed = 下载失败

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = 查看
viewer-mode-text = 文本
viewer-mode-translation = 翻译
viewer-mode-timestamps = 时间戳
viewer-export = 导出
viewer-translate = 翻译
viewer-resize = 调整大小
viewer-identify-speakers = 识别说话人
viewer-identify-speakers-tooltip = 说话人识别将在后续版本中推出
viewer-find = 查找
viewer-find-label = 查找：
viewer-find-placeholder = 输入要查找的文本...
viewer-find-clear = 清除
viewer-find-prev-tooltip = 上一个匹配项（{ $shortcut }）
viewer-find-next-tooltip = 下一个匹配项（{ $shortcut }）
viewer-empty = 没有片段——输入内容可能只有静音
viewer-open-error = 无法打开识别结果
viewer-col-start = 开始
viewer-col-end = 结束
viewer-col-text = 文本
viewer-col-translation = 翻译
viewer-saved = 已编辑 · 已保存
viewer-unsaved-warning = 有一处编辑无法保存——它保留在内存中，但未写入磁盘
viewer-player-loading = 正在加载音频…
viewer-player-failed = 无法播放
viewer-playback-controls = 播放控制：
viewer-loop-segment = 循环片段
viewer-loop-segment-tooltip = 点击识别片段时启用/禁用循环播放
viewer-follow-audio = 跟随音频
viewer-follow-audio-tooltip = 在识别文本中启用/禁用跟随当前音频位置。启用后，自动滚动到当前文本。
viewer-speed = 速度：
viewer-scroll-to-current = 滚动到当前位置
viewer-scroll-to-current-tooltip = 滚动到当前正在播放的文本

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3)

viewer-translate-key-required-title = 需要API Key
viewer-translate-key-required-body = 请在偏好设置中输入OpenAI API Key
viewer-translate-settings-title = AI 翻译设置

## Export menu (views/export_menu.rs, F-084)

viewer-export-text = 文本
viewer-export-translation = 翻译
viewer-export-dialog-title = 保存文件
viewer-export-filter = 文本文件
viewer-export-failed = 无法写入导出文件——详情请查看日志
export-written = 已写入 { $path }

## Transcript resizer window (views/resizer.rs, F-086)

resizer-settings = 设置
resizer-create-new = 创建新转录
resizer-extend-heading = 延长结束时间
resizer-extend-label = 最多延长结束时间（秒）
resizer-extend-button = 延长结尾
resizer-resize-heading = 调整大小选项
resizer-resize-label = 所需字幕长度
resizer-resize-button = 调整大小
resizer-resize-disabled-tooltip = 仅在转录时禁用逐词时间戳时可用
resizer-merge-heading = 合并选项
resizer-merge-by-gap = 按间隔合并
resizer-split-by-punctuation = 按标点分割
resizer-split-by-max-length = 按最大长度分割
resizer-merge-button = 合并
resizer-merge-disabled-tooltip = 仅在转录时启用逐词时间戳时可用
resizer-merging = 正在合并…
resizer-merge-error = 无法执行合并：
resizer-save-error = 无法保存更改：

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)

recording-window-title = 实时录制
recording-model-load-note = 首次使用模型可能需要几分钟的时间才能加载
recording-advanced-button = 高级...
recording-microphone-label = 麦克风:
recording-record-button = 录制
recording-stop-button = 停止
recording-realtime-warning = { $size } 上次运行以 { $ratio }× 实时速度完成识别——实时识别可能会跟不上
recording-panel-placeholder = 点击开始录制
recording-panel-translation-placeholder = 等待AI翻译...
recording-model-downloading = 模型下载中… { $percent }%
recording-model-downloading-indeterminate = 模型下载中…
recording-model-loading = 模型加载中…
recording-error-prefix = 开始新录制时出错
recording-error-suffix = 请检查您的音频设备或检查应用程序日志以获取更多信息。
recording-thread-crashed = 录制会话意外结束。请检查应用程序日志以获取更多信息。
meter-average-volume = 平均音量
meter-queue = 队列

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060)

recording-advanced-title = 高级设置
recording-advanced-speech-heading = 语音识别设置
recording-advanced-initial-prompt-label = 初始提示：
recording-advanced-translation-heading = 翻译设置
recording-advanced-llm-enable = 启用AI翻译
recording-advanced-recording-heading = 录制设置
recording-advanced-silence-threshold-label = 静音阈值：
recording-advanced-mode-label = 实时录制模式:
recording-advanced-mode-append-below = 增加下方
recording-advanced-mode-append-above = 增加上方
recording-advanced-mode-append-and-correct = 增加并纠正
recording-advanced-separator-label = 行分隔符：
recording-advanced-step-label = 转录步长:
recording-advanced-hide-unconfirmed = 隐藏未确认内容
recording-advanced-export-enable = 启用实时录制导出
recording-advanced-export-folder-label = 导出文件夹：
recording-advanced-export-browse = 浏览
recording-advanced-export-file-name-label = 导出文件名：
recording-advanced-export-file-type-label = 导出文件类型：
recording-advanced-export-file-type-txt = 文本文件 (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    限制导出条目数
    （0 = 全部导出）：
recording-advanced-upload-url-label = 上传 URL：
recording-advanced-ok = Ok

## Presentation window + bar (views/presentation.rs, F-065)

presentation-show-button = 显示
presentation-show-tooltip = 在新窗口中显示
presentation-text-size-label = 文字大小：
presentation-theme-label = 主题
presentation-theme-light = 浅色
presentation-theme-dark = 深色
presentation-theme-custom = 自定义
presentation-text-color-label = 文字颜色
presentation-background-color-label = 背景颜色
presentation-fullscreen-button = 全屏
presentation-window-title = 实时识别展示

## Copy bar (views/recording.rs, F-065 §2.7)

recording-copy-button = 复制
recording-copy-tooltip = 复制识别内容到剪贴板
recording-copy-copied = 已复制！
recording-copy-nothing = 没有可复制的内容！

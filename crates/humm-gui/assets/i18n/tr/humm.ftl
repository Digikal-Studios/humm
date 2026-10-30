## humm-gui — Turkish (tr) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b, design spec §9.2). Section
## comments stay in English on purpose: they are developer-facing notes about
## *where* a key renders, identical in every catalog, not translatable copy.
##
## Provenance: wherever a humm string matches or closely paraphrases a Buzz
## msgid, Buzz's own `tr_TR` translation is used verbatim
## (`buzz/locale/tr_TR/LC_MESSAGES/buzz.po` at Buzz `298dbf0`); product name
## "Buzz" is re-pointed at "humm". humm-only strings with no Buzz msgid keep
## the English source value until a dedicated pass fills them.
##
## Product name "humm" is never translated.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry. Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py), translated by Buzz's own po.

locale-name-en-us = İngilizce
locale-name-ca-es = Katalanca
locale-name-da-dk = Danca
locale-name-nl = Felemenkçe
locale-name-fr = Fransızca
locale-name-de-de = Almanca
locale-name-es-es = İspanyolca
locale-name-it-it = İtalyanca
locale-name-ja-jp = Japonca
locale-name-lv-lv = Letonca
locale-name-pl-pl = Lehçe
locale-name-pt-br = Portekizce (Brezilya)
locale-name-ru = Rusça
locale-name-tr-tr = Türkçe
locale-name-uk-ua = Ukraynaca
locale-name-zh-cn = Çince (Basitleştirilmiş)
locale-name-zh-tw = Çince (Geleneksel)

## Empty-state hero (views/main_window.rs)

hero-drop = Drop media here, or ⌘O
hero-import-button = Import files…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Dosya Adı / URL
table-col-model = Model
table-col-task = Görev
table-col-status = Durum
table-col-date-completed = Tamamlanma Tarihi
table-col-date-added = Eklenme Tarihi
table-col-notes = Notlar

## Task table — header/row context menus

table-menu-reset-column-order = Sütun Sırasını Sıfırla
table-menu-restart = Deşifreyi Yeniden Başlat
table-menu-rename = Yeniden Adlandır
table-menu-notes = Not Ekle/Düzenle

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Yazıya Dök
task-translate = İngilizceye Çevir

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = isolating speech

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Kayıt
toolbar-new-file = Yeni Dosya Deşifresi
toolbar-new-url = Yeni URL Deşifresi
toolbar-open-transcript = Deşifreyi Aç
toolbar-open-transcript-tooltip = Select one or more completed transcriptions to open
toolbar-cancel = Deşifreyi İptal Et
toolbar-clear = Geçmişi Temizle

## Menu bar (views/main_window.rs, F-004)

menu-file = Dosya
menu-file-import-file = Dosya İçe Aktar...
menu-file-import-url = URL İçe Aktar...
menu-file-import-folder = Klasör İçe Aktar...
menu-help-title = Yardım
menu-help-about = About humm
menu-help-docs = Yardım
menu-help-docs-tooltip = Documentation arrives with the first release
menu-help-preferences = Tercihler...

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Geçmişi Temizle
clear-confirm-body = Seçili deşifre(ler)i silmek istediğinizden emin misiniz? Bu işlem geri alınamaz.
clear-confirm-ok = Tamam
clear-confirm-cancel = İptal
clear-error = Could not delete the selected transcription(s)

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Deşifreyi Yeniden Adlandır
rename-label = Yeni adı girin:
rename-error = Could not rename
notes-title = Notlar
notes-label = Bu deşifre için ilgili notları girin:
notes-error = Could not save notes
dialog-ok = Tamam
dialog-cancel = İptal

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transcription complete
toast-completed-suffix = file(s) written
toast-canceled = Transcription canceled
toast-detail-title = Transcription Failed
toast-detail-open-log = Open log folder
toast-detail-retry = Retry
toast-detail-close = Kapat
retry-error-unavailable = Could not restart: saved options are unavailable

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm couldn't record history updates — exports still complete; see the log file

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Drop to import

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp missing — install it (e.g. `brew install yt-dlp`) or set HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = URL İçe Aktar
url-dialog-label = URL:
url-dialog-placeholder = https://example.com/audio.mp3
url-dialog-invalid = Girdiğiniz URL geçersiz.
url-dialog-ok = Tamam
url-dialog-cancel = İptal

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Model:
options-task-label = Görev:
options-language-label = Dil:
language-detect = Dili Algıla
options-word-timestamps = Kelime düzeyinde zamanlamalar
options-extract-speech = Konuşmayı ayıkla
options-extract-speech-missing-tooltip = demucs not found — install it (e.g. `pipx install demucs`) to enable speech extraction
options-export-label = Dışa aktar:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = no export selected
options-advanced = Gelişmiş...
options-initial-prompt-label = Başlangıç İstemi:
options-hf-repo-label = Hugging Face model
options-run = Çalıştır

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = API Anahtarı:
options-llm-heading = Çeviri ayarları
options-llm-enable = Yapay zekâ çevirisini etkinleştir
options-llm-model-label = Yapay zekâ modeli:
options-llm-model-presets-button = Presets
options-llm-prompt-label = Yapay zekâ için talimatlar:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Watching { $name }
folderwatch-pill-paused = Paused
folderwatch-pill-pause-tip = Pause folder watching
folderwatch-pill-resume-tip = Resume folder watching

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Güncelleme Mevcut
update-dialog-title = Güncelleme Mevcut
update-dialog-header = humm'ın yeni bir sürümü mevcut!
update-dialog-current-version = Geçerli sürüm:
update-dialog-new-version = Yeni sürüm:
update-dialog-release-notes = Sürüm Notları:
update-dialog-open-release-page = Open release page
update-dialog-close = Kapat

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Sürüm
about-check-button = Güncellemeleri denetle
about-show-logs-button = Günlükleri göster
about-close-button = Kapat
about-up-to-date = Güncel sürümü kullanıyorsunuz!
about-check-failed = Update check failed — see the log file

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Tercihler
prefs-tab-general = Genel
prefs-tab-models = Modeller
prefs-tab-shortcuts = Kısayollar
prefs-tab-folder-watch = Klasör İzleme
prefs-tab-postproc = Post-processing
prefs-tab-privacy = Privacy

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Arayüz Dili
prefs-general-font-size-label = Yazı Tipi Boyutu
prefs-general-font-size-custom-label = Custom font size
prefs-general-openai-key-label = OpenAI API anahtarı
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Show
prefs-general-key-hide = Hide
prefs-general-test-button = Test Et
prefs-general-test-title = OpenAI API Anahtarı Testi
prefs-general-test-success-body = API anahtarınız geçerli. humm bu anahtarı Whisper API deşifreleri ve yapay zekâ çevirileri için kullanacak.
prefs-general-invalid-key-title = Geçersiz API anahtarı
prefs-general-invalid-key-body = API yalnızca base64 karakterlerini (A-Za-z0-9+/=_-) destekler. API anahtarındaki diğer karakterler hatalara yol açabilir.
prefs-general-key-save-error-title = Hata
prefs-general-key-save-error-body = OpenAI API anahtarı anahtar zincirine kaydedilemedi
prefs-general-base-url-label = OpenAI temel URL'si
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = OpenAI API modeli
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Varsayılan dışa aktarma dosya adı
prefs-general-force-cpu-row-label = GPU'yu Devre Dışı Bırak
prefs-general-force-cpu-label = Yalnızca CPU kullan ve GPU hızlandırmasını devre dışı bırak
prefs-general-force-cpu-tooltip = Büyük modeller GPU belleğinize sığmıyorsa ve humm çöküyorsa bunu etkinleştirin

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Send anonymous launch telemetry
privacy-telemetry-disabled-caption = This build has telemetry disabled at compile time.
privacy-telemetry-items-intro = Each launch event includes:
privacy-telemetry-item-app-version = App version
privacy-telemetry-item-system-name = System name
privacy-telemetry-item-os-version = OS version
privacy-telemetry-item-kernel-release = Kernel release
privacy-telemetry-item-cpu-architecture = CPU architecture
privacy-telemetry-item-system-locale = System locale
privacy-telemetry-item-identifier = A random anonymous identifier
privacy-update-checkbox = Check for updates automatically

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = İndirilenler
models-section-available = İndirilebilir
models-download-button = İndir
models-delete-button = Sil
models-show-location-button = Dosya konumunu göster
models-cancel-button = İptal
models-custom-url-label = Custom ggml model URL:
models-delete-confirm-title = Modeli Sil
models-delete-confirm-body = Seçili modeli silmek istediğinizden emin misiniz?
models-download-error-title = Hata
models-download-failed = İndirme başarısız oldu
models-speed-badge = { $ratio }× realtime
models-speed-badge-tip = Measured realtime speed from this machine's live recording sessions

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Kayıt Penceresini Aç
shortcut-desc-import-file = Dosya İçe Aktar
shortcut-desc-import-url = URL İçe Aktar
shortcut-desc-open-preferences-window = Tercihler Penceresini Aç
shortcut-desc-view-transcript-text = Deşifre Metnini Görüntüle
shortcut-desc-view-transcript-translation = Deşifre Çevirisini Görüntüle
shortcut-desc-view-transcript-timestamps = Deşifre Zaman Damgalarını Görüntüle
shortcut-desc-search-transcript = Deşifrede Ara
shortcut-desc-search-next = Sonraki Arama Sonucuna Git
shortcut-desc-search-previous = Önceki Arama Sonucuna Git
shortcut-desc-scroll-to-current-text = Geçerli Metne Kaydır
shortcut-desc-play-pause-audio = Sesi Oynat/Duraklat
shortcut-desc-replay-current-segment = Geçerli Bölümü Tekrar Oynat
shortcut-desc-toggle-playback-controls = Oynatma Denetimlerini Aç/Kapat
shortcut-desc-decrease-segment-start = Bölüm Başlangıç Süresini Azalt
shortcut-desc-increase-segment-start = Bölüm Başlangıç Süresini Artır
shortcut-desc-decrease-segment-end = Bölüm Bitiş Süresini Azalt
shortcut-desc-increase-segment-end = Bölüm Bitiş Süresini Artır
shortcut-desc-clear-history = Geçmişi Temizle
shortcut-desc-stop-transcription = Deşifreyi İptal Et
prefs-shortcuts-press = Press shortcut…
prefs-shortcuts-clear-tooltip = Reset to default
prefs-shortcuts-conflict = Conflicts with { $other }
prefs-shortcuts-reset = Varsayılanlara Sıfırla

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Klasör izlemeyi etkinleştir
prefs-fw-input-folder-label = Giriş klasörü
prefs-fw-input-folder-placeholder = /path/to/input/folder
prefs-fw-output-folder-label = Çıktı klasörü
prefs-fw-output-folder-placeholder = /path/to/output/folder
prefs-fw-browse = Gözat
prefs-fw-delete-checkbox = İşlenen dosyaları sil
prefs-fw-disposition-note = Once transcribed, the source file is moved into the output folder — or deleted, if Delete processed files is checked.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = These hooks run automatically after each transcription completes. A hook failure is logged but never fails the transcription itself.
prefs-postproc-skip-title = Önceden Deşifre Edilenleri Atla
prefs-postproc-skip-enable = Enable Skip Already Transcribed
prefs-postproc-skip-check-files = Mevcut sonuç dosyalarını kontrol et
prefs-postproc-skip-check-files-desc = Ses dosyasının yanında aynı adlı bir .txt, .srt veya .vtt dosyası bulunursa atla
prefs-postproc-skip-check-db = Deşifre veritabanında kontrol et
prefs-postproc-skip-check-db-desc = Bu dosya daha önce deşifre edilmiş ve kaydı veritabanında varsa atla
prefs-postproc-dfn-title = DeepFilterNet Noise Reduction
prefs-postproc-dfn-enable = Enable DeepFilterNet Noise Reduction
prefs-postproc-dfn-keep-file = Keep denoised file after transcription
prefs-postproc-langdetect-title = Gelişmiş dil algılama
prefs-postproc-langdetect-enable = Enable Enhanced language detection
prefs-postproc-langdetect-download-tiny = Hiç model yoksa tiny modelini indir
prefs-postproc-langdetect-download-tiny-desc = Hiç whisper.cpp modeli indirilmemişse, dil algılamada kullanmak için tiny modelini indirir.
prefs-postproc-resizer-title = Otomatik Deşifre Yeniden Boyutlandırıcı
prefs-postproc-resizer-enable = Enable Automatic Transcript Resizer
prefs-postproc-resizer-merge-by-gap = Boşluğa göre birleştir
prefs-postproc-resizer-merge-gap-seconds = Birleştirme boşluğu (saniye)
prefs-postproc-resizer-split-by-punctuation = Noktalamaya göre böl
prefs-postproc-resizer-punctuation = Noktalama
prefs-postproc-resizer-split-by-max-length = Azami uzunluğa göre böl
prefs-postproc-resizer-max-length = Azami altyazı uzunluğu
prefs-postproc-summary-title = Yapay Zekâ Özeti
prefs-postproc-summary-enable = Enable AI Summary
prefs-postproc-summary-api-url = API temel URL'si
prefs-postproc-summary-api-key-label = API anahtarı
prefs-postproc-summary-model = Model
prefs-postproc-summary-prompt = Özetleme istemi
prefs-postproc-summary-save-to-notes = Özeti Notlar'a kaydet
prefs-postproc-summary-save-to-file = Özeti bir dosyaya kaydet
prefs-postproc-summary-output-folder = Çıktı klasörü (dosya için)
prefs-postproc-summary-key-save-error-body = Unable to save AI Summary API key to keyring
prefs-postproc-docx-title = DOCX Olarak Dışa Aktar
prefs-postproc-docx-enable = Enable Export to DOCX
prefs-postproc-docx-output-folder = Çıktı klasörü
prefs-postproc-docx-include-timestamps = Zaman damgalarını ekle
prefs-postproc-output-folder-hint = Kaynak dosyanın yanına kaydetmek için boş bırakın.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Görünüm
viewer-mode-text = Metin
viewer-mode-translation = Çeviri
viewer-mode-timestamps = Zaman Damgaları
viewer-export = Dışa Aktar
viewer-translate = Çevir
viewer-resize = Yeniden Boyutlandır
viewer-identify-speakers = Konuşmacıları Belirle
viewer-find = Bul
viewer-find-label = Bul:
viewer-find-placeholder = Aranacak metni girin...
viewer-find-clear = Temizle
viewer-find-prev-tooltip = Previous match ({ $shortcut })
viewer-find-next-tooltip = Next match ({ $shortcut })
viewer-empty = No segments — the input may be silence-only
viewer-open-error = Could not open the transcript
viewer-col-start = Başlangıç
viewer-col-end = Bitiş
viewer-col-text = Metin
viewer-col-translation = Çeviri
viewer-saved = edited · saved
viewer-unsaved-warning = An edit could not be saved — it is kept in memory but not written to disk
viewer-player-loading = Loading audio…
viewer-player-failed = Playback unavailable
viewer-playback-controls = Oynatma Denetimleri:
viewer-loop-segment = Bölümü Döngüye Al
viewer-loop-segment-tooltip = Deşifre bölümlerine tıklandığında döngüyü etkinleştir/devre dışı bırak
viewer-follow-audio = Sesi Takip Et
viewer-follow-audio-tooltip = Deşifrede geçerli ses konumunun takibini etkinleştir/devre dışı bırak. Etkinleştirildiğinde geçerli metne otomatik olarak kaydırır.
viewer-speed = Speed:
viewer-scroll-to-current = Geçerli Konuma Kaydır
viewer-scroll-to-current-tooltip = O an konuşulan metne kaydır

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = API Anahtarı Gerekli
viewer-translate-key-required-body = Lütfen tercihlerden OpenAI API anahtarını girin
viewer-translate-settings-title = AI Translation Settings

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Metin
viewer-export-translation = Çeviri
viewer-export-dialog-title = Dosyayı Kaydet
viewer-export-filter = Metin dosyaları
viewer-export-failed = The export could not be written — see the log for details
export-written = Written to { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Ayarlar
resizer-create-new = Yeni deşifre oluştur
resizer-extend-heading = Bitiş süresini uzat
resizer-extend-label = Bitişleri en fazla şu kadar uzat (saniye)
resizer-extend-button = Bitişleri uzat
resizer-resize-heading = Yeniden Boyutlandırma Seçenekleri
resizer-resize-label = İstenen altyazı uzunluğu
resizer-resize-button = Yeniden Boyutlandır
resizer-resize-disabled-tooltip = Yalnızca deşifre sırasında kelime düzeyinde zamanlamalar kapalıysa kullanılabilir
resizer-merge-heading = Birleştirme Seçenekleri
resizer-merge-by-gap = Boşluğa göre birleştir
resizer-split-by-punctuation = Noktalamaya göre böl
resizer-split-by-max-length = Azami uzunluğa göre böl
resizer-merge-button = Birleştir
resizer-merge-disabled-tooltip = Yalnızca deşifre sırasında kelime düzeyinde zamanlamalar açıksa kullanılabilir
resizer-merging = Merging…
resizer-merge-error = The merge could not run:
resizer-save-error = The changes could not be saved:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Adım 1: Konuşmacıları belirle
speaker-id-step2-heading = Adım 2: Konuşmacılara ad ver
speaker-id-ready-label = Konuşmacıları belirlemeye hazır
speaker-id-missing-audio-label = Ses dosyası bulunamadı
speaker-id-identify-button = Belirle
speaker-id-cancel-button = İptal
speaker-id-cancelling = İptal ediliyor...
speaker-id-decoding = Decoding audio…
speaker-id-downloading-models = Downloading models…
speaker-id-identifying = Identifying speakers…
speaker-id-play-sample = Örneği oynat
speaker-id-merge-checkbox = Konuşmacı cümlelerini birleştir
speaker-id-save-button = Kaydet
speaker-id-save-error = The changes could not be saved:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Canlı Kayıt
recording-model-load-note = Bir modelin ilk kullanımda yüklenmesi birkaç dakika sürebilir.
recording-advanced-button = Gelişmiş...
recording-microphone-label = Mikrofon:
recording-record-button = Kayıt
recording-stop-button = Durdur
recording-realtime-warning = { $size } transcribed at { $ratio }× realtime last run — live transcription may fall behind
recording-chunk-skipped-warning = A chunk failed and was skipped: { $detail }
recording-panel-placeholder = Başlamak için Kayıt düğmesine tıklayın...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Yapay zekâ çevirisi bekleniyor...
recording-model-downloading = Downloading model… { $percent }%
recording-model-downloading-indeterminate = Model indiriliyor
recording-model-loading = Loading model…
recording-error-prefix = Yeni bir kayıt başlatılırken hata oluştu:
recording-error-suffix = Lütfen ses aygıtlarınızı kontrol edin veya daha fazla bilgi için uygulama günlüklerine bakın.
recording-thread-crashed = The recording session ended unexpectedly. Please check the application logs for more information.
meter-average-volume = Ortalama ses düzeyi
meter-queue = Sıra

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Gelişmiş Ayarlar
recording-advanced-speech-heading = Konuşma tanıma ayarları
recording-advanced-initial-prompt-label = Başlangıç İstemi:
recording-advanced-translation-heading = Çeviri ayarları
recording-advanced-llm-enable = Yapay zekâ çevirisini etkinleştir
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Kayıt ayarları
recording-advanced-silence-threshold-label = Sessizlik eşiği:
recording-advanced-mode-label = Live recording mode:
recording-advanced-mode-append-below = Alta ekle
recording-advanced-mode-append-above = Üste ekle
recording-advanced-mode-append-and-correct = Ekle ve düzelt
recording-advanced-separator-label = Satır ayırıcı:
recording-advanced-step-label = Deşifre adımı:
recording-advanced-hide-unconfirmed = Onaylanmamışları gizle
recording-advanced-export-enable = Canlı kaydın dışa aktarılmasını etkinleştir
recording-advanced-export-folder-label = Dışa aktarma klasörü:
recording-advanced-export-browse = Gözat
recording-advanced-export-file-name-label = Dışa aktarma dosya adı:
recording-advanced-export-file-type-label = Dışa aktarma dosya türü:
recording-advanced-export-file-type-txt = Metin dosyası (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label = 
    Limit export entries
    (0 = export all):
recording-advanced-upload-url-label = Upload URL:
recording-advanced-ok = Tamam

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Show
presentation-show-tooltip = Yeni pencerede göster
presentation-text-size-label = Yazı Boyutu:
presentation-theme-label = Tema
presentation-theme-light = Açık
presentation-theme-dark = Koyu
presentation-theme-custom = Özel
presentation-text-color-label = Yazı Rengi
presentation-background-color-label = Arka Plan Rengi
presentation-fullscreen-button = Tam Ekran
presentation-window-title = Canlı Deşifre Sunumu

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Kopyala
recording-copy-tooltip = Deşifreyi panoya kopyala
recording-copy-copied = Kopyalandı!
recording-copy-nothing = Kopyalanacak bir şey yok!

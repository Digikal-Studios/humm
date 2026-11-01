## humm-gui — Portuguese (Brazil) (pt-BR) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b/M8 Task 14, design spec
## §7.2). Section comments stay in English on purpose: they are
## developer-facing notes about *where* a key renders, identical in every
## catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own pt_BR translation is used
## verbatim (`buzz/locale/pt_BR/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh, matching
## the register the Buzz-sourced strings already establish. Buzz's pt_BR po
## carries no formal/informal header marker but consistently uses the
## neutral Brazilian "você" register throughout its own translations (e.g.
## "Tem certeza que deseja excluir...", "Digite o novo nome:") — this
## catalog stays in that same register consistently, including for the
## fresh humm-only strings.
##
## Buzz's own pt_BR po translates the msgid "Ok" (its own relabelled dialog
## button text) as "Ok" (unchanged casing). Following the es/zh-CN/M8-Task-13
## precedent (Buzz's own lineage, not a translation choice made here), all
## four of clear-confirm-ok, dialog-ok, url-dialog-ok and recording-advanced-
## ok collapse to that same "Ok" — coincidentally matching en's own spelling
## for three of the four (dialog-ok is "OK" in en), which is incidental to
## this locale, not intent; the en catalog's Ok/OK casing split is English-
## source-specific Qt lineage and does not survive translation.
##
## Six Buzz msgids are known (from the M8 Task 13 batch) to sometimes carry
## two translation variants concatenated with no separator, an upstream Buzz
## artifact rather than a translation choice. Three of the six are affected
## in pt_BR's own po and are fixed here to the second (revised) variant
## alone: `prefs-general-force-cpu-tooltip` (doubles "Ative isso..." then
## "Marque isso..." with no separator — the second, revised imperative is
## kept), `resizer-resize-disabled-tooltip` and `resizer-merge-disabled-
## tooltip` (each doubles two genuinely different phrasings of "word-level
## timings", e.g. "tempos de nível de palavra estavam desativados" vs
## "tempos em nível de palavra foram desabilitados" — the second, revised
## half is kept in each case). The other three of the six are clean (single
## variant, no concatenation) in pt_BR's po and are used as-is: `recording-
## model-load-note`, `viewer-loop-segment-tooltip`, `prefs-general-key-save-
## error-body`.
##
## Two further Buzz msgids match humm strings exactly but carry a truly
## *empty* msgstr in pt_BR's po (untranslated upstream, gettext falls back
## silently): `prefs-postproc-skip-check-files-desc` and `prefs-postproc-
## skip-check-db-desc`. Both are translated fresh here rather than left
## blank — the same two keys M8 Task 13 found empty in all four of its own
## locales.
##
## Strings naming the product "Buzz" are re-pointed at "humm", matching the
## en catalog's own substitution. Product name "humm" is never translated.
## Technical terms Buzz itself leaves untranslated in pt (API, GPU, CPU,
## Whisper, URL, TXT/SRT/VTT) are left alone here too; "Backend" is likewise
## kept untranslated, matching de/es/it/ca/da/nl's own choice for the same
## term.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry; every catalog carries the *whole* set, so
## the language combo stays fully localized whichever locale is active.
## Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), in Buzz's own dict order.

locale-name-en-us = Inglês
locale-name-ca-es = Catalão
locale-name-da-dk = Dinamarquês
locale-name-nl = Holandês
locale-name-fr = Francês
locale-name-de-de = Alemão
locale-name-es-es = Espanhol
locale-name-it-it = Italiano
locale-name-ja-jp = Japonês
locale-name-lv-lv = Letão
locale-name-pl-pl = Polonês
locale-name-pt-br = Português (Brasil)
locale-name-ru = Russo
locale-name-tr-tr = Turco
locale-name-uk-ua = Ucraniano
locale-name-zh-cn = Chinês (Simplificado)
locale-name-zh-tw = Chinês (Tradicional)

## Empty-state hero (views/main_window.rs)

hero-drop = Solte a mídia aqui, ou %shortcut
hero-import-button = Importar arquivos…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Nome do Arquivo / URL
table-col-model = Modelo
table-col-task = Tarefa
table-col-status = Status
table-col-date-completed = Data de Conclusão
table-col-date-added = Data de Adição
table-col-notes = Notas

## Task table — header/row context menus

table-menu-reset-column-order = Redefinir Ordem das Colunas
table-menu-restart = Reiniciar Transcrição
table-menu-rename = Renomear
table-menu-notes = Adicionar/Editar Notas

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transcrever
task-translate = Traduzir para Inglês

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = isolando a voz

## Status words and notices added with the UX review fixes.
status-queued = Na fila
status-canceled = Cancelado
status-skipped = Ignorado
status-completed = Concluído
status-failed = Falhou
status-in-progress = Em andamento
status-eta = ETA
status-downloading-model = Baixando modelo
status-downloading-media = Baixando mídia
status-reducing-noise = reduzindo ruído
table-menu-show-error = Mostrar erro
toast-written-to = Gravado em
toast-postproc-written = Arquivos adicionais gravados
toast-postproc-errors = Uma etapa de pós-processamento relatou um erro. A transcrição continua concluída.
toast-reduction-skipped = A redução de ruído não foi executada; a transcrição continuou.
options-word-timestamps-tooltip = Redimensionar fica disponível depois somente se os tempos por palavra estavam desligados durante a transcrição. Mesclar fica disponível depois somente se estavam ligados.
options-model-will-download = Executar baixa este modelo primeiro. A janela continua utilizável.
prefs-postproc-dfn-runs-before = O DeepFilterNet roda antes da transcrição e substitui o áudio que o modelo ouve.
prefs-postproc-dfn-keep-name = Manter o arquivo sem ruído deixa um arquivo chamado <name>_DeepFilterNet3.wav ao lado da fonte.
prefs-general-export-template-tokens-intro = Os espaços dentro das chaves fazem parte de cada token.
prefs-general-export-template-model-type = model_type é sempre Whisper.cpp.
prefs-general-cookie-label = Arquivo de cookies
prefs-general-cookie-browse = Procurar…
prefs-general-cookie-caption = HUMM_DOWNLOAD_COOKIEFILE tem prioridade quando está definida. Deixe o campo vazio para limpá-lo.
prefs-saved = Salvo
prefs-save-failed = Não foi possível salvar as preferências:
viewer-saved-neighbor = uma deixa vizinha foi ajustada
viewer-timestamp-unparsed = O tempo anterior foi mantido. Aquilo não era um tempo.
viewer-timestamp-range = O tempo anterior foi mantido. O início deve ser anterior ao fim.
speaker-id-save-copies = Salvar copia esta transcrição para uma nova linha concluída. O original permanece como está.
speaker-id-merge-tooltip = Linhas consecutivas do mesmo falante são unidas.
speaker-id-takes-minutes = Isso pode levar vários minutos. Cancelar descarta a execução.
already-running = o humm já está em execução.
already-running-with-files = o humm já está em execução. Os arquivos passados a esta inicialização não foram abertos. Importe-os pela janela que já está aberta.
banner-buzz-imported = %n transcrições importadas do Buzz.
banner-buzz-import-failed = Não foi possível importar as transcrições do Buzz. O humm tentará de novo na próxima inicialização.

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Gravar
toolbar-new-file = Nova Transcrição de Arquivo
toolbar-new-url = Nova Transcrição de URL
toolbar-open-transcript = Abrir Transcrição
toolbar-open-transcript-tooltip = Selecione uma ou mais transcrições concluídas para abrir
toolbar-cancel = Cancelar Transcrição
toolbar-clear = Limpar Histórico

## Menu bar (views/main_window.rs, F-004)

menu-file = Arquivo
menu-file-import-file = Importar Arquivo…
menu-file-import-url = Importar URL…
menu-file-import-folder = Importar Pasta…
menu-help-title = Ajuda
menu-help-about = Sobre o humm
menu-help-docs = Ajuda
menu-help-docs-tooltip = Não há uma página de ajuda no aplicativo.
menu-help-preferences = Preferências…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Limpar Histórico
clear-confirm-body = Tem certeza que deseja excluir a(s) transcrição(ões) selecionada(s)? Esta ação não pode ser desfeita.
clear-confirm-ok = Ok
clear-confirm-cancel = Cancelar
clear-error = Não foi possível excluir a(s) transcrição(ões) selecionada(s)

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Renomear Transcrição
rename-label = Digite o novo nome:
rename-error = Não foi possível renomear
notes-title = Notas
notes-label = Digite algumas notas relevantes para esta transcrição:
notes-error = Não foi possível salvar as notas
dialog-ok = Ok
dialog-cancel = Cancelar

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transcrição concluída
toast-completed-suffix = arquivo(s) escrito(s)
toast-canceled = Transcrição cancelada
toast-detail-title = Falha na transcrição
toast-detail-open-log = Abrir pasta de logs
toast-detail-retry = Tentar novamente
toast-detail-close = Fechar
retry-error-unavailable = Não foi possível reiniciar: as opções salvas não estão disponíveis

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = O humm não conseguiu registrar atualizações do histórico — as exportações ainda foram concluídas; consulte o arquivo de log

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Solte para importar

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## Byte-identical to the queue's own `YTDLP_MISSING` message
## (`humm_core::queue`) — that constant isn't `pub`, so this key duplicates
## it verbatim rather than importing it; keep both in sync if either changes.

url-import-unavailable-tooltip = yt-dlp ausente — instale-o (ex.: `brew install yt-dlp`) ou defina HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-invalid is byte-exact with Buzz's `import_url_dialog.py`
## `QMessageBox.critical` text — pinned by a unit test. url-dialog-ok/
## url-dialog-cancel are deliberately "Ok"/"Cancel" (not "OK", see
## dialog-ok's own comment above) — Buzz relabels this dialog's
## QDialogButtonBox to exactly that casing (`import_url_dialog.py:25-29`),
## the same convention as clear-confirm-ok/clear-confirm-cancel.

url-dialog-title = Importar URL
url-dialog-label = URL:
url-dialog-placeholder = https://exemplo.com/audio.mp3
url-dialog-invalid = A URL inserida é inválida.
url-dialog-ok = Ok
url-dialog-cancel = Cancelar

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Modelo:
options-task-label = Tarefa:
options-language-label = Idioma:
language-detect = Detectar Idioma
options-word-timestamps = Tempos em nível de palavra
options-extract-speech = Extrair fala
options-extract-speech-missing-tooltip = demucs não encontrado — instale-o (ex.: `pipx install demucs`) para habilitar a extração de voz
options-export-label = Exportar:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = nenhuma exportação selecionada
options-advanced = Avançado
options-initial-prompt-label = Prompt Inicial:
options-hf-repo-label = Modelo do Hugging Face
options-run = Executar

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Chave API:
options-llm-heading = Configurações de tradução
options-llm-enable = Habilitar tradução por IA
options-llm-model-label = Modelo de IA:
options-llm-model-presets-button = Predefinições
options-llm-prompt-label = Instruções para a IA:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Observando { $name }
folderwatch-pill-paused = Pausado
folderwatch-pill-pause-tip = Pausar observação da pasta
folderwatch-pill-resume-tip = Retomar observação da pasta

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Atualização Disponível
update-dialog-title = Atualização Disponível
update-dialog-header = Uma nova versão do humm está disponível!
update-dialog-current-version = Versão atual:
update-dialog-new-version = Nova versão:
update-dialog-release-notes = Notas de Versão:
update-dialog-open-release-page = Abrir página de lançamento
update-dialog-close = Fechar

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Versão
about-check-button = Verificar atualizações
about-show-logs-button = Mostrar logs
about-close-button = Fechar
about-up-to-date = Você está atualizado!
about-check-failed = Falha ao verificar atualizações — consulte o arquivo de log

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Preferências
prefs-tab-general = Geral
prefs-tab-models = Modelos
prefs-tab-shortcuts = Atalhos
prefs-tab-folder-watch = Monitorar Pasta
prefs-tab-postproc = Pós-processamento
prefs-tab-privacy = Privacidade

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Idioma da Interface
prefs-general-font-size-label = Tamanho da Fonte
prefs-general-font-size-custom-label = Tamanho de fonte personalizado
prefs-general-openai-key-label = Chave API da OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Mostrar
prefs-general-key-hide = Ocultar
prefs-general-test-button = Testar
prefs-general-test-title = Teste da Chave API OpenAI
prefs-general-test-success-body = Sua chave API é válida. O humm usará esta chave para realizar transcrições API Whisper e traduções de IA.
prefs-general-invalid-key-title = Chave API inválida
prefs-general-invalid-key-body = A API suporta apenas caracteres base64 (A-Za-z0-9+/=_-). Outros caracteres na chave API podem causar erros.
prefs-general-key-save-error-title = Erro
prefs-general-key-save-error-body = Não foi possível salvar a chave da API OpenAI no cofre de chaves
prefs-general-base-url-label = URL base da OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Modelo de API da OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Nome padrão do arquivo de exportação
prefs-general-force-cpu-row-label = Desabilitar GPU
prefs-general-force-cpu-label = Usar somente a CPU e desabilitar aceleração por GPU
prefs-general-force-cpu-tooltip = Marque isso se modelos maiores não couberem na memória da GPU e o humm travar

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Enviar telemetria anônima de inicialização
privacy-telemetry-disabled-caption = Esta compilação tem a telemetria desabilitada em tempo de compilação.
privacy-telemetry-items-intro = Cada evento de inicialização inclui:
privacy-telemetry-item-app-version = Versão do aplicativo
privacy-telemetry-item-system-name = Nome do sistema
privacy-telemetry-item-os-version = Versão do SO
privacy-telemetry-item-kernel-release = Versão do kernel
privacy-telemetry-item-cpu-architecture = Arquitetura da CPU
privacy-telemetry-item-system-locale = Localidade do sistema
privacy-telemetry-item-identifier = Um identificador anônimo aleatório
privacy-update-checkbox = Verificar atualizações automaticamente

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Baixado
models-section-available = Disponível para Download
models-download-button = Baixar
models-delete-button = Excluir
models-show-location-button = Mostrar local do arquivo
models-cancel-button = Cancelar
models-custom-url-label = URL personalizada do modelo ggml:
models-delete-confirm-title = Excluir Modelo
models-delete-confirm-body = Tem certeza que deseja excluir o modelo selecionado?
models-download-error-title = Erro
models-download-failed = Falha ao baixar
models-speed-badge = { $ratio }× tempo real
models-speed-badge-tip = Velocidade de tempo real medida a partir das sessões de gravação ao vivo desta máquina

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Abrir Janela de Gravação
shortcut-desc-import-file = Importar Arquivo
shortcut-desc-import-url = Importar URL
shortcut-desc-open-preferences-window = Abrir Janela de Preferências
shortcut-desc-view-transcript-text = Ver Texto da Transcrição
shortcut-desc-view-transcript-translation = Ver Tradução da Transcrição
shortcut-desc-view-transcript-timestamps = Ver Marcações de Tempo da Transcrição
shortcut-desc-search-transcript = Pesquisar Transcrição
shortcut-desc-search-next = Ir para o Próximo Resultado de Pesquisa na Transcrição
shortcut-desc-search-previous = Ir para o Resultado de Pesquisa Anterior na Transcrição
shortcut-desc-scroll-to-current-text = Role até o Texto Atual
shortcut-desc-play-pause-audio = Tocar/Pausar o Áudio
shortcut-desc-replay-current-segment = Repetir o Segmento Atual
shortcut-desc-toggle-playback-controls = Alternar Controles de Reprodução
shortcut-desc-decrease-segment-start = Diminuir o Inicio do Segmento
shortcut-desc-increase-segment-start = Aumentar o Início do Segmento
shortcut-desc-decrease-segment-end = Diminuir o Final do Segmento
shortcut-desc-increase-segment-end = Estender o Final do Segmento
shortcut-desc-clear-history = Limpar Histórico
shortcut-desc-stop-transcription = Cancelar Transcrição
prefs-shortcuts-press = Pressione o atalho…
prefs-shortcuts-clear-tooltip = Redefinir para o padrão
prefs-shortcuts-conflict = Conflita com { $other }
prefs-shortcuts-reset = Redefinir para o Padrão

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Habilitar monitoramento de pasta
prefs-fw-input-folder-label = Pasta de entrada
prefs-fw-input-folder-placeholder = /caminho/para/a/pasta/de/entrada
prefs-fw-output-folder-label = Pasta de saída
prefs-fw-output-folder-placeholder = /caminho/para/a/pasta/de/saída
prefs-fw-browse = Procurar
prefs-fw-delete-checkbox = Excluir arquivos processados
prefs-fw-disposition-note = Uma vez transcrito, o arquivo de origem é movido para a pasta de saída — ou excluído, se Excluir arquivos processados estiver marcado.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Ignorar, a redução de ruído e a detecção de idioma rodam antes da transcrição. O redimensionamento, o resumo e a exportação DOCX rodam depois. Uma falha é registrada e nunca faz a transcrição falhar.
prefs-postproc-skip-title = Pular já transcritos
prefs-postproc-skip-enable = Ativar Pular já transcritos
prefs-postproc-skip-check-files = Verificar arquivos de resultado existentes
prefs-postproc-skip-check-files-desc = Pular se um arquivo .txt, .srt ou .vtt correspondente ao nome do arquivo de áudio for encontrado ao lado dele
prefs-postproc-skip-check-db = Verificar no banco de dados de transcrições
prefs-postproc-skip-check-db-desc = Pular se este arquivo já foi transcrito e o registro existe no banco de dados
prefs-postproc-dfn-title = Redução de ruído DeepFilterNet
prefs-postproc-dfn-enable = Ativar Redução de Ruído DeepFilterNet
prefs-postproc-dfn-keep-file = Manter arquivo sem ruído após a transcrição
prefs-postproc-langdetect-title = Detecção de idioma aprimorada
prefs-postproc-langdetect-enable = Ativar Detecção Aprimorada de Idioma
prefs-postproc-langdetect-download-tiny = Baixar o modelo tiny se nenhum estiver disponível
prefs-postproc-langdetect-download-tiny-desc = Se nenhum modelo whisper.cpp estiver baixado, baixa o modelo tiny para usar na detecção de idioma.
prefs-postproc-resizer-title = Redimensionador automático de transcrição
prefs-postproc-resizer-enable = Ativar Redimensionador Automático de Transcrição
prefs-postproc-resizer-merge-by-gap = Mesclar por intervalo
prefs-postproc-resizer-merge-gap-seconds = Intervalo de mesclagem (segundos)
prefs-postproc-resizer-split-by-punctuation = Dividir por pontuação
prefs-postproc-resizer-punctuation = Pontuação
prefs-postproc-resizer-split-by-max-length = Dividir por comprimento máximo
prefs-postproc-resizer-max-length = Comprimento máximo da legenda
prefs-postproc-summary-title = Resumo com IA
prefs-postproc-summary-enable = Ativar Resumo de IA
prefs-postproc-summary-api-url = URL base da API
prefs-postproc-summary-api-key-label = Chave de API
prefs-postproc-summary-model = Modelo
prefs-postproc-summary-prompt = Prompt de resumo
prefs-postproc-summary-save-to-notes = Salvar resumo nas notas
prefs-postproc-summary-save-to-file = Salvar resumo em um arquivo
prefs-postproc-summary-output-folder = Pasta de saída (para o arquivo)
prefs-postproc-summary-key-save-error-body = Não foi possível salvar a chave da API de Resumo de IA no cofre de chaves
prefs-postproc-docx-title = Exportar para DOCX
prefs-postproc-docx-enable = Ativar Exportação para DOCX
prefs-postproc-docx-output-folder = Pasta de saída
prefs-postproc-docx-include-timestamps = Incluir marcações de tempo
prefs-postproc-output-folder-hint = Deixe em branco para salvar ao lado do arquivo de origem.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Visualizar
viewer-mode-text = Texto
viewer-mode-translation = Tradução
viewer-mode-timestamps = Marcações de tempo
viewer-export = Exportar
viewer-translate = Traduzir
viewer-resize = Redimensionar
viewer-identify-speakers = Identificar Falantes
viewer-find = Procurar
viewer-find-label = Procurar:
viewer-find-placeholder = Digite o texto a procurar...
viewer-find-clear = Limpar
viewer-find-prev-tooltip = Correspondência anterior ({ $shortcut })
viewer-find-next-tooltip = Próxima correspondência ({ $shortcut })
viewer-empty = Nenhum segmento — a entrada pode ser apenas silêncio
viewer-open-error = Não foi possível abrir a transcrição
viewer-col-start = Início
viewer-col-end = Fim
viewer-col-text = Texto
viewer-col-translation = Tradução
viewer-saved = editado · salvo
viewer-unsaved-warning = Uma edição não pôde ser salva — ela é mantida na memória, mas não foi gravada no disco
viewer-player-loading = Carregando áudio…
viewer-player-failed = Reprodução indisponível
viewer-playback-controls = Controles de Reprodução:
viewer-loop-segment = Segmento de Loop
viewer-loop-segment-tooltip = Habilitar/desabilitar loop ao clicar em segmentos de transcrição
viewer-follow-audio = Siga o Áudio
viewer-follow-audio-tooltip = Ativar/desativar a opção de seguir a posição atual do áudio na transcrição. Quando ativado, rola automaticamente para o texto atual.
viewer-speed = Velocidade:
viewer-scroll-to-current = Rolar para o Atual
viewer-scroll-to-current-tooltip = Role até o texto falado no momento

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Chave API Necessária
viewer-translate-key-required-body = Insira a chave API OpenAI nas preferências
viewer-translate-settings-title = Configurações de Tradução por IA

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Texto
viewer-export-translation = Tradução
viewer-export-dialog-title = Salvar Arquivo
viewer-export-filter = Arquivos de texto
viewer-export-failed = A exportação não pôde ser gravada — veja o log para detalhes
export-written = Gravado em { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Configurações
resizer-create-new = Criar nova transcrição
resizer-extend-heading = Estender o tempo final
resizer-extend-label = Estender finais em até (segundos)
resizer-extend-button = Estender finais
resizer-resize-heading = Opções de Redimensionamento
resizer-resize-label = Duração desejada da legenda
resizer-resize-button = Redimensionar
resizer-resize-disabled-tooltip = Disponível apenas se os tempos em nível de palavra foram desabilitados durante a transcrição
resizer-merge-heading = Opções de Mesclagem
resizer-merge-by-gap = Mesclar por intervalo
resizer-split-by-punctuation = Dividir por pontuação
resizer-split-by-max-length = Dividir por comprimento máximo
resizer-merge-button = Mesclar
resizer-merge-disabled-tooltip = Disponível apenas se os tempos em nível de palavra foram habilitados durante a transcrição
resizer-merging = Mesclando…
resizer-merge-error = A mesclagem não pôde ser executada:
resizer-save-error = As alterações não puderam ser salvas:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
##
## Step-heading/button/checkbox strings are byte-exact ports of Buzz's
## `speaker_identification_widget.py` (`:490,502,506,515,517,543,558,573,578,
## 620`). The three progress-stage labels below the button strings are humm's
## own (Buzz's numbered "N/8 ..." steps are not ported -- design spec §6/this
## milestone's task brief); speaker-id-save-error has no Buzz equivalent
## either (Buzz's own save path cannot fail audibly at all -- UX_SPEC's
## "never silent" terminal-state rule is why humm surfaces one).

speaker-id-step1-heading = Passo 1: Identificar falantes
speaker-id-step2-heading = Passo 2: Nomear falantes
speaker-id-ready-label = Pronto para identificar falantes
speaker-id-missing-audio-label = Arquivo de áudio não encontrado
speaker-id-identify-button = Identificar
speaker-id-cancel-button = Cancelar
speaker-id-cancelling = Cancelando...
speaker-id-decoding = Decodificando áudio…
speaker-id-downloading-models = Baixando modelos…
speaker-id-identifying = Identificando locutores…
speaker-id-play-sample = Reproduzir amostra
speaker-id-merge-checkbox = Mesclar frases do falante
speaker-id-save-button = Salvar
speaker-id-save-error = As alterações não puderam ser salvas:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Gravação ao Vivo
recording-model-load-note = O primeiro uso de um modelo pode levar vários minutos para carregar.
recording-advanced-button = Avançado...
recording-microphone-label = Microfone:
recording-record-button = Gravar
recording-stop-button = Parar
recording-realtime-warning = { $size } transcrito a { $ratio }× tempo real na última execução — a transcrição ao vivo pode ficar atrasada
recording-chunk-skipped-warning = Um trecho falhou e foi ignorado: { $detail }
recording-panel-placeholder = Clique em Gravar para começar...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = Aguardando tradução da IA...
recording-model-downloading = Baixando modelo… { $percent }%
recording-model-downloading-indeterminate = Baixando modelo…
recording-model-loading = Carregando modelo…
recording-error-prefix = Ocorreu um erro ao iniciar uma nova gravação:
recording-error-suffix = Verifique seus dispositivos de áudio ou os logs do aplicativo para mais informações.
recording-thread-crashed = A sessão de gravação terminou inesperadamente. Verifique os logs do aplicativo para mais informações.
meter-average-volume = Volume médio
meter-queue = Fila

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Configurações Avançadas
recording-advanced-speech-heading = Configurações de reconhecimento de fala
recording-advanced-initial-prompt-label = Prompt Inicial:
recording-advanced-translation-heading = Configurações de tradução
recording-advanced-llm-enable = Habilitar tradução por IA
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Configurações de gravação
recording-advanced-silence-threshold-label = Limiar de silêncio:
recording-advanced-mode-label = Modo de gravação ao vivo:
recording-advanced-mode-append-below = Acrescentar abaixo
recording-advanced-mode-append-above = Acrescentar acima
recording-advanced-mode-append-and-correct = Acrescentar e corrigir
recording-advanced-separator-label = Separador de linha:
recording-advanced-step-label = Etapa de transcrição:
recording-advanced-hide-unconfirmed = Ocultar não confirmado
recording-advanced-export-enable = Habilitar exportação de gravação ao vivo
recording-advanced-export-folder-label = Pasta de exportação:
recording-advanced-export-browse = Procurar
recording-advanced-export-file-name-label = Nome do arquivo de exportação:
recording-advanced-export-file-type-label = Tipo de arquivo de exportação:
recording-advanced-export-file-type-txt = Arquivo de texto (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Limitar entradas de exportação
    (0 = exportar tudo):
recording-advanced-upload-url-label = URL de upload:
recording-advanced-ok = Ok

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Mostrar
presentation-show-tooltip = Mostrar em nova janela
presentation-text-size-label = Tamanho do Texto:
presentation-theme-label = Tema
presentation-theme-light = Claro
presentation-theme-dark = Escuro
presentation-theme-custom = Personalizado
presentation-text-color-label = Cor do Texto
presentation-background-color-label = Cor de Fundo
presentation-fullscreen-button = Tela Cheia
presentation-window-title = Apresentação de Transcrição ao Vivo

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Copiar
recording-copy-tooltip = Copiar transcrição para a área de transferência
recording-copy-copied = Copiado!
recording-copy-nothing = Nada para copiar!

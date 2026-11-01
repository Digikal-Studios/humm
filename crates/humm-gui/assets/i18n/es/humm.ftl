## humm-gui — Spanish (es) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b, design spec §9.2). Section
## comments stay in English on purpose: they are developer-facing notes about
## *where* a key renders, identical in every catalog, not translatable copy.
##
## Provenance rule applied throughout: wherever a humm string matches or
## closely paraphrases a Buzz msgid, Buzz's own es_ES translation is used
## verbatim (`buzz/locale/es_ES/LC_MESSAGES/buzz.po`) so terminology and
## muscle memory carry over; everything else is translated fresh in a neutral
## register (usted, no regional vocabulary). Buzz's own es catalog mixes tú and
## usted string by string; verbatim Buzz strings keep whichever form Buzz used
## rather than being harmonized, so the Buzz lineage stays auditable.
##
## Two mechanical corrections to the Buzz source, both upstream artifacts
## rather than translation choices: (1) six Buzz msgids carry two merged
## translation variants concatenated with no separator (e.g.
## "…transcripciónActivar/desactivar la reproducción…") — 11 locale-key
## instances across Buzz's de and es catalogs, of which this catalog is
## affected by five (`prefs-general-force-cpu-tooltip`,
## `recording-model-load-note`, `viewer-loop-segment-tooltip`,
## `resizer-resize-disabled-tooltip`, `resizer-merge-disabled-tooltip`; es's
## own `prefs-general-key-save-error-body` is clean, unlike de's); each takes
## the second (revised) variant alone. (2) Strings naming the product "Buzz"
## are re-pointed at "humm", matching the en catalog's own substitution.
##
## Product name "humm" is never translated. Technical terms Buzz itself leaves
## untranslated in es (API, GPU, CPU, Whisper, URL, TXT/SRT/VTT) are left alone
## here too.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry. Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py:34-50), translated by Buzz's own po.

locale-name-en-us = Inglés
locale-name-ca-es = Catalán
locale-name-da-dk = Danés
locale-name-nl = Holandés
locale-name-fr = Francés
locale-name-de-de = Alemán
locale-name-es-es = Español
locale-name-it-it = Italiano
locale-name-ja-jp = Japonés
locale-name-lv-lv = Letón
locale-name-pl-pl = Polaco
locale-name-pt-br = Portugués (Brasil)
locale-name-ru = Ruso
locale-name-tr-tr = Turco
locale-name-uk-ua = Ucraniano
locale-name-zh-cn = Chino (simplificado)
locale-name-zh-tw = Chino (tradicional)

## Empty-state hero (views/main_window.rs)

hero-drop = Suelte archivos multimedia aquí, o %shortcut
hero-import-button = Importar archivos…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Nombre de archivo / URL
table-col-model = Modelo
table-col-task = Tarea
table-col-status = Estado
table-col-date-completed = Fecha de finalización
table-col-date-added = Fecha de adición
table-col-notes = Notas

## Task table — header/row context menus

table-menu-reset-column-order = Restablecer orden de columnas
table-menu-restart = Reiniciar transcripción
table-menu-rename = Renombrar
table-menu-notes = Añadir/Editar notas

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transcribir
task-translate = Traducir al Inglés

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition, not a Buzz string; translated fresh.

status-isolating-speech = Aislando la voz

## Status words and notices added with the UX review fixes.
status-queued = En cola
status-canceled = Cancelado
status-skipped = Omitido
status-completed = Completado
status-failed = Error
status-in-progress = En progreso
status-eta = ETA
status-downloading-model = Descargando modelo
status-downloading-media = Descargando medios
status-reducing-noise = reduciendo ruido
table-menu-show-error = Mostrar error
toast-written-to = Escrito en
toast-postproc-written = Archivos adicionales escritos
toast-postproc-errors = Un paso de posprocesado informó un error. La transcripción sigue completa.
toast-reduction-skipped = La reducción de ruido no se ejecutó; la transcripción continuó.
options-word-timestamps-tooltip = Redimensionar solo está disponible después si los tiempos por palabra estaban desactivados durante la transcripción. Combinar solo está disponible después si estaban activados.
options-model-will-download = Ejecutar descarga primero este modelo. La ventana sigue usable.
prefs-postproc-dfn-runs-before = DeepFilterNet se ejecuta antes de la transcripción y sustituye el audio que oye el modelo.
prefs-postproc-dfn-keep-name = Conservar el archivo sin ruido deja un archivo llamado <name>_DeepFilterNet3.wav junto a la fuente.
prefs-general-export-template-tokens-intro = Los espacios dentro de las llaves forman parte de cada token.
prefs-general-export-template-model-type = model_type es siempre Whisper.cpp.
prefs-general-cookie-label = Archivo de cookies
prefs-general-cookie-browse = Examinar…
prefs-general-cookie-caption = HUMM_DOWNLOAD_COOKIEFILE tiene prioridad cuando está definida. Deje el campo vacío para borrarlo.
prefs-saved = Guardado
prefs-save-failed = No se pudieron guardar las preferencias:
viewer-saved-neighbor = se ajustó una señal vecina
viewer-timestamp-unparsed = Se conservó el tiempo anterior. Eso no era un tiempo.
viewer-timestamp-range = Se conservó el tiempo anterior. El inicio debe ser anterior al final.
speaker-id-save-copies = Guardar copia esta transcripción en una fila completada nueva. El original se queda como está.
speaker-id-merge-tooltip = Las líneas consecutivas del mismo hablante se unen.
speaker-id-takes-minutes = Esto puede tardar varios minutos. Cancelar descarta la ejecución.
already-running = humm ya se está ejecutando.
already-running-with-files = humm ya se está ejecutando. Los archivos pasados a este inicio no se abrieron. Impórtelos desde la ventana que ya está abierta.
banner-buzz-imported = Se importaron %n transcripciones de Buzz.
banner-buzz-import-failed = No se pudieron importar las transcripciones de Buzz. humm lo intentará de nuevo la próxima vez que se inicie.

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Grabar
toolbar-new-file = Nueva transcripción de archivos
toolbar-new-url = Nueva transcripción de URL
toolbar-open-transcript = Abrir transcripción
toolbar-open-transcript-tooltip = Seleccione una o más transcripciones completadas para abrirlas
toolbar-cancel = Cancelar transcripción
toolbar-clear = Vaciar historial

## Menu bar (views/main_window.rs, F-004)

menu-file = Archivo
menu-file-import-file = Importar archivo…
menu-file-import-url = Importar URL…
menu-file-import-folder = Importar carpeta…
menu-help-title = Ayuda
menu-help-about = Acerca de humm
menu-help-docs = Ayuda
menu-help-docs-tooltip = No hay una página de ayuda en la aplicación.
menu-help-preferences = Preferencias…

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## Buzz's es po keeps the English "Ok" spelling for its relabelled dialog
## buttons, so clear-confirm-ok/url-dialog-ok/recording-advanced-ok/dialog-ok
## stay "Ok" here — Buzz's own es lineage, not untranslated English.

clear-confirm-title = Vaciar historial
clear-confirm-body = ¿Confirma que quiere eliminar las transcripciones seleccionadas? Esta acción no se puede deshacer.
clear-confirm-ok = Ok
clear-confirm-cancel = Cancelar
clear-error = No se pudieron eliminar las transcripciones seleccionadas

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Renombrar transcripción
rename-label = Introducir nuevo nombre:
rename-error = No se pudo renombrar
notes-title = Notas
notes-label = Introduzca algunas notas relevantes para esta transcripción:
notes-error = No se pudieron guardar las notas
dialog-ok = Ok
dialog-cancel = Cancelar

## Toasts (views/toasts.rs, app.rs, F-026)
##
## toast-completed-suffix renders after a count ("… — 3 archivo(s) escrito(s)"),
## so it must read as a noun phrase, not a sentence.

toast-completed = Transcripción completada
toast-completed-suffix = archivo(s) escrito(s)
toast-canceled = Transcripción cancelada
toast-detail-title = La transcripción falló
toast-detail-open-log = Abrir carpeta de registros
toast-detail-retry = Reintentar
toast-detail-close = Cerrar
retry-error-unavailable = No se pudo reiniciar: las opciones guardadas no están disponibles

## Store-health banner (views/main_window.rs, app.rs, F-004)

banner-store-health = humm no pudo registrar las actualizaciones del historial — las exportaciones se siguen completando; consulte el archivo de registro

## Drag-and-drop overlay (views/main_window.rs, F-021)

drop-overlay-text = Suelte para importar

## URL-import unavailable tooltip (views/main_window.rs, F-022)
##
## The binary name, the shell command and the env var are identifiers and stay
## verbatim; only the prose around them is translated.

url-import-unavailable-tooltip = falta yt-dlp — instálelo (p. ej. `brew install yt-dlp`) o defina HUMM_YTDLP

## URL-import dialog (views/url_dialog.rs, F-022)
##
## url-dialog-placeholder keeps Buzz's own es localization of the example host
## (ejemplo.com) — it is a hint, never a URL humm resolves.

url-dialog-title = URL de importación
url-dialog-label = URL:
url-dialog-placeholder = https://ejemplo.com/audio.mp3
url-dialog-invalid = La URL que has introducido no es válida.
url-dialog-ok = Ok
url-dialog-cancel = Cancelar

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Modelo:
options-task-label = Tarea:
options-language-label = Idioma:
language-detect = Detectar idioma
options-word-timestamps = Tiempos a nivel de palabra
options-extract-speech = Extraer voz
options-extract-speech-missing-tooltip = no se encontró demucs — instálelo (p. ej. `pipx install demucs`) para habilitar la extracción de voz
options-export-label = Exportar:
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = sin exportación seleccionada
options-advanced = Avanzado
options-initial-prompt-label = Indicación inicial:
options-hf-repo-label = Modelo de Hugging Face
options-run = Ejecutar

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2).
##
## "Backend" is standard Spanish technical usage and stays as-is, the same way
## Buzz's own es catalog keeps API/GPU/CPU.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Clave API:
options-llm-heading = Ajustes de traducción
options-llm-enable = Habilite la traducción con IA
options-llm-model-label = Modelo de IA:
options-llm-model-presets-button = Preajustes
options-llm-prompt-label = Instrucciones para la IA:

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Vigilando { $name }
folderwatch-pill-paused = En pausa
folderwatch-pill-pause-tip = Pausar la vigilancia de carpetas
folderwatch-pill-resume-tip = Reanudar la vigilancia de carpetas

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Actualización disponible
update-dialog-title = Actualización disponible
update-dialog-header = ¡Hay una nueva versión de humm disponible!
update-dialog-current-version = Versión actual:
update-dialog-new-version = Nueva versión:
update-dialog-release-notes = Notas de la versión:
update-dialog-open-release-page = Abrir página de la versión
update-dialog-close = Cerrar

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Versión
about-check-button = Buscar actualizaciones
about-show-logs-button = Mostrar registros
about-close-button = Cerrar
about-up-to-date = ¡Estás al día!
about-check-failed = La comprobación de actualizaciones falló — consulte el archivo de registro

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Preferencias
prefs-tab-general = General
prefs-tab-models = Modelos
prefs-tab-shortcuts = Atajos
prefs-tab-folder-watch = Vigilancia de carpetas
prefs-tab-postproc = Postprocesamiento
prefs-tab-privacy = Privacidad

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1)

prefs-general-ui-language-label = Lenguaje de interfaz de usuario
prefs-general-font-size-label = Tamaño de fuente
prefs-general-font-size-custom-label = Tamaño de fuente personalizado
prefs-general-openai-key-label = Clave API de OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Mostrar
prefs-general-key-hide = Ocultar
prefs-general-test-button = Prueba
prefs-general-test-title = Prueba de la clave API de OpenAI
prefs-general-test-success-body = Tu clave API es válida. humm usará esta clave para realizar transcripciones de la API de Whisper y traducciones de IA.
prefs-general-invalid-key-title = Clave API no válida
prefs-general-invalid-key-body = La API solo admite caracteres base64 (A-Za-z0-9+/=_-). Otros caracteres de la clave de API pueden causar errores.
prefs-general-key-save-error-title = Error
prefs-general-key-save-error-body = No se puede guardar la clave de la API de OpenAI en el llavero
prefs-general-base-url-label = URL base de OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Modelo de la API de OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Nombre de archivo de exportación predeterminado
prefs-general-force-cpu-row-label = Desactivar GPU
prefs-general-force-cpu-label = Usa solo CPU y desactiva la aceleración de GPU
prefs-general-force-cpu-tooltip = Configure esto si los modelos más grandes no se ajustan a la memoria de su GPU y humm se bloquea

## Privacy tab (views/preferences.rs, ADR-009, UX §J-11)

privacy-telemetry-checkbox = Enviar telemetría de inicio anónima
privacy-telemetry-disabled-caption = Esta compilación tiene la telemetría desactivada en tiempo de compilación.
privacy-telemetry-items-intro = Cada evento de inicio incluye:
privacy-telemetry-item-app-version = Versión de la aplicación
privacy-telemetry-item-system-name = Nombre del sistema
privacy-telemetry-item-os-version = Versión del sistema operativo
privacy-telemetry-item-kernel-release = Versión del núcleo
privacy-telemetry-item-cpu-architecture = Arquitectura de la CPU
privacy-telemetry-item-system-locale = Configuración regional del sistema
privacy-telemetry-item-identifier = Un identificador anónimo aleatorio
privacy-update-checkbox = Buscar actualizaciones automáticamente

## Models tab (models_ui.rs, F-044 basic)

models-section-downloaded = Descargado
models-section-available = Disponible para descarga
models-download-button = Descargar
models-delete-button = Eliminar
models-show-location-button = Mostrar ubicación de archivo
models-cancel-button = Cancelar
models-custom-url-label = URL personalizada del modelo ggml:
models-delete-confirm-title = Eliminar modelo
models-delete-confirm-body = ¿Confirma que quiere eliminar el modelo seleccionado?
models-download-error-title = Error
models-download-failed = Descarga fallida
models-speed-badge = { $ratio }× tiempo real
models-speed-badge-tip = Velocidad en tiempo real medida en las sesiones de grabación en vivo de este equipo

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `.po` translations for `shortcut.py`'s descriptions.

shortcut-desc-open-record-window = Abrir ventana de grabación
shortcut-desc-import-file = Importar archivo
shortcut-desc-import-url = URL de importación
shortcut-desc-open-preferences-window = Abrir ventana de preferencias
shortcut-desc-view-transcript-text = Ver el texto de la transcripción
shortcut-desc-view-transcript-translation = Ver la traducción de la transcripción
shortcut-desc-view-transcript-timestamps = Ver marcas de tiempo de la transcripción
shortcut-desc-search-transcript = Buscar transcripción
shortcut-desc-search-next = Ir al siguiente resultado de búsqueda de transcripción
shortcut-desc-search-previous = Ir al resultado de búsqueda de transcripción anterior
shortcut-desc-scroll-to-current-text = Desplazarse al texto actual
shortcut-desc-play-pause-audio = Reproducir/Pausar audio
shortcut-desc-replay-current-segment = Reproducir segmento actual
shortcut-desc-toggle-playback-controls = Alternar controles de reproducción
shortcut-desc-decrease-segment-start = Disminuir el tiempo de inicio del segmento
shortcut-desc-increase-segment-start = Aumentar el tiempo de inicio del segmento
shortcut-desc-decrease-segment-end = Disminuir el tiempo de finalización del segmento
shortcut-desc-increase-segment-end = Aumentar el tiempo de finalización del segmento
shortcut-desc-clear-history = Vaciar historial
shortcut-desc-stop-transcription = Cancelar transcripción
prefs-shortcuts-press = Presione una combinación de teclas…
prefs-shortcuts-clear-tooltip = Restablecer al valor por defecto
prefs-shortcuts-conflict = Entra en conflicto con { $other }
prefs-shortcuts-reset = Reestablecer los Valores por Defecto

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own es_ES translations
## (`folder_watch_preferences_widget.py`), used verbatim.
## `prefs-fw-disposition-note` is a ✚ humm-only addition (design spec §3.4,
## no Buzz source) -- translated fresh in this catalog's own register.

prefs-fw-enable = Habilitar la inspección de carpetas
prefs-fw-input-folder-label = Carpeta de entrada
prefs-fw-input-folder-placeholder = /ruta/a/la/carpeta/de/entrada
prefs-fw-output-folder-label = Carpeta de salida
prefs-fw-output-folder-placeholder = /ruta/a/la/carpeta/de/salida
prefs-fw-browse = Navegar
prefs-fw-delete-checkbox = Eliminar archivos procesados
prefs-fw-disposition-note = Una vez transcrito, el archivo de origen se mueve a la carpeta de salida — o se elimina, si Eliminar archivos procesados está marcado.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16). Section titles and field labels/descriptions are
## Buzz's own es_ES plugin locale JSONs (`plugins/*/locale/es_ES.json`), used
## verbatim -- Buzz's own translations. The four "Habilitar …" checkbox
## labels and `prefs-postproc-header`/`prefs-postproc-summary-key-save-error-
## body` have no Buzz source (Buzz's plugin enablement lived in the unported
## Plugins management dialog, ADR-010) -- translated fresh in this
## catalog's own register.

prefs-postproc-header = Omitir, la reducción de ruido y la detección de idioma se ejecutan antes de la transcripción. El redimensionado, el resumen y la exportación DOCX se ejecutan después. Un fallo se registra y nunca hace fallar la transcripción.
prefs-postproc-skip-title = Omitir ya transcritos
prefs-postproc-skip-enable = Habilitar Omitir ya transcritos
prefs-postproc-skip-check-files = Buscar archivos de resultado existentes
prefs-postproc-skip-check-files-desc = Omitir si se encuentra un archivo .txt, .srt o .vtt con el mismo nombre de audio junto a él
prefs-postproc-skip-check-db = Comprobar en la base de datos de transcripciones
prefs-postproc-skip-check-db-desc = Omitir si este archivo ya ha sido transcrito y el registro existe en la base de datos
prefs-postproc-dfn-title = Reducción de ruido DeepFilterNet
prefs-postproc-dfn-enable = Habilitar Reducción de ruido DeepFilterNet
prefs-postproc-dfn-keep-file = Conservar el archivo sin ruido tras la transcripción
prefs-postproc-langdetect-title = Detección de idioma mejorada
prefs-postproc-langdetect-enable = Habilitar detección de idioma mejorada
prefs-postproc-langdetect-download-tiny = Descargar el modelo tiny si no hay ninguno disponible
prefs-postproc-langdetect-download-tiny-desc = Si no hay ningún modelo de whisper.cpp descargado, descarga el modelo tiny para usarlo en la detección de idioma.
prefs-postproc-resizer-title = Redimensionador automático de transcripciones
prefs-postproc-resizer-enable = Habilitar el redimensionador automático de transcripciones
prefs-postproc-resizer-merge-by-gap = Fusionar por intervalo
prefs-postproc-resizer-merge-gap-seconds = Intervalo de fusión (segundos)
prefs-postproc-resizer-split-by-punctuation = Dividir por puntuación
prefs-postproc-resizer-punctuation = Puntuación
prefs-postproc-resizer-split-by-max-length = Dividir por longitud máxima
prefs-postproc-resizer-max-length = Longitud máxima del subtítulo
prefs-postproc-summary-title = Resumen con IA
prefs-postproc-summary-enable = Habilitar Resumen con IA
prefs-postproc-summary-api-url = URL base de la API
prefs-postproc-summary-api-key-label = Clave de API
prefs-postproc-summary-model = Modelo
prefs-postproc-summary-prompt = Indicación de resumen
prefs-postproc-summary-save-to-notes = Guardar resumen en notas
prefs-postproc-summary-save-to-file = Guardar resumen en un archivo
prefs-postproc-summary-output-folder = Carpeta de salida (para el archivo)
prefs-postproc-summary-key-save-error-body = No se puede guardar la clave de la API de Resumen con IA en el llavero
prefs-postproc-docx-title = Exportar a DOCX
prefs-postproc-docx-enable = Habilitar Exportar a DOCX
prefs-postproc-docx-output-folder = Carpeta de salida
prefs-postproc-docx-include-timestamps = Incluir marcas de tiempo
prefs-postproc-output-folder-hint = Déjalo vacío para guardar junto al archivo de origen.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Ver
viewer-mode-text = Texto
viewer-mode-translation = Traducción
viewer-mode-timestamps = Marcas de tiempo
viewer-export = Exportar
viewer-translate = Traducir
viewer-resize = Cambiar el tamaño
viewer-identify-speakers = Identificar hablantes
viewer-find = Buscar
viewer-find-label = Encontrar:
viewer-find-placeholder = Introducir texto para encontrar...
viewer-find-clear = Limpiar
viewer-find-prev-tooltip = Coincidencia anterior ({ $shortcut })
viewer-find-next-tooltip = Coincidencia siguiente ({ $shortcut })
viewer-empty = Sin segmentos — puede que la entrada sea solo silencio
viewer-open-error = No se pudo abrir la transcripción
viewer-col-start = Inicio
viewer-col-end = Fin
viewer-col-text = Texto
viewer-col-translation = Traducción
viewer-saved = editado · guardado
viewer-unsaved-warning = No se pudo guardar una edición — se conserva en memoria pero no se ha escrito en el disco
viewer-player-loading = Cargando audio…
viewer-player-failed = Reproducción no disponible
viewer-playback-controls = Controles de reproducción:
viewer-loop-segment = Segmento de bucle
viewer-loop-segment-tooltip = Activar/desactivar la reproducción en bucle al hacer clic en segmentos de la transcripción
viewer-follow-audio = Seguir audio
viewer-follow-audio-tooltip = Activa/desactiva el seguimiento de la posición actual del audio en la transcripción. Cuando está activado, se desplaza automáticamente al texto actual.
viewer-speed = Velocidad:
viewer-scroll-to-current = Desplácese hasta Actual
viewer-scroll-to-current-tooltip = Desplazarse hasta el texto hablado actualmente

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3)

viewer-translate-key-required-title = Clave de API requerida
viewer-translate-key-required-body = Ingrese la clave API de OpenAI en las preferencias
viewer-translate-settings-title = Ajustes de traducción con IA

## Export menu (views/export_menu.rs, F-084)

viewer-export-text = Texto
viewer-export-translation = Traducción
viewer-export-dialog-title = Guardar archivo
viewer-export-filter = Archivos de texto
viewer-export-failed = No se pudo escribir la exportación — consulte el registro para más detalles
export-written = Escrito en { $path }

## Transcript resizer window (views/resizer.rs, F-086)

resizer-settings = Configuración
resizer-create-new = Crear nueva transcripción
resizer-extend-heading = Extender tiempo final
resizer-extend-label = Extender finales hasta (segundos)
resizer-extend-button = Extender finales
resizer-resize-heading = Opciones de cambio de tamaño
resizer-resize-label = Longitud deseada de los subtítulos
resizer-resize-button = Cambiar el tamaño
resizer-resize-disabled-tooltip = Disponible solo si los tiempos a nivel de palabra estaban desactivados durante la transcripción
resizer-merge-heading = Opciones de fusión
resizer-merge-by-gap = Fusionar por intervalo
resizer-split-by-punctuation = Dividir por puntuación
resizer-split-by-max-length = Dividir por longitud máxima
resizer-merge-button = Fusión
resizer-merge-disabled-tooltip = Disponible solo si los tiempos a nivel de palabra estaban activados durante la transcripción
resizer-merging = Fusionando…
resizer-merge-error = No se pudo ejecutar la fusión:
resizer-save-error = No se pudieron guardar los cambios:

## Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)

speaker-id-step1-heading = Paso 1: Identificar hablantes
speaker-id-step2-heading = Paso 2: Nombrar hablantes
speaker-id-ready-label = Listo para identificar hablantes
speaker-id-missing-audio-label = Archivo de audio no encontrado
speaker-id-identify-button = Identificar
speaker-id-cancel-button = Cancelar
speaker-id-cancelling = Cancelando...
speaker-id-decoding = Decodificando audio…
speaker-id-downloading-models = Descargando modelos…
speaker-id-identifying = Identificando hablantes…
speaker-id-play-sample = Reproducir muestra
speaker-id-merge-checkbox = Fusionar frases del hablante
speaker-id-save-button = Guardar
speaker-id-save-error = No se pudieron guardar los cambios:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)

recording-window-title = Grabación en vivo
recording-model-load-note = El uso por primera vez de un modelo puede tardar varios minutos en cargarse.
recording-advanced-button = Avanzado...
recording-microphone-label = Micrófono:
recording-record-button = Grabar
recording-stop-button = Detener
recording-realtime-warning = { $size } se transcribió a { $ratio }× tiempo real en la última ejecución — la transcripción en vivo puede quedarse atrás
recording-chunk-skipped-warning = Un fragmento falló y se omitió: { $detail }
recording-panel-placeholder = Pulse en Grabar para comenzar...
recording-panel-translation-placeholder = A la espera de la traducción de la IA...
recording-model-downloading = Descargando modelo… { $percent } %
recording-model-downloading-indeterminate = Descargando modelo…
recording-model-loading = Cargando modelo…
recording-error-prefix = Se produjo un error al iniciar una grabación nueva:
recording-error-suffix = Compruebe sus dispositivos de audio o consulte los registros de la aplicación para obtener más información.
recording-thread-crashed = La sesión de grabación finalizó inesperadamente. Consulte los registros de la aplicación para obtener más información.
meter-average-volume = Volumen promedio
meter-queue = Cola

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060)

recording-advanced-title = Configuración avanzada
recording-advanced-speech-heading = Configuración de reconocimiento de voz
recording-advanced-initial-prompt-label = Indicación inicial:
recording-advanced-translation-heading = Ajustes de traducción
recording-advanced-llm-enable = Habilite la traducción con IA
recording-advanced-recording-heading = Ajustes de grabación
recording-advanced-silence-threshold-label = Umbral de silencio:
recording-advanced-mode-label = Modo de grabación en directo:
recording-advanced-mode-append-below = Añadir a continuación
recording-advanced-mode-append-above = Añadir arriba
recording-advanced-mode-append-and-correct = Añadir y corregir
recording-advanced-separator-label = Separador de línea:
recording-advanced-step-label = Paso de transcripción:
recording-advanced-hide-unconfirmed = Ocultar no confirmado
recording-advanced-export-enable = Habilitar la exportación de grabación en vivo
recording-advanced-export-folder-label = Carpeta de exportación:
recording-advanced-export-browse = Navegar
recording-advanced-export-file-name-label = Nombre del archivo de exportación:
recording-advanced-export-file-type-label = Tipo de archivo de exportación:
recording-advanced-export-file-type-txt = Archivo de texto (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label =
    Limitar entradas de exportación
    (0 = exportar todo):
recording-advanced-upload-url-label = URL de subida:
recording-advanced-ok = Ok

## Presentation window + bar (views/presentation.rs, F-065)

presentation-show-button = Mostrar
presentation-show-tooltip = Mostrar en nueva ventana
presentation-text-size-label = Tamaño del texto:
presentation-theme-label = Tema
presentation-theme-light = Claro
presentation-theme-dark = Oscuro
presentation-theme-custom = Personalizado
presentation-text-color-label = Color del texto
presentation-background-color-label = Color de fondo
presentation-fullscreen-button = Pantalla completa
presentation-window-title = Presentación de transcripción en vivo

## Copy bar (views/recording.rs, F-065 §2.7)

recording-copy-button = Copiar
recording-copy-tooltip = Copiar transcripción al portapapeles
recording-copy-copied = ¡Copiado!
recording-copy-nothing = ¡Nada que copiar!

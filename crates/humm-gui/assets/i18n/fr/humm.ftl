## humm-gui — French (fr) catalog.
##
## Key-for-key port of `../en/humm.ftl` (F-008b, design spec §9.2). Section
## comments stay in English on purpose: they are developer-facing notes about
## *where* a key renders, identical in every catalog, not translatable copy.
##
## Provenance: wherever a humm string matches or closely paraphrases a Buzz
## msgid, Buzz's own `fr` translation is used verbatim
## (`buzz/locale/fr/LC_MESSAGES/buzz.po` at Buzz `298dbf0`); product name
## "Buzz" is re-pointed at "humm". humm-only strings with no Buzz msgid keep
## the English source value until a dedicated pass fills them.
##
## Product name "humm" is never translated.

## Locale display names (i18n::display_name, F-008a) -- one key per
## i18n::AVAILABLE_LOCALES entry. Source: Buzz's `ui_locales` dict values
## (general_preferences_widget.py), translated by Buzz's own po.

locale-name-en-us = Anglais
locale-name-ca-es = Catalan
locale-name-da-dk = Danois
locale-name-nl = Néerlandais
locale-name-fr = Français
locale-name-de-de = Allemand
locale-name-es-es = Espagnol
locale-name-it-it = Italien
locale-name-ja-jp = Japonais
locale-name-lv-lv = Letton
locale-name-pl-pl = Polonais
locale-name-pt-br = Portugais (Brésil)
locale-name-ru = Russe
locale-name-tr-tr = Turc
locale-name-uk-ua = Ukrainien
locale-name-zh-cn = Chinois (simplifié)
locale-name-zh-tw = Chinois (traditionnel)

## Empty-state hero (views/main_window.rs)

hero-drop = Drop media here, or %shortcut
hero-import-button = Import files…

## Task table (views/task_table.rs, F-003) — column headers

table-col-file-name = Nom du fichier / URL
table-col-model = Modèle
table-col-task = Tâche
table-col-status = Statut
table-col-date-completed = Date d'achèvement
table-col-date-added = Date d'ajout
table-col-notes = Notes

## Task table — header/row context menus

table-menu-reset-column-order = Réinitialiser l'ordre des colonnes
table-menu-restart = Redémarrer la transcription
table-menu-rename = Renommer
table-menu-notes = Ajouter/Modifier les notes

## Task table — Task column labels (viewmodel.rs)

task-transcribe = Transcrire
task-translate = Traduire en anglais

## Task table — status-column phase text (format.rs, F-027, M8) -- a humm
## addition (design spec UX_SPEC.md's ✚ phase list), not a Buzz string, so
## unlike this file's other status texts it goes through `tr` rather than
## staying a literal byte-exact Buzz string.

status-isolating-speech = isolating speech

## Status words and notices added with the UX review fixes.
status-queued = En file
status-canceled = Annulé
status-skipped = Ignoré
status-completed = Terminé
status-failed = Échec
status-in-progress = En cours
status-eta = ETA
status-downloading-model = Téléchargement du modèle
status-downloading-media = Téléchargement du média
status-reducing-noise = réduction du bruit
table-menu-show-error = Afficher l'erreur
toast-written-to = Écrit dans
toast-postproc-written = Fichiers supplémentaires écrits
toast-postproc-errors = Une étape de post-traitement a signalé une erreur. La transcription est tout de même terminée.
toast-reduction-skipped = La réduction du bruit n'a pas été exécutée ; la transcription a continué.
options-word-timestamps-tooltip = Redimensionner n'est disponible ensuite que si les minutages par mot étaient désactivés pendant la transcription. Fusionner n'est disponible ensuite que s'ils étaient activés.
options-model-will-download = Exécuter télécharge d'abord ce modèle. La fenêtre reste utilisable.
prefs-postproc-dfn-runs-before = DeepFilterNet s'exécute avant la transcription et remplace l'audio que le modèle entend.
prefs-postproc-dfn-keep-name = Conserver le fichier débruité laisse un fichier nommé <name>_DeepFilterNet3.wav à côté de la source.
prefs-general-export-template-tokens-intro = Les espaces à l'intérieur des accolades font partie de chaque jeton.
prefs-general-export-template-model-type = model_type vaut toujours Whisper.cpp.
prefs-general-cookie-label = Fichier de cookies
prefs-general-cookie-browse = Parcourir…
prefs-general-cookie-caption = HUMM_DOWNLOAD_COOKIEFILE l'emporte sur ce champ lorsqu'elle est définie. Laissez le champ vide pour l'effacer.
prefs-saved = Enregistré
prefs-save-failed = Impossible d'enregistrer les préférences :
viewer-saved-neighbor = un repère voisin a été ajusté
viewer-timestamp-unparsed = L'heure précédente a été conservée. Ce n'était pas une heure.
viewer-timestamp-range = L'heure précédente a été conservée. Le début doit précéder la fin.
speaker-id-save-copies = Enregistrer copie cette transcription dans une nouvelle ligne terminée. L'original reste tel quel.
speaker-id-merge-tooltip = Les lignes consécutives du même locuteur sont réunies.
speaker-id-takes-minutes = Cela peut prendre plusieurs minutes. Annuler abandonne l'exécution.
already-running = humm est déjà en cours d'exécution.
already-running-with-files = humm est déjà en cours d'exécution. Les fichiers passés à ce lancement n'ont pas été ouverts. Importez-les depuis la fenêtre déjà ouverte.
banner-buzz-imported = %n transcriptions importées depuis Buzz.
banner-buzz-import-failed = Impossible d'importer les transcriptions depuis Buzz. humm réessaiera au prochain démarrage.

## Toolbar (views/main_window.rs, F-004; Record unlocked F-004/F-060, Task 10)

toolbar-record = Enregistrer
toolbar-new-file = Nouvelle transcription de fichier
toolbar-new-url = Nouvelle transcription d'URL
toolbar-open-transcript = Ouvrir une transcription
toolbar-open-transcript-tooltip = Select one or more completed transcriptions to open
toolbar-cancel = Annuler la transcription
toolbar-clear = Effacer l'historique

## Menu bar (views/main_window.rs, F-004)

menu-file = Fichier
menu-file-import-file = Importer un fichier...
menu-file-import-url = Importer une URL...
menu-file-import-folder = Importer un dossier...
menu-help-title = Aide
menu-help-about = About humm
menu-help-docs = Aide
menu-help-docs-tooltip = Il n'y a pas de page d'aide dans l'application.
menu-help-preferences = Préférences...

## Clear-History confirm modal (views/main_window.rs, F-026)
##
## clear-confirm-ok is deliberately "Ok" (lowercase k), not "OK" — Buzz's
## QMessageBox.button(...).setText(_("Ok")) literally relabels its Yes/No
## buttons to that exact casing (main_window.py:249-250). dialog-ok below,
## for the Rename/Notes modals, is a *different* dialog kind (Qt's
## QInputDialog, whose own default OK button is genuinely spelled "OK") —
## the casing difference between the two is intentional Buzz lineage, not
## a typo to "fix" into consistency.

clear-confirm-title = Effacer l'historique
clear-confirm-body = Voulez-vous vraiment supprimer la/les transcription(s) sélectionnée(s) ? Cette action est irréversible.
clear-confirm-ok = Ok
clear-confirm-cancel = Annuler
clear-error = Could not delete the selected transcription(s)

## Rename/Notes modals (views/rename_notes.rs, F-026)

rename-title = Renommer la transcription
rename-label = Entrez un nouveau nom :
rename-error = Could not rename
notes-title = Notes
notes-label = Saisissez des notes pertinentes pour cette transcription :
notes-error = Could not save notes
dialog-ok = Ok
dialog-cancel = Annuler

## Toasts (views/toasts.rs, app.rs, F-026)

toast-completed = Transcription complete
toast-completed-suffix = file(s) written
toast-canceled = Transcription canceled
toast-detail-title = Transcription Failed
toast-detail-open-log = Open log folder
toast-detail-retry = Retry
toast-detail-close = Fermer
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

url-dialog-title = Importer une URL
url-dialog-label = URL :
url-dialog-placeholder = https://exemple.com/audio.mp3
url-dialog-invalid = L'URL que vous avez saisie est invalide.
url-dialog-ok = Ok
url-dialog-cancel = Annuler

## Options sheet (views/options_sheet.rs, F-023)

options-model-label = Modèle :
options-task-label = Tâche :
options-language-label = Langue :
language-detect = Détection automatique
options-word-timestamps = Horodatages par mot
options-extract-speech = Extraire la parole
options-extract-speech-missing-tooltip = demucs not found — install it (e.g. `pipx install demucs`) to enable speech extraction
options-export-label = Exporter :
options-export-txt = TXT
options-export-srt = SRT
options-export-vtt = VTT
options-summary-no-formats = no export selected
options-advanced = Avancé...
options-initial-prompt-label = Invite initiale :
options-hf-repo-label = Hugging Face model
options-run = Exécuter

## Options sheet -- backend selector + Api Key row + LLM translation block
## (F-023 extension, F-041, F-031, Task 14, design spec §7.2). The
## invalid-charset/keyring-save-error modal reuses the
## prefs-general-invalid-key-*/prefs-general-key-save-error-* keys above --
## same underlying keyring entry, same Buzz-exact wording either way.

options-backend-label = Backend:
options-backend-whisper = Whisper
options-backend-openai-api = OpenAI Whisper API
options-api-key-label = Clé API :
options-llm-heading = Paramètres de traduction
options-llm-enable = Activer la traduction IA
options-llm-model-label = Modèle IA :
options-llm-model-presets-button = Presets
options-llm-prompt-label = Instructions pour l'IA :

## Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)

folderwatch-pill-watching = Watching { $name }
folderwatch-pill-paused = Paused
folderwatch-pill-pause-tip = Pause folder watching
folderwatch-pill-resume-tip = Resume folder watching

## Update chip + dialog (views/update_notice.rs, F-006, ADR-015)

update-chip-label = Mise à jour disponible
update-dialog-title = Mise à jour disponible
update-dialog-header = Une nouvelle version de humm est disponible !
update-dialog-current-version = Version actuelle :
update-dialog-new-version = Nouvelle version :
update-dialog-release-notes = Notes de version :
update-dialog-open-release-page = Open release page
update-dialog-close = Fermer

## About window (views/about.rs, F-006 gui-manual half)

about-app-name = humm
about-version-prefix = Version
about-check-button = Vérifier les mises à jour
about-show-logs-button = Afficher les journaux
about-close-button = Fermer
about-up-to-date = Vous êtes à jour !
about-check-failed = Update check failed — see the log file

## Preferences window shell (views/preferences.rs, F-044/ADR-009)

prefs-title = Préférences
prefs-tab-general = Général
prefs-tab-models = Modèles
prefs-tab-shortcuts = Raccourcis
prefs-tab-folder-watch = Surveillance de dossier
prefs-tab-postproc = Post-processing
prefs-tab-privacy = Privacy

## Preferences General tab, complete (views/prefs_general.rs, F-100,
## design spec §8.1) -- Buzz row order: UI language, font size, the OpenAI
## block, default export file name, Disable GPU.

prefs-general-ui-language-label = Langue de l'interface
prefs-general-font-size-label = Taille de la police
prefs-general-font-size-custom-label = Custom font size
prefs-general-openai-key-label = Clé API OpenAI
prefs-general-openai-key-placeholder = sk-...
prefs-general-key-show = Show
prefs-general-key-hide = Hide
prefs-general-test-button = Tester
prefs-general-test-title = Test de la clé API OpenAI
prefs-general-test-success-body = Votre clé API est valide. humm utilisera cette clé pour les transcriptions via l'API Whisper et les traductions IA.
prefs-general-invalid-key-title = Clé API invalide
prefs-general-invalid-key-body = L'API ne supporte que les caractères base64 (A-Za-z0-9+/=_-). D'autres caractères dans la clé API peuvent causer des erreurs.
prefs-general-key-save-error-title = Erreur
prefs-general-key-save-error-body = Impossible de sauvegarder la clé API OpenAI dans le trousseau
prefs-general-base-url-label = URL de base OpenAI
prefs-general-base-url-placeholder = https://api.openai.com/v1
prefs-general-model-label = Modèle API OpenAI
prefs-general-model-placeholder = whisper-1
prefs-general-export-template-label = Nom de fichier d'exportation par défaut
prefs-general-force-cpu-row-label = Désactiver le GPU
prefs-general-force-cpu-label = Utiliser uniquement le CPU et désactiver l'accélération GPU
prefs-general-force-cpu-tooltip = Activez cette option si les modèles plus gros ne tiennent pas dans la mémoire GPU et que humm plante

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

models-section-downloaded = Téléchargé
models-section-available = Disponible au téléchargement
models-download-button = Télécharger
models-delete-button = Supprimer
models-show-location-button = Afficher l'emplacement du fichier
models-cancel-button = Annuler
models-custom-url-label = Custom ggml model URL:
models-delete-confirm-title = Supprimer le modèle
models-delete-confirm-body = Voulez-vous vraiment supprimer le modèle sélectionné ? Cette action est irréversible.
models-download-error-title = Erreur
models-download-failed = Échec du téléchargement
models-speed-badge = { $ratio }× realtime
models-speed-badge-tip = Measured realtime speed from this machine's live recording sessions

## Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor + ✚
## conflict detection, design spec §8.3). The 20 shortcut-desc-* strings are
## Buzz's own `shortcut.py` descriptions, byte-for-byte.

shortcut-desc-open-record-window = Ouvrir la fenêtre d'enregistrement
shortcut-desc-import-file = Importer un fichier
shortcut-desc-import-url = Importer une URL
shortcut-desc-open-preferences-window = Ouvrir les préférences
shortcut-desc-view-transcript-text = Afficher le texte de la transcription
shortcut-desc-view-transcript-translation = Afficher la traduction de la transcription
shortcut-desc-view-transcript-timestamps = Afficher les horodatages de la transcription
shortcut-desc-search-transcript = Rechercher dans la transcription
shortcut-desc-search-next = Résultat suivant de la recherche
shortcut-desc-search-previous = Résultat précédent de la recherche
shortcut-desc-scroll-to-current-text = Faire défiler jusqu'au texte actuel
shortcut-desc-play-pause-audio = Lecture / Pause
shortcut-desc-replay-current-segment = Rejouer le segment actuel
shortcut-desc-toggle-playback-controls = Afficher / Masquer les contrôles de lecture
shortcut-desc-decrease-segment-start = Diminuer le début du segment
shortcut-desc-increase-segment-start = Augmenter le début du segment
shortcut-desc-decrease-segment-end = Diminuer la fin du segment
shortcut-desc-increase-segment-end = Augmenter la fin du segment
shortcut-desc-clear-history = Effacer l'historique
shortcut-desc-stop-transcription = Annuler la transcription
prefs-shortcuts-press = Press shortcut…
prefs-shortcuts-clear-tooltip = Reset to default
prefs-shortcuts-conflict = Conflicts with { $other }
prefs-shortcuts-reset = Rétablir les valeurs par défaut

## Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c, design
## spec §8.4). Row labels/Browse are Buzz's own `folder_watch_preferences_
## widget.py` strings, ported verbatim. `prefs-fw-disposition-note` is a ✚
## humm-only addition (design spec §3.4) -- Buzz never told users a watched
## file's source is always removed from the input tree (deleted or moved);
## the embedded options form below this section reuses the options-sheet's
## own `options-*`/shared strings above, no new keys needed for it.

prefs-fw-enable = Activer la surveillance de dossier
prefs-fw-input-folder-label = Dossier d'entrée
prefs-fw-input-folder-placeholder = /path/to/input/folder
prefs-fw-output-folder-label = Dossier de sortie
prefs-fw-output-folder-placeholder = /path/to/output/folder
prefs-fw-browse = Parcourir
prefs-fw-delete-checkbox = Supprimer les fichiers traités
prefs-fw-disposition-note = Once transcribed, the source file is moved into the output folder — or deleted, if Delete processed files is checked.

## Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010, design
## spec §8.5, Task 16 + M8 Tasks 7/8) -- the config home for the native
## post-processing hooks (F-110's resolution: features, not plugins). Section
## titles and field labels/descriptions are Buzz's own `plugins/*/plugin.py`
## `config_fields` strings, ported byte-faithful (they were user-facing
## copy). `prefs-fw-browse`/`dialog-ok` above are reused for this tab's own
## Browse buttons and key-commit-failure modal.

prefs-postproc-header = Ignorer, la réduction du bruit et la détection de la langue s'exécutent avant la transcription. Le redimensionnement, le résumé et l'export DOCX s'exécutent après. Un échec est journalisé et ne fait jamais échouer la transcription.
prefs-postproc-skip-title = Ignorer les déjà transcrits
prefs-postproc-skip-enable = Enable Skip Already Transcribed
prefs-postproc-skip-check-files = Vérifier les fichiers de résultats existants
prefs-postproc-skip-check-files-desc = Ignorer si un fichier .txt, .srt ou .vtt correspondant au fichier audio est trouvé à côté
prefs-postproc-skip-check-db = Vérifier dans la base de données des transcriptions
prefs-postproc-skip-check-db-desc = Ignorer si ce fichier a déjà été transcrit et que l'enregistrement existe dans la base de données
prefs-postproc-dfn-title = DeepFilterNet Noise Reduction
prefs-postproc-dfn-enable = Enable DeepFilterNet Noise Reduction
prefs-postproc-dfn-keep-file = Keep denoised file after transcription
prefs-postproc-langdetect-title = Détection de langue améliorée
prefs-postproc-langdetect-enable = Enable Enhanced language detection
prefs-postproc-langdetect-download-tiny = Télécharger le modèle 'tiny' si aucun n'est disponible
prefs-postproc-langdetect-download-tiny-desc = Si aucun modèle whisper.cpp n'est téléchargé, télécharge le modèle 'tiny' pour la détection de la langue.
prefs-postproc-resizer-title = Redimensionneur automatique de transcription
prefs-postproc-resizer-enable = Enable Automatic Transcript Resizer
prefs-postproc-resizer-merge-by-gap = Fusionner par espace
prefs-postproc-resizer-merge-gap-seconds = Espace de fusion (secondes)
prefs-postproc-resizer-split-by-punctuation = Diviser par ponctuation
prefs-postproc-resizer-punctuation = Ponctuation
prefs-postproc-resizer-split-by-max-length = Diviser par longueur max
prefs-postproc-resizer-max-length = Longueur max des sous-titres
prefs-postproc-summary-title = Résumé IA
prefs-postproc-summary-enable = Enable AI Summary
prefs-postproc-summary-api-url = URL de base de l'API
prefs-postproc-summary-api-key-label = Clé API
prefs-postproc-summary-model = Modèle
prefs-postproc-summary-prompt = Invite de résumé
prefs-postproc-summary-save-to-notes = Sauvegarder le résumé dans les Notes
prefs-postproc-summary-save-to-file = Sauvegarder le résumé dans un fichier
prefs-postproc-summary-output-folder = Dossier de sortie (pour le fichier)
prefs-postproc-summary-key-save-error-body = Unable to save AI Summary API key to keyring
prefs-postproc-docx-title = Exporter en DOCX
prefs-postproc-docx-enable = Enable Export to DOCX
prefs-postproc-docx-output-folder = Dossier de sortie
prefs-postproc-docx-include-timestamps = Inclure les horodatages
prefs-postproc-output-folder-hint = Laisser vide pour sauvegarder à côté du fichier source.

## Transcript viewer window (views/viewer.rs, F-080)

viewer-view = Affichage
viewer-mode-text = Texte
viewer-mode-translation = Traduction
viewer-mode-timestamps = Horodatages
viewer-export = Exporter
viewer-translate = Traduire
viewer-resize = Redimensionner
viewer-identify-speakers = Identifier les intervenants
viewer-find = Rechercher
viewer-find-label = Rechercher :
viewer-find-placeholder = Saisissez le texte à rechercher...
viewer-find-clear = Effacer
viewer-find-prev-tooltip = Previous match ({ $shortcut })
viewer-find-next-tooltip = Next match ({ $shortcut })
viewer-empty = No segments — the input may be silence-only
viewer-open-error = Could not open the transcript
viewer-col-start = Début
viewer-col-end = Fin
viewer-col-text = Texte
viewer-col-translation = Traduction
viewer-saved = edited · saved
viewer-unsaved-warning = An edit could not be saved — it is kept in memory but not written to disk
viewer-player-loading = Loading audio…
viewer-player-failed = Playback unavailable
viewer-playback-controls = Contrôles de lecture :
viewer-loop-segment = Boucler le segment
viewer-loop-segment-tooltip = Activer/désactiver la boucle lors du clic sur les segments de transcription
viewer-follow-audio = Suivre l'audio
viewer-follow-audio-tooltip = Activer/désactiver le suivi de la position audio actuelle dans la transcription. Lorsque activé, fait défiler automatiquement jusqu'au texte actuel.
viewer-speed = Speed:
viewer-scroll-to-current = Faire défiler jusqu'au texte actuel
viewer-scroll-to-current-tooltip = Faire défiler jusqu'au texte en cours de lecture

## Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15, design
## spec §7.3). The key-required title/body are byte-exact ports of Buzz's
## `on_translate_button_clicked` (`transcription_viewer_widget.py:1303-1306`).
## viewer-translate-settings-title has no Buzz equivalent: Buzz opens its
## full Advanced Settings dialog from this gate (whose own title is "Advanced
## Settings", reused verbatim as recording-advanced-title); this modal shows
## only the LLM model/prompt fields (options-llm-model-label/
## options-llm-model-presets-button/options-llm-prompt-label, shared with the
## options sheet's block), so it gets its own, narrower title instead.

viewer-translate-key-required-title = Clé API requise
viewer-translate-key-required-body = Veuillez entrer une clé API OpenAI dans les préférences
viewer-translate-settings-title = AI Translation Settings

## Export menu (views/export_menu.rs, F-084)
##
## viewer-export-text/viewer-export-translation are their own keys, not a
## reuse of viewer-mode-text/viewer-mode-translation above — see
## `export_menu::entry_label`'s doc comment.

viewer-export-text = Texte
viewer-export-translation = Traduction
viewer-export-dialog-title = Sauvegarder le fichier
viewer-export-filter = Fichiers texte
viewer-export-failed = The export could not be written — see the log for details
export-written = Written to { $path }

## Transcript resizer window (views/resizer.rs, F-086)
##
## Section labels/tooltips are byte-exact ports of Buzz's
## `transcription_resizer_widget.py` strings.

resizer-settings = Paramètres
resizer-create-new = Créer une nouvelle transcription
resizer-extend-heading = Prolonger la durée de fin
resizer-extend-label = Prolonger la fin jusqu'à (secondes)
resizer-extend-button = Prolonger les fins
resizer-resize-heading = Options de redimensionnement
resizer-resize-label = Durée souhaitée des sous-titres
resizer-resize-button = Redimensionner
resizer-resize-disabled-tooltip = Disponible uniquement si les horodatages par mot ont été désactivés lors de la transcription
resizer-merge-heading = Options de fusion
resizer-merge-by-gap = Fusionner par espace
resizer-split-by-punctuation = Diviser par ponctuation
resizer-split-by-max-length = Diviser par longueur max
resizer-merge-button = Fusionner
resizer-merge-disabled-tooltip = Disponible uniquement si les horodatages par mot ont été activés lors de la transcription
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

speaker-id-step1-heading = Étape 1 : Identifier les intervenants
speaker-id-step2-heading = Étape 2 : Nommer les intervenants
speaker-id-ready-label = Prêt à identifier les intervenants
speaker-id-missing-audio-label = Fichier audio introuvable
speaker-id-identify-button = Identifier
speaker-id-cancel-button = Annuler
speaker-id-cancelling = Annulation en cours...
speaker-id-decoding = Decoding audio…
speaker-id-downloading-models = Downloading models…
speaker-id-identifying = Identifying speakers…
speaker-id-play-sample = Lire un échantillon
speaker-id-merge-checkbox = Fusionner les phrases du même intervenant
speaker-id-save-button = Sauvegarder
speaker-id-save-error = The changes could not be saved:

## Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
##
## Buzz-parity strings are byte-exact ports of
## `recording_transcriber_widget.py`/`record_button.py`/`audio_meter_widget.py`.

recording-window-title = Enregistrement en direct
recording-model-load-note = La première utilisation d'un modèle peut prendre plusieurs minutes à charger.
recording-advanced-button = Avancé...
recording-microphone-label = Microphone :
recording-record-button = Enregistrer
recording-stop-button = Arrêter
recording-realtime-warning = { $size } transcribed at { $ratio }× realtime last run — live transcription may fall behind
recording-chunk-skipped-warning = A chunk failed and was skipped: { $detail }
recording-panel-placeholder = Cliquez sur Enregistrer pour commencer...
# Byte-exact Buzz string (`recording_transcriber_widget.py:193`,
# `translation_text_box.setPlaceholderText(_("Waiting for AI translation..."))`).
recording-panel-translation-placeholder = En attente de la traduction IA...
recording-model-downloading = Downloading model… { $percent }%
recording-model-downloading-indeterminate = Téléchargement du modèle
recording-model-loading = Loading model…
recording-error-prefix = Une erreur est survenue lors du lancement d'un nouvel enregistrement :
recording-error-suffix = Veuillez vérifier vos périphériques audio ou consulter les journaux de l'application pour plus d'informations.
recording-thread-crashed = The recording session ended unexpectedly. Please check the application logs for more information.
meter-average-volume = Volume moyen
meter-queue = File d'attente

## Recording advanced-settings sheet (views/recording_advanced.rs, F-060, Task 12)
##
## Byte-exact ports of `advanced_settings_dialog.py`'s labels (`_setup_*_section`,
## `:59-255`) — the LLM-translation block is live as of M6 (F-063, spec §7.4);
## the AI model/prompt fields reuse the shared options-llm-* strings.

recording-advanced-title = Paramètres avancés
recording-advanced-speech-heading = Paramètres de reconnaissance vocale
recording-advanced-initial-prompt-label = Invite initiale :
recording-advanced-translation-heading = Paramètres de traduction
recording-advanced-llm-enable = Activer la traduction IA
# The AI model/instructions fields reuse the shared options-llm-* strings
# (`views::options_sheet::render_llm_model_prompt_fields`), so the recording
# window has no separate model/instructions label keys of its own.
recording-advanced-recording-heading = Paramètres d'enregistrement
recording-advanced-silence-threshold-label = Seuil de silence :
recording-advanced-mode-label = Live recording mode:
recording-advanced-mode-append-below = Ajouter en dessous
recording-advanced-mode-append-above = Ajouter au-dessus
recording-advanced-mode-append-and-correct = Ajouter et corriger
recording-advanced-separator-label = Séparateur de ligne :
recording-advanced-step-label = Étape de transcription :
recording-advanced-hide-unconfirmed = Masquer les non confirmés
recording-advanced-export-enable = Activer l'exportation en direct
recording-advanced-export-folder-label = Dossier d'exportation :
recording-advanced-export-browse = Parcourir
recording-advanced-export-file-name-label = Nom du fichier d'exportation :
recording-advanced-export-file-type-label = Type de fichier d'exportation :
recording-advanced-export-file-type-txt = Fichier texte (.txt)
recording-advanced-export-file-type-csv = CSV (.csv)
recording-advanced-export-max-entries-label = 
    Limit export entries
    (0 = export all):
recording-advanced-upload-url-label = Upload URL:
recording-advanced-ok = Ok

## Presentation window + bar (views/presentation.rs, F-065, Task 12)
##
## Byte-exact ports of `recording_transcriber_widget.py:267-482` (the bar)
## and `presentation_window.py` (the window title). `presentation-show-button`
## is humm's own short button label — Buzz's own button is icon-only, its
## tooltip is the byte-exact-cited string (`presentation-show-tooltip`).

presentation-show-button = Show
presentation-show-tooltip = Afficher dans une nouvelle fenêtre
presentation-text-size-label = Taille du texte :
presentation-theme-label = Thème
presentation-theme-light = Clair
presentation-theme-dark = Sombre
presentation-theme-custom = Personnalisé
presentation-text-color-label = Couleur du texte
presentation-background-color-label = Couleur d'arrière-plan
presentation-fullscreen-button = Plein écran
presentation-window-title = Présentation de la transcription en direct

## Copy bar (views/recording.rs, F-065 §2.7, Task 12)

recording-copy-button = Copier
recording-copy-tooltip = Copier la transcription dans le presse-papiers
recording-copy-copied = Copié !
recording-copy-nothing = Rien à copier !

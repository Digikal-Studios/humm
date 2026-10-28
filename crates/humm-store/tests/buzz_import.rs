//! Integration tests for `SqliteStore::import_buzz_db` (Q-6, F-029): the
//! one-time import of a user's existing `Buzz.sqlite` into humm's store.
//!
//! The fixture Buzz database is built by executing
//! `fixtures/buzz_schema.sql` (copied verbatim from Buzz's own
//! `buzz/schema.sql`) with a plain `rusqlite::Connection`, then inserting
//! rows directly — deliberately bypassing `SqliteStore` so this exercises a
//! database shaped exactly like a real Buzz install, not humm's own writer.

use std::path::Path;

use humm_core::boundary::TaskStore;
use humm_core::domain::{
    Backend, DEFAULT_TEMPERATURE, Input, Millis, ModelSize, ModelSpec, OutputFormat, OutputSpec,
    Segment, Source, Status, TaskKind, TaskSpec, TranscriptionId, TranscriptionOptions,
};
use humm_core::language::Language;
use humm_store::SqliteStore;
use pretty_assertions::assert_eq;
use rusqlite::{Connection, params};

const FIXTURE_SCHEMA: &str = include_str!("fixtures/buzz_schema.sql");

const COMPLETED_ID: &str = "11111111-1111-1111-1111-111111111111";
const FAILED_ID: &str = "22222222-2222-2222-2222-222222222222";
const IN_PROGRESS_ID: &str = "33333333-3333-3333-3333-333333333333";
const URL_ID: &str = "44444444-4444-4444-4444-444444444444";
const NULL_LANGUAGE_ID: &str = "55555555-5555-5555-5555-555555555555";

const INSERT_TRANSCRIPTION_SQL: &str = "INSERT INTO transcription (
        id, error_message, export_formats, file, output_folder, progress,
        language, model_type, source, status, task, time_ended, time_queued,
        time_started, url, whisper_model_size, hugging_face_model_id,
        word_level_timings, extract_speech, name, notes
    ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
        ?17, ?18, ?19, ?20, ?21
    )";

const INSERT_SEGMENT_SQL: &str = "INSERT INTO transcription_segment \
     (end_time, start_time, text, translation, transcription_id) \
     VALUES (?1, ?2, ?3, ?4, ?5)";

/// Builds a fixture `Buzz.sqlite` at `path` by executing the verbatim
/// schema, then inserting five `transcription` rows covering every shape
/// the brief calls for (completed-with-segments, failed, in-progress, a URL
/// task, and a row with a `NULL` language) plus three segments on the
/// completed row (one with an empty translation, two with translation
/// text, matching Buzz's default `''`).
fn build_buzz_fixture(path: &Path) {
    let conn = Connection::open(path).expect("open fixture db");
    conn.execute_batch(FIXTURE_SCHEMA)
        .expect("apply buzz schema");

    conn.execute(
        INSERT_TRANSCRIPTION_SQL,
        params![
            COMPLETED_ID,
            Option::<String>::None,
            "txt, srt",
            "/buzz/audio1.mp3",
            "/buzz/out",
            1.0,
            "en",
            "Whisper.cpp",
            "file_import",
            "completed",
            "transcribe",
            "2024-01-01T10:05:00.000000",
            "2024-01-01T10:00:00.000000",
            "2024-01-01T10:00:05.000000",
            Option::<String>::None,
            "small",
            Option::<String>::None,
            true,
            false,
            Option::<String>::None,
            Option::<String>::None,
        ],
    )
    .expect("insert completed row");

    conn.execute(
        INSERT_SEGMENT_SQL,
        params![1000_i64, 0_i64, "Hello world", "", COMPLETED_ID],
    )
    .expect("insert segment 1");
    conn.execute(
        INSERT_SEGMENT_SQL,
        params![
            2500_i64,
            1000_i64,
            "How are you",
            "Comment ça va",
            COMPLETED_ID
        ],
    )
    .expect("insert segment 2");
    conn.execute(
        INSERT_SEGMENT_SQL,
        params![4000_i64, 2500_i64, "Goodbye", "Au revoir", COMPLETED_ID],
    )
    .expect("insert segment 3");

    conn.execute(
        INSERT_TRANSCRIPTION_SQL,
        params![
            FAILED_ID,
            Some("ffmpeg not found"),
            "txt",
            "/buzz/audio2.wav",
            Option::<String>::None,
            0.42,
            "fr",
            "Whisper.cpp",
            "file_import",
            "failed",
            "transcribe",
            "2024-01-02T09:00:00.000000",
            "2024-01-02T08:55:00.000000",
            "2024-01-02T08:55:10.000000",
            Option::<String>::None,
            "medium",
            Option::<String>::None,
            false,
            false,
            Option::<String>::None,
            Option::<String>::None,
        ],
    )
    .expect("insert failed row");

    conn.execute(
        INSERT_TRANSCRIPTION_SQL,
        params![
            IN_PROGRESS_ID,
            Option::<String>::None,
            "srt, vtt",
            "/buzz/audio3.mp3",
            "/buzz/out3",
            0.65,
            "de",
            "Whisper.cpp",
            "file_import",
            "in_progress",
            "translate",
            Option::<String>::None,
            "2024-01-03T00:00:00.000000",
            "2024-01-03T00:00:05.000000",
            Option::<String>::None,
            "large-v3",
            Option::<String>::None,
            true,
            true,
            Option::<String>::None,
            Option::<String>::None,
        ],
    )
    .expect("insert in-progress row");

    conn.execute(
        INSERT_TRANSCRIPTION_SQL,
        params![
            URL_ID,
            Option::<String>::None,
            "txt",
            Option::<String>::None,
            "/buzz/out4",
            0.0,
            "en",
            "Whisper.cpp",
            "url_import",
            "queued",
            "transcribe",
            Option::<String>::None,
            "2024-01-04T00:00:00.000000",
            Option::<String>::None,
            "https://example.com/podcast.mp3",
            "small",
            Option::<String>::None,
            false,
            false,
            Option::<String>::None,
            Option::<String>::None,
        ],
    )
    .expect("insert url row");

    conn.execute(
        INSERT_TRANSCRIPTION_SQL,
        params![
            NULL_LANGUAGE_ID,
            Option::<String>::None,
            "txt",
            "/buzz/audio5.mp3",
            "/buzz/out5",
            1.0,
            Option::<String>::None,
            "Hugging Face",
            "file_import",
            "completed",
            "transcribe",
            "2024-01-05T00:10:00.000000",
            "2024-01-05T00:00:00.000000",
            "2024-01-05T00:00:05.000000",
            Option::<String>::None,
            Option::<String>::None,
            "openai/whisper-large-v3",
            false,
            false,
            Some("Custom name"),
            Some("Some notes here"),
        ],
    )
    .expect("insert null-language row");
}

/// Columns `TaskRow` intentionally doesn't expose (`language`, `model_type`,
/// `task`, `export_formats`, `whisper_model_size`, `hugging_face_model_id`,
/// `word_level_timings`, `extract_speech`, `kind`) — fetched via a second,
/// independent connection to the same on-disk store file so verbatim
/// column survival can still be spot-checked without reaching into
/// `SqliteStore`'s private state.
#[allow(clippy::type_complexity)]
fn fetch_raw_columns(
    humm_path: &Path,
    id: &str,
) -> (
    Option<String>, // language
    Option<String>, // model_type
    Option<String>, // task
    Option<String>, // export_formats
    Option<String>, // whisper_model_size
    Option<String>, // hugging_face_model_id
    bool,           // word_level_timings
    bool,           // extract_speech
    Option<String>, // options_json
    String,         // kind
) {
    let conn = Connection::open(humm_path).expect("open store file for verification");
    conn.query_row(
        "SELECT language, model_type, task, export_formats, whisper_model_size, \
         hugging_face_model_id, word_level_timings, extract_speech, options_json, kind \
         FROM transcription WHERE id = ?1",
        params![id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ))
        },
    )
    .expect("query raw columns")
}

fn sample_task_spec() -> TaskSpec {
    TaskSpec {
        input: Input::Path(std::path::PathBuf::from("/tmp/in.wav")),
        options: TranscriptionOptions {
            language: Language::parse("en"),
            task: TaskKind::Transcribe,
            model: ModelSpec {
                backend: Backend::Whisper,
                size: ModelSize::Small,
                hf_repo: None,
            },
            word_timestamps: false,
            initial_prompt: String::new(),
            temperature: DEFAULT_TEMPERATURE.to_vec(),
            llm: None,
            extract_speech: false,
            identify_speakers: false,
        },
        output: OutputSpec {
            formats: [OutputFormat::Txt].into_iter().collect(),
            directory: Some(std::path::PathBuf::from("/tmp/out")),
            file_name_template: None,
            paragraph_split: Millis(2000),
            source_disposition: None,
        },
    }
}

#[test]
fn imports_buzz_rows_and_segments_verbatim() {
    let dir = tempfile::tempdir().expect("tempdir");
    let buzz_path = dir.path().join("Buzz.sqlite");
    build_buzz_fixture(&buzz_path);
    let humm_path = dir.path().join("humm.sqlite");

    let store = SqliteStore::open(&humm_path).expect("open store");
    assert!(!store.buzz_import_done().expect("buzz_import_done"));

    let outcome = store.import_buzz_db(&buzz_path).expect("import");
    assert!(!outcome.skipped);
    assert_eq!(outcome.tasks, 5);
    assert_eq!(outcome.segments, 3);
    assert!(store.buzz_import_done().expect("buzz_import_done"));

    // -- completed row: full spot-check, including the columns `TaskRow`
    // doesn't expose, and the three segments (times + translation text).
    let completed_id = TranscriptionId::parse(COMPLETED_ID).expect("uuid");
    let row = store
        .get_task(&completed_id)
        .expect("get")
        .expect("present");
    assert_eq!(row.status, Status::Completed);
    assert_eq!(row.source, Some(Source::FileImport));
    assert_eq!(row.file.as_deref(), Some(Path::new("/buzz/audio1.mp3")));
    assert_eq!(row.url, None);
    assert_eq!(row.progress, 1.0);
    assert_eq!(row.error_message, None);
    assert_eq!(row.time_queued, "2024-01-01T10:00:00.000000");
    assert_eq!(
        row.time_started.as_deref(),
        Some("2024-01-01T10:00:05.000000")
    );
    assert_eq!(
        row.time_ended.as_deref(),
        Some("2024-01-01T10:05:00.000000")
    );
    assert_eq!(row.name, None);
    assert_eq!(row.notes, None);

    let (
        language,
        model_type,
        task,
        export_formats,
        whisper_model_size,
        hf_id,
        wlt,
        es,
        options_json,
        kind,
    ) = fetch_raw_columns(&humm_path, COMPLETED_ID);
    assert_eq!(language.as_deref(), Some("en"));
    assert_eq!(model_type.as_deref(), Some("Whisper.cpp"));
    assert_eq!(task.as_deref(), Some("transcribe"));
    assert_eq!(export_formats.as_deref(), Some("txt, srt"));
    assert_eq!(whisper_model_size.as_deref(), Some("small"));
    assert_eq!(hf_id, None);
    assert!(wlt);
    assert!(!es);
    // Contract: imported rows restart from column data, not resumed JSON.
    assert_eq!(options_json, None);
    assert_eq!(kind, "file");

    let segments = store.get_segments(&completed_id).expect("segments");
    assert_eq!(
        segments,
        vec![
            Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "Hello world".to_string(),
                translation: String::new(),
            },
            Segment {
                start: Millis(1000),
                end: Millis(2500),
                text: "How are you".to_string(),
                translation: "Comment ça va".to_string(),
            },
            Segment {
                start: Millis(2500),
                end: Millis(4000),
                text: "Goodbye".to_string(),
                translation: "Au revoir".to_string(),
            },
        ]
    );

    // -- failed row.
    let failed_id = TranscriptionId::parse(FAILED_ID).expect("uuid");
    let row = store.get_task(&failed_id).expect("get").expect("present");
    assert_eq!(row.status, Status::Failed);
    assert_eq!(row.error_message.as_deref(), Some("ffmpeg not found"));
    assert_eq!(row.progress, 0.42);
    let (language, ..) = fetch_raw_columns(&humm_path, FAILED_ID);
    assert_eq!(language.as_deref(), Some("fr"));

    // -- in-progress row: Buzz status kept verbatim (not yet flipped —
    // that's `cancel_stale_tasks`'s job, called separately by the caller).
    let in_progress_id = TranscriptionId::parse(IN_PROGRESS_ID).expect("uuid");
    let row = store
        .get_task(&in_progress_id)
        .expect("get")
        .expect("present");
    assert_eq!(row.status, Status::InProgress);
    let (_, _, task, ..) = fetch_raw_columns(&humm_path, IN_PROGRESS_ID);
    assert_eq!(task.as_deref(), Some("translate"));

    // -- URL row.
    let url_id = TranscriptionId::parse(URL_ID).expect("uuid");
    let row = store.get_task(&url_id).expect("get").expect("present");
    assert_eq!(row.file, None);
    assert_eq!(row.url.as_deref(), Some("https://example.com/podcast.mp3"));
    assert_eq!(row.source, Some(Source::UrlImport));
    assert_eq!(row.status, Status::Queued);

    // -- NULL-language row (also spot-checks name/notes surviving verbatim).
    let null_language_id = TranscriptionId::parse(NULL_LANGUAGE_ID).expect("uuid");
    let row = store
        .get_task(&null_language_id)
        .expect("get")
        .expect("present");
    assert_eq!(row.name.as_deref(), Some("Custom name"));
    assert_eq!(row.notes.as_deref(), Some("Some notes here"));
    let (language, model_type, _, _, whisper_model_size, hf_id, ..) =
        fetch_raw_columns(&humm_path, NULL_LANGUAGE_ID);
    assert_eq!(language, None);
    assert_eq!(model_type.as_deref(), Some("Hugging Face"));
    assert_eq!(whisper_model_size, None);
    assert_eq!(hf_id.as_deref(), Some("openai/whisper-large-v3"));
}

#[test]
fn second_import_call_is_skipped_and_does_not_read_the_buzz_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let buzz_path = dir.path().join("Buzz.sqlite");
    build_buzz_fixture(&buzz_path);
    let humm_path = dir.path().join("humm.sqlite");

    let store = SqliteStore::open(&humm_path).expect("open store");
    let first = store.import_buzz_db(&buzz_path).expect("first import");
    assert!(!first.skipped);
    assert_eq!(first.tasks, 5);
    assert_eq!(first.segments, 3);

    // The guard trips before any file access, so even a nonexistent path
    // must still be reported as skipped rather than erroring.
    let nonexistent = dir.path().join("does/not/exist.sqlite");
    let second = store.import_buzz_db(&nonexistent).expect("second import");
    assert!(second.skipped);
    assert_eq!(second.tasks, 0);
    assert_eq!(second.segments, 0);

    // Counts in the store are unchanged by the skipped call.
    assert_eq!(store.list_tasks().expect("list").len(), 5);
    let completed_id = TranscriptionId::parse(COMPLETED_ID).expect("uuid");
    assert_eq!(
        store.get_segments(&completed_id).expect("segments").len(),
        3
    );
}

#[test]
fn preexisting_row_with_same_id_is_not_duplicated_and_its_segments_are_skipped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let buzz_path = dir.path().join("Buzz.sqlite");
    build_buzz_fixture(&buzz_path);
    let humm_path = dir.path().join("humm.sqlite");

    let store = SqliteStore::open(&humm_path).expect("open store");

    // Pre-seed a row under the same id as the fixture's completed row,
    // before it's ever imported.
    let completed_id = TranscriptionId::parse(COMPLETED_ID).expect("uuid");
    store
        .create_task(&completed_id, &sample_task_spec(), Source::FileImport)
        .expect("pre-seed row");

    let outcome = store.import_buzz_db(&buzz_path).expect("import");
    assert!(!outcome.skipped);
    // 5 fixture rows minus the 1 duplicate id.
    assert_eq!(outcome.tasks, 4);
    // The completed row (the only one with segments) was not imported this
    // run, so its segments must not be copied either — not merely
    // deduplicated, but excluded from the segment copy entirely.
    assert_eq!(outcome.segments, 0);

    // The pre-existing row is untouched (still `queued`, from `create_task`,
    // not overwritten with Buzz's `completed` data).
    let row = store
        .get_task(&completed_id)
        .expect("get")
        .expect("present");
    assert_eq!(row.status, Status::Queued);
    assert!(
        store
            .get_segments(&completed_id)
            .expect("segments")
            .is_empty()
    );

    // Exactly 5 total: the 1 pre-existing + 4 newly imported (not 6).
    assert_eq!(store.list_tasks().expect("list").len(), 5);

    let failed_id = TranscriptionId::parse(FAILED_ID).expect("uuid");
    assert!(store.get_task(&failed_id).expect("get").is_some());
}

#[test]
fn corrupt_buzz_db_errors_without_marker_and_retry_after_fixing_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let buzz_path = dir.path().join("Buzz.sqlite");

    // A valid SQLite file, but missing the `transcription` table the import
    // SQL expects: `ATTACH` itself succeeds (the file opens fine), and the
    // failure surfaces only once the import queries it — the case that
    // requires an explicit `DETACH` on the way out.
    {
        let conn = Connection::open(&buzz_path).expect("open corrupt db");
        conn.execute_batch("CREATE TABLE unrelated (x INTEGER);")
            .expect("create unrelated table");
    }
    let humm_path = dir.path().join("humm.sqlite");
    let store = SqliteStore::open(&humm_path).expect("open store");

    store
        .import_buzz_db(&buzz_path)
        .expect_err("corrupt buzz db must error");
    assert!(!store.buzz_import_done().expect("buzz_import_done"));
    assert!(store.list_tasks().expect("list").is_empty());

    // Fix the file in place with a real fixture, then retry. This also
    // proves the earlier failure balanced its ATTACH with a DETACH: if
    // `buzz` were still attached to the store's connection, this second
    // `ATTACH DATABASE ... AS buzz` would itself fail.
    std::fs::remove_file(&buzz_path).expect("remove corrupt file");
    build_buzz_fixture(&buzz_path);

    let outcome = store.import_buzz_db(&buzz_path).expect("retry import");
    assert!(!outcome.skipped);
    assert_eq!(outcome.tasks, 5);
    assert_eq!(outcome.segments, 3);
    assert!(store.buzz_import_done().expect("buzz_import_done"));
}

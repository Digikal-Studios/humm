//! Telemetry (ADR-009): a single `app_launched` event sent to PostHog on
//! startup — a parity port of Buzz's `Application.__init__`
//! (`buzz/widgets/application.py:80-93`), which builds a `Posthog` client
//! with its own project key and fires one `app_launched` event carrying six
//! properties (`app`, `locale`, `system`, `release`, `machine`, `version`)
//! keyed to a persisted anonymous `distinct_id`.
//!
//! humm keeps the event name, the property keys, and the fire-and-forget
//! delivery, but swaps three things: its own PostHog project (own build-time
//! `HUMM_POSTHOG_KEY`, baked in via `option_env!` at the call site — absent
//! at build time means telemetry is off, not "send with no key"), a raw
//! `POST /capture/` instead of vendoring the `posthog` Python client (humm
//! already depends on `reqwest`; ADR-009 calls this "zero new
//! dependencies"), and `PrivacySettings::user_identifier` in place of Buzz's
//! `Settings.get_user_identifier()` for the persisted uuid4 `distinct_id`
//! (both mechanisms — `Settings`/`humm_core::settings` — persist the same
//! kind of value).
//!
//! Every disable path — Buzz's `BUZZ_DISABLE_TELEMETRY` plus humm's two
//! additions (no build-time key; debug builds) plus the settings toggle —
//! collapses into one pure, independently-testable gate: [`telemetry_enabled`].
//! Actually wiring this module into GUI startup (reading the build-time key,
//! persisting `user_identifier`, calling [`Event::app_launched`] after the
//! first frame) is GUI-shell work, not this module's concern — see
//! `crate::settings::PrivacySettings`.

use std::time::Duration;

use serde_json::json;

/// PostHog's raw capture endpoint (`POST /capture/`) — the same one Buzz's
/// `posthog` Python client wraps, called here directly instead.
const POSTHOG_CAPTURE_URL: &str = "https://us.i.posthog.com/capture/";

/// How long [`PostHogTelemetry::capture`] waits for the POST to complete
/// before giving up. Telemetry must never make startup or shutdown feel
/// slow, so this is short and its expiry is silently ignored.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(3);

/// One telemetry event: a name and its already-shaped JSON property bag.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// The PostHog event name. Buzz only ever sends `"app_launched"`
    /// (`application.py:83`); humm currently does the same.
    pub name: &'static str,
    /// Event properties, already shaped as the JSON object PostHog expects
    /// (i.e. ready to nest under a capture payload's `"properties"` key).
    pub properties: serde_json::Value,
}

impl Event {
    /// Builds the `app_launched` event — Buzz's `application.py:80-93`,
    /// property-for-property: `app`, `locale`, `system`, `release`,
    /// `machine`, `version`, in that order, and no others.
    ///
    /// - `app`: `app_version` verbatim (Buzz: `VERSION`).
    /// - `locale`: `locale` verbatim, or JSON `null` when `None` (Buzz sends
    ///   `locale.getlocale()`, a 2-tuple; humm sends a single string or
    ///   nothing — the exact locale representation is GUI-shell work, not
    ///   this constructor's concern).
    /// - `system`: Python `platform.system()` spelling via
    ///   [`crate::platform::platform_system_name`] — `"Darwin"`, `"Linux"`,
    ///   or `"Windows"`, not Rust's lowercase `consts::OS`.
    /// - `release`: `kernel_release` verbatim, or `null` when `None` (Buzz:
    ///   `platform.release()`, the kernel version string, e.g. `"25.5.0"` —
    ///   see [`kernel_release`] for how a caller obtains this value; it is
    ///   deliberately a parameter here, not called internally, so this
    ///   constructor stays a pure function of its inputs). On Windows the
    ///   caller passes the release from [`crate::platform::parse_windows_ver`]
    ///   (`"10"` or `"11"`), which [`kernel_release`] reads from `cmd /C ver`.
    /// - `machine`: [`std::env::consts::ARCH`] with `"aarch64"` mapped to
    ///   `"arm64"` (Buzz's `platform.machine()` reports `"arm64"` on Apple
    ///   Silicon; Rust's `ARCH` constant reports `"aarch64"` for the
    ///   identical hardware — a naming difference to normalize, not a real
    ///   platform difference), else `ARCH` as-is.
    /// - `version`: `os_version` verbatim, or `null` when `None` (Buzz:
    ///   `platform.version()`). macOS sends `sw_vers -productVersion` — see
    ///   [`macos_version`]. Linux sends `uname -v`. Windows sends the
    ///   `ver` version token — see [`os_version`]. The value is a parameter
    ///   so this constructor stays a pure function of its inputs).
    pub fn app_launched(
        app_version: &str,
        locale: Option<&str>,
        kernel_release: Option<&str>,
        os_version: Option<&str>,
    ) -> Event {
        let system = crate::platform::platform_system_name(std::env::consts::OS);
        let machine = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            other => other,
        };
        Event {
            name: "app_launched",
            properties: json!({
                "app": app_version,
                "locale": locale,
                "system": system,
                "release": kernel_release,
                "machine": machine,
                "version": os_version,
            }),
        }
    }
}

/// A sink for telemetry events.
///
/// [`NoopTelemetry`] is the sink whenever [`telemetry_enabled`] says no
/// (opted out, no build-time key, debug build); [`PostHogTelemetry`] is the
/// real one. Callers hold a `Box<dyn Telemetry>`/`Arc<dyn Telemetry>` picked
/// once at startup so the rest of the app never branches on whether
/// telemetry is actually enabled.
pub trait Telemetry: Send + Sync {
    /// Sends `event`. Implementations must not block the caller noticeably
    /// nor let a delivery failure propagate — telemetry is best-effort and
    /// invisible to the rest of the app.
    fn capture(&self, event: Event);
}

/// A [`Telemetry`] sink that discards every event.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopTelemetry;

impl Telemetry for NoopTelemetry {
    fn capture(&self, _event: Event) {}
}

/// A [`Telemetry`] sink that POSTs events to PostHog's raw capture API.
pub struct PostHogTelemetry {
    api_key: String,
    distinct_id: String,
}

impl PostHogTelemetry {
    /// Builds a sink that authenticates with `api_key` (humm's own,
    /// build-time `HUMM_POSTHOG_KEY`) and attributes every event to the
    /// persisted anonymous `distinct_id`.
    pub fn new(api_key: String, distinct_id: String) -> Self {
        PostHogTelemetry {
            api_key,
            distinct_id,
        }
    }

    /// The JSON body [`PostHogTelemetry::capture`] POSTs: PostHog's raw
    /// capture-API shape — `api_key`, `event`, `distinct_id`, `properties`,
    /// and no other keys. Exposed separately from `capture` so the payload
    /// shape is unit-testable without a network call.
    pub fn payload(&self, event: &Event) -> serde_json::Value {
        json!({
            "api_key": self.api_key,
            "event": event.name,
            "distinct_id": self.distinct_id,
            "properties": event.properties,
        })
    }
}

impl Telemetry for PostHogTelemetry {
    /// Fire-and-forget: spawns a detached thread that POSTs
    /// [`PostHogTelemetry::payload`] to [`POSTHOG_CAPTURE_URL`] with a
    /// [`CAPTURE_TIMEOUT`] and discards every outcome — client-build
    /// failure, connection error, non-2xx status, or timeout. A telemetry
    /// failure must never surface to the caller or delay it, and this
    /// module has nothing useful to do with the response body regardless.
    fn capture(&self, event: Event) {
        let payload = self.payload(&event);
        std::thread::spawn(move || {
            let Ok(client) = reqwest::blocking::Client::builder()
                .timeout(CAPTURE_TIMEOUT)
                .build()
            else {
                return;
            };
            let body = serde_json::to_vec(&payload).unwrap_or_default();
            let _ = client
                .post(POSTHOG_CAPTURE_URL)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send();
        });
    }
}

/// The telemetry on/off gate, pure and independently testable.
///
/// Disabled (returns `false`) if any of:
/// - `build_key` is `None` or empty — no PostHog project configured at
///   build time, so there is nothing to send to.
/// - `debug_build` is `true` — never send telemetry from a debug build
///   (ADR-009), regardless of every other input.
/// - `env_disable` is `Some` **and non-empty** — Buzz's
///   `BUZZ_DISABLE_TELEMETRY` parity (`application.py:88`:
///   `os.getenv("BUZZ_DISABLE_TELEMETRY", None)` then `if not
///   disable_telemetry:`). An unset env var is `None` in Rust just as it is
///   in Python (falsy, telemetry stays on); a *set-but-empty* value is
///   `Some("")`, which Python's `not ""` also treats as falsy — so it must
///   NOT disable here either. Any other non-empty value disables, including
///   `"0"`: Python's truthiness test is on the *string*, not a parsed
///   boolean, and a non-empty string (even `"0"`) is truthy.
/// - `settings_enabled` is `false` — the user's Preferences → Privacy
///   toggle (`crate::settings::PrivacySettings::telemetry_enabled`).
pub fn telemetry_enabled(
    build_key: Option<&str>,
    debug_build: bool,
    env_disable: Option<&str>,
    settings_enabled: bool,
) -> bool {
    let has_key = build_key.is_some_and(|key| !key.is_empty());
    let env_disabled = env_disable.is_some_and(|value| !value.is_empty());
    has_key && !debug_build && !env_disabled && settings_enabled
}

/// Best-effort kernel release for the `release` property in
/// [`Event::app_launched`], paralleling Buzz's `platform.release()`.
///
/// Unix: `uname -r` (for example `"25.5.0"`). Windows: the release token
/// from `cmd /C ver`, parsed by [`crate::platform::parse_windows_ver`]
/// (`"11"`, `"10"`, `"6"`, …). Windows calls `ver` only. Git for Windows
/// ships `uname`, and `uname -r` is a different string from
/// `platform.release()`.
///
/// `None` on any failure (command missing, non-zero exit, non-UTF-8 or empty
/// output, or `ver` text with no `[Version …]` token): a `release`-less
/// event still sends (as JSON `null`), so this is a leaf utility with a
/// graceful-absence contract, not something callers need to handle as an
/// error.
pub fn kernel_release() -> Option<String> {
    #[cfg(windows)]
    {
        windows_ver().map(|ver| ver.release)
    }
    #[cfg(not(windows))]
    {
        run_trimmed("uname", &["-r"])
    }
}

/// OS version string for [`Event::app_launched`]'s `version` property.
///
/// macOS: `sw_vers -productVersion`. Linux and other Unix: `uname -v`
/// (Python `platform.version()`). Windows: the version token inside
/// `cmd /C ver`, parsed by [`crate::platform::parse_windows_ver`].
pub fn os_version() -> Option<String> {
    // Each target compiles exactly one body. An `if cfg!(...) { return }`
    // plus cfg-stripped tails leaves macOS with an `if` whose type is `()`
    // (E0317), because the other arms are not in the crate.
    #[cfg(target_os = "macos")]
    {
        macos_version()
    }
    #[cfg(windows)]
    {
        windows_ver().map(|ver| ver.version)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        run_trimmed("uname", &["-v"])
    }
}

/// Live `cmd /C ver`, parsed. Windows-only: other hosts have no `ver`.
#[cfg(windows)]
fn windows_ver() -> Option<crate::platform::WindowsVer> {
    let output = std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    crate::platform::parse_windows_ver(&text)
}

/// Best-effort macOS product version via `sw_vers -productVersion` (e.g.
/// `"15.5"`) — the value humm sends for the `version` property in
/// [`Event::app_launched`], in place of Buzz's `platform.version()` (a
/// longer kernel build string that is less useful to a human reader).
///
/// Same best-effort, `None`-on-failure contract as [`kernel_release`].
pub fn macos_version() -> Option<String> {
    run_trimmed("sw_vers", &["-productVersion"])
}

/// Runs `command args...`, returning its trimmed stdout on a successful,
/// non-empty, valid-UTF-8 exit, or `None` on any failure — the shared
/// best-effort shell-out used by [`kernel_release`] and [`macos_version`].
fn run_trimmed(command: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(command)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- Event::app_launched: property keys/values (Buzz parity) -------------

    #[test]
    fn app_launched_property_keys_match_buzz() {
        let event = Event::app_launched("1.2.3", Some("en_US"), Some("25.5.0"), Some("15.5"));
        let props = event
            .properties
            .as_object()
            .expect("properties is an object");

        let mut keys: Vec<&str> = props.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["app", "locale", "machine", "release", "system", "version"]
        );

        assert_eq!(event.name, "app_launched");
        assert_eq!(props["app"], "1.2.3");
        assert_eq!(props["locale"], "en_US");
        assert_eq!(props["release"], "25.5.0");
        assert_eq!(props["version"], "15.5");
    }

    #[test]
    fn app_launched_system_matches_python_platform_system() {
        let event = Event::app_launched("1.0.0", None, None, None);
        assert_eq!(
            event.properties["system"],
            crate::platform::platform_system_name(std::env::consts::OS)
        );
    }

    /// Linux-only. A `#[cfg]` gate keeps Windows and macOS from reporting this
    /// as passed when the body never ran.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_reports_uname_release_and_version() {
        let release = kernel_release().expect("uname -r");
        let version = os_version().expect("uname -v");
        assert!(!release.is_empty());
        assert!(!version.is_empty());
        // `sw_vers` is macOS-only. A Linux run must not pretend it produced
        // a macOS product version by leaving this as the only check.
        assert!(macos_version().is_none());
    }

    /// Fails unless both live probes match `parse_windows_ver` of a fresh
    /// `cmd /C ver`. `kernel_release` on Windows must come from that parse,
    /// which a successful `uname -r` (Git for Windows) would not.
    #[cfg(windows)]
    #[test]
    fn windows_release_and_version_match_live_ver() {
        let output = std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .output()
            .expect("cmd /C ver");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "cmd /C ver failed: {stderr}");
        let text = String::from_utf8(output.stdout).expect("ver stdout is utf-8");
        let parsed = crate::platform::parse_windows_ver(&text).expect("live ver output must parse");
        let release = kernel_release();
        let version = os_version();
        assert_eq!(release.as_deref(), Some(parsed.release.as_str()));
        assert_eq!(version.as_deref(), Some(parsed.version.as_str()));
    }

    #[test]
    fn app_launched_machine_maps_aarch64_to_arm64() {
        let event = Event::app_launched("1.0.0", None, None, None);
        if cfg!(target_arch = "aarch64") {
            assert_eq!(event.properties["machine"], "arm64");
        } else {
            assert_eq!(event.properties["machine"], std::env::consts::ARCH);
        }
    }

    #[test]
    fn app_launched_absent_optionals_serialize_as_json_null() {
        let event = Event::app_launched("1.0.0", None, None, None);
        assert_eq!(event.properties["locale"], serde_json::Value::Null);
        assert_eq!(event.properties["release"], serde_json::Value::Null);
        assert_eq!(event.properties["version"], serde_json::Value::Null);
    }

    // -- PostHogTelemetry::payload: raw capture-API shape ---------------------

    #[test]
    fn payload_matches_posthog_capture_shape() {
        let telemetry =
            PostHogTelemetry::new("phc_test_key".to_string(), "distinct-123".to_string());
        let event = Event::app_launched("1.2.3", Some("en_US"), Some("25.5.0"), Some("15.5"));
        let payload = telemetry.payload(&event);

        let object = payload.as_object().expect("payload is an object");
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["api_key", "distinct_id", "event", "properties"]);

        assert_eq!(payload["api_key"], "phc_test_key");
        assert_eq!(payload["event"], "app_launched");
        assert_eq!(payload["distinct_id"], "distinct-123");
        assert_eq!(payload["properties"], event.properties);
    }

    // -- telemetry_enabled: full gate matrix -----------------------------------

    /// One row of the gate matrix: `(build_key, debug_build, env_disable,
    /// settings_enabled, expected)`. A named alias, not an inline tuple type,
    /// so the test below doesn't trip clippy's `type_complexity` lint.
    type GateCase = (Option<&'static str>, bool, Option<&'static str>, bool, bool);

    #[test]
    fn telemetry_enabled_gate_matrix() {
        let cases: &[GateCase] = &[
            // Baseline: every gate open -> enabled.
            (Some("phc_key"), false, None, true, true),
            // Gate 1: build key missing or empty -> disabled either way.
            (None, false, None, true, false),
            (Some(""), false, None, true, false),
            // Gate 2: debug build always disables, regardless of everything else.
            (Some("phc_key"), true, None, true, false),
            // Gate 3: env var set and non-empty disables, including "0" (Buzz
            // truthiness is on the string, not a parsed bool); unset or
            // set-but-empty leaves telemetry on.
            (Some("phc_key"), false, Some("1"), true, false),
            (Some("phc_key"), false, Some("0"), true, false),
            (Some("phc_key"), false, Some(""), true, true),
            (Some("phc_key"), false, None, true, true),
            // Gate 4: settings toggle off disables.
            (Some("phc_key"), false, None, false, false),
        ];

        for &(build_key, debug_build, env_disable, settings_enabled, expected) in cases {
            assert_eq!(
                telemetry_enabled(build_key, debug_build, env_disable, settings_enabled),
                expected,
                "build_key={build_key:?} debug_build={debug_build} \
                 env_disable={env_disable:?} settings_enabled={settings_enabled}"
            );
        }
    }

    // -- NoopTelemetry: discards without panicking -----------------------------

    #[test]
    fn noop_telemetry_capture_does_not_panic() {
        let telemetry = NoopTelemetry;
        telemetry.capture(Event::app_launched("1.0.0", None, None, None));
    }
}

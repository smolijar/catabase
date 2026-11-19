//! Read OS crash reports after restart. Never try to recover from a fatal signal.
//! Only an allowlist of technical fields leaves this module; raw logs stay local.
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use super::{AppError, CoreError};

const REPORT_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const MAX_SQL_CHARS: usize = 64_000;
const ISSUE_URL: &str = "https://github.com/smolijar/catabase/issues/new";

pub(super) struct Diagnostics {
    directory: PathBuf,
    context_file: PathBuf,
    query: Mutex<Option<QueryContext>>,
    // Serializes acknowledgements, without holding a lock during OS log scans.
    acknowledgements: Mutex<()>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct QueryContext {
    sql: String,
    phase: String,
    truncated: bool,
    pid: u32,
    started_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DiagnosticReport {
    id: Option<String>,
    new_crash: bool,
    title: String,
    body: String,
    query: Option<ReportQuery>,
    note: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReportQuery {
    sql: String,
    phase: String,
    truncated: bool,
    started_at: DateTime<Utc>,
    from_crash: bool,
}

struct Crash {
    id: String,
    time: DateTime<Utc>,
    pid: Option<u32>,
    launched_at: Option<DateTime<Utc>>,
    version: String,
    exception: String,
    details: String,
}

impl Diagnostics {
    pub(super) fn new(data_dir: &Path) -> Self {
        let directory = data_dir.join("diagnostics");
        let _ = fs::create_dir_all(&directory);
        // Keep a bounded set of previous sessions, so crash SQL survives restart.
        for path in recent_files(&directory, "query-", "json")
            .into_iter()
            .skip(20)
        {
            let _ = fs::remove_file(path);
        }
        Self {
            context_file: directory.join(format!("query-{}.json", uuid::Uuid::new_v4())),
            directory,
            query: Mutex::new(None),
            acknowledgements: Mutex::new(()),
        }
    }

    fn acknowledge(&self, id: &str) -> Result<(), CoreError> {
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CoreError::message("Invalid diagnostic report."));
        }
        let _lock = self
            .acknowledgements
            .lock()
            .map_err(|_| CoreError::message("Diagnostics are busy."))?;
        let mut ids = self.acknowledged();
        if !ids.iter().any(|known| known == id) {
            ids.push(id.to_string());
        }
        if ids.len() > 100 {
            ids.drain(..ids.len() - 100);
        }
        write_private(&self.directory.join("reviewed.json"), &ids)
    }

    fn acknowledged(&self) -> Vec<String> {
        read_json(&self.directory.join("reviewed.json"), 16_000).unwrap_or_default()
    }

    fn crash_query(&self, crash: &Crash) -> Option<QueryContext> {
        // PID plus execution time avoids attaching another session's SQL.
        recent_files(&self.directory, "query-", "json")
            .into_iter()
            .take(21)
            .filter_map(|path| read_json::<QueryContext>(&path, 512_000))
            .filter(|query| {
                Some(query.pid) == crash.pid
                    && query.started_at <= crash.time
                    && crash
                        .launched_at
                        .is_none_or(|launch| query.started_at >= launch)
                    && crash
                        .time
                        .signed_duration_since(query.started_at)
                        .num_seconds()
                        < REPORT_AGE.as_secs() as i64
            })
            .max_by_key(|query| query.started_at)
    }
}

pub(super) struct QueryTracking(Option<Arc<Diagnostics>>, DateTime<Utc>);

pub(super) struct QueryObserver {
    app: AppHandle,
    tracking: Mutex<Option<QueryTracking>>,
}

impl QueryObserver {
    pub(super) fn new(app: &AppHandle) -> Self {
        Self {
            app: app.clone(),
            tracking: Mutex::new(None),
        }
    }
}

impl catabase_core::StatementObserver for QueryObserver {
    fn started(&self, sql: &str) {
        if let Ok(mut tracking) = self.tracking.lock() {
            tracking.take();
            *tracking = Some(track_query(&self.app, sql));
        }
    }

    fn finished(&self, _: &str, _: Result<&catabase_core::QueryResult, &CoreError>, _: u128) {
        if let Ok(mut tracking) = self.tracking.lock() {
            tracking.take();
        }
    }
}

/// Best-effort local context, like SQL history. Sharing it always requires opt-in.
pub(super) fn track_query(app: &AppHandle, sql: &str) -> QueryTracking {
    let state = app
        .try_state::<Arc<Diagnostics>>()
        .map(|state| Arc::clone(&state));
    let started_at = Utc::now();
    if let Some(state) = &state
        && let Ok(mut latest) = state.query.lock()
    {
        let context = QueryContext {
            sql: sql.chars().take(MAX_SQL_CHARS).collect(),
            truncated: sql.chars().count() > MAX_SQL_CHARS,
            phase: "running".into(),
            pid: std::process::id(),
            started_at,
        };
        let _ = write_private(&state.context_file, &context);
        *latest = Some(context);
    }
    QueryTracking(state, started_at)
}

impl Drop for QueryTracking {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if let Some(state) = &self.0
            && let Ok(mut latest) = state.query.lock()
            && let Some(context) = latest.as_mut()
            && context.started_at == self.1
        {
            context.phase = "completed".into();
            let _ = write_private(&state.context_file, context);
        }
    }
}

#[tauri::command]
pub(super) async fn diagnostics_report(
    app: AppHandle,
    current_session: Option<bool>,
) -> Result<DiagnosticReport, AppError> {
    let state = Arc::clone(&app.state::<Arc<Diagnostics>>());
    let version = app.package_info().version.to_string();
    let logs = if cfg!(target_os = "macos") {
        app.path()
            .home_dir()
            .ok()
            .map(|home| home.join("Library/Logs/DiagnosticReports"))
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || {
        make_report(
            &state,
            &version,
            logs.as_deref(),
            current_session.unwrap_or(false),
            SystemTime::now(),
        )
    })
    .await
    .map_err(|_| CoreError::message("Could not prepare diagnostics.").into())
}

#[tauri::command]
pub(super) async fn acknowledge_diagnostics(app: AppHandle, id: String) -> Result<(), AppError> {
    let state = Arc::clone(&app.state::<Arc<Diagnostics>>());
    tauri::async_runtime::spawn_blocking(move || state.acknowledge(&id))
        .await
        .map_err(|_| CoreError::message("Could not save the report review."))??;
    Ok(())
}

#[tauri::command]
pub(super) async fn open_diagnostic_issue(title: String, body: String) -> Result<(), AppError> {
    let url = issue_url(&title, &body)?;
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = tokio::process::Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = tokio::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    };
    let status = tokio::time::timeout(Duration::from_secs(10), command.kill_on_drop(true).status())
        .await
        .map_err(|_| {
            CoreError::message("Opening GitHub timed out. You can copy the report instead.")
        })??;
    if !status.success() {
        return Err(
            CoreError::message("Could not open GitHub. You can copy the report instead.").into(),
        );
    }
    Ok(())
}

fn issue_url(title: &str, body: &str) -> Result<String, CoreError> {
    if title.trim().is_empty() || title.chars().count() > 200 {
        return Err(CoreError::message(
            "Use an issue title between 1 and 200 characters.",
        ));
    }
    let mut url = url::Url::parse(ISSUE_URL).expect("fixed GitHub issue URL");
    url.query_pairs_mut()
        .append_pair("title", title)
        .append_pair("body", body);
    if url.as_str().len() > 8_000 {
        return Err(CoreError::message(
            "This report is too long for a GitHub link. Copy the report instead.",
        ));
    }
    Ok(url.into())
}

fn make_report(
    state: &Diagnostics,
    version: &str,
    logs: Option<&Path>,
    current_session: bool,
    now: SystemTime,
) -> DiagnosticReport {
    let crash = logs.and_then(|directory| latest_crash(directory, now));
    let current = state.query.lock().ok().and_then(|query| query.clone());
    let (query, from_crash) = if current_session && current.is_some() {
        (current, false)
    } else if let Some(crash) = &crash {
        (state.crash_query(crash), true)
    } else {
        (current, false)
    };
    let query = query.map(|query| ReportQuery {
        sql: query.sql,
        phase: query.phase,
        truncated: query.truncated,
        started_at: query.started_at,
        from_crash,
    });
    let mut body = format!(
        "## What happened\n\nDescribe what you were doing and how to reproduce the problem.\n\n## Diagnostics\n\n- Catabase: {version}\n- Platform: {} / {}\n- Build: {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    let mut title = "Catabase problem report".to_string();
    let mut note = if logs.is_some() {
        "No recent macOS crash report was found. You can still report a problem."
    } else {
        "Automatic native crash detection is available on macOS. You can still report a problem."
    }
    .to_string();
    let mut new_crash = false;
    if let Some(crash) = &crash {
        new_crash = !state.acknowledged().contains(&crash.id);
        title = format!("Crash: {} (Catabase {})", crash.exception, crash.version);
        body.push_str(&format!(
            "- Crashed version: {}\n- Crash time (UTC): {}\n{}",
            crash.version,
            crash.time.format("%Y-%m-%d %H:%M:%S"),
            crash.details
        ));
        note = "A recent macOS crash report is available. Review the report before sharing.".into();
    }
    DiagnosticReport {
        id: crash.map(|crash| crash.id),
        new_crash,
        title,
        body,
        query,
        note,
    }
}

fn recent_files(directory: &Path, prefix: &str, extension: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(directory) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.starts_with(prefix)
                && entry.path().extension().and_then(|ext| ext.to_str()) == Some(extension)
                && let Ok(metadata) = entry.metadata()
                && metadata.is_file()
                && !entry.file_type().is_ok_and(|kind| kind.is_symlink())
            {
                files.push((
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    entry.path(),
                ));
            }
        }
    }
    files.sort_unstable_by_key(|file| std::cmp::Reverse(file.0));
    files.into_iter().map(|(_, path)| path).collect()
}

fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > limit {
        return None;
    }
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data).ok()?;
    (data.len() as u64 <= limit).then_some(data)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Option<T> {
    serde_json::from_slice(&read_bounded(path, limit)?).ok()
}

fn write_private(path: &Path, value: &impl Serialize) -> Result<(), CoreError> {
    let bytes = serde_json::to_vec(value)?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        options.open(&temp)?.write_all(&bytes)?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    Ok(result?)
}

fn latest_crash(directory: &Path, now: SystemTime) -> Option<Crash> {
    recent_files(directory, "catabase-", "ips")
        .into_iter()
        .take(30)
        .filter_map(|path| read_bounded(&path, MAX_LOG_BYTES))
        .filter_map(|bytes| parse_crash(&bytes))
        .filter(|crash| {
            now.duration_since(crash.time.into())
                .is_ok_and(|age| age <= REPORT_AGE)
        })
        .max_by_key(|crash| crash.time)
}

fn parse_crash(bytes: &[u8]) -> Option<Crash> {
    let text = std::str::from_utf8(bytes).ok()?;
    let (_, report) = text.split_once('\n')?;
    let report: Value = serde_json::from_str(report).ok()?;
    if report.pointer("/bundleInfo/CFBundleIdentifier")?.as_str()? != "com.catabase.app" {
        return None;
    }
    let time = DateTime::parse_from_str(
        report.get("captureTime")?.as_str()?,
        "%Y-%m-%d %H:%M:%S%.f %z",
    )
    .ok()?
    .with_timezone(&Utc);
    let version = technical(
        report
            .pointer("/bundleInfo/CFBundleShortVersionString")
            .and_then(Value::as_str),
        false,
    );
    let exception = technical(
        report.pointer("/exception/type").and_then(Value::as_str),
        false,
    );
    let signal = technical(
        report.pointer("/exception/signal").and_then(Value::as_str),
        false,
    );
    let os = technical(
        report.pointer("/osVersion/train").and_then(Value::as_str),
        false,
    );
    let mut details = format!("- OS at crash: {os}\n- Exception: {exception} ({signal})\n");
    if let Some(code) = report.pointer("/termination/code").and_then(Value::as_i64) {
        let namespace = technical(
            report
                .pointer("/termination/namespace")
                .and_then(Value::as_str),
            false,
        );
        details.push_str(&format!("- Termination: {namespace}, code {code}\n"));
    }
    let images = report.get("usedImages").and_then(Value::as_array);
    if let (Some(threads), Some(images)) = (report.get("threads").and_then(Value::as_array), images)
    {
        let fault = report
            .get("faultingThread")
            .and_then(Value::as_u64)
            .map(|index| index as usize)
            .or_else(|| {
                threads.iter().position(|thread| {
                    thread.get("triggered").and_then(Value::as_bool) == Some(true)
                })
            });
        if let Some(thread) = fault.and_then(|index| threads.get(index)) {
            details.push_str(&format!(
                "\n### Crashed thread\n\n```text\n{}```\n",
                stack(thread, images, 24)
            ));
        }
        let mut seen = HashSet::new();
        for (index, thread) in threads
            .iter()
            .enumerate()
            .filter(|(index, _)| Some(*index) != fault)
        {
            let frames = thread.get("frames").and_then(Value::as_array);
            let in_app = frames.is_some_and(|frames| {
                frames.iter().any(|frame| {
                    frame
                        .get("imageIndex")
                        .and_then(Value::as_u64)
                        .and_then(|index| images.get(index as usize))
                        .and_then(|image| image.get("name"))
                        .and_then(Value::as_str)
                        == Some("catabase")
                })
            });
            if !in_app {
                continue;
            }
            let trace = stack(thread, images, 14);
            if !seen.insert(trace.clone()) {
                continue;
            }
            details.push_str(&format!(
                "\n### Background thread {index}\n\n```text\n{trace}```\n"
            ));
            if seen.len() >= 6 {
                break;
            }
        }
    }
    Some(Crash {
        id: format!("{:x}", Sha256::digest(bytes)),
        time,
        pid: report
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok()),
        launched_at: report
            .get("procLaunch")
            .and_then(Value::as_str)
            .and_then(|time| DateTime::parse_from_str(time, "%Y-%m-%d %H:%M:%S%.f %z").ok())
            .map(|time| time.with_timezone(&Utc)),
        version,
        exception,
        details,
    })
}

// Crash messages, paths, registers, UUIDs, thread names and memory contents are
// intentionally excluded. Symbols are binary function names, never log messages.
fn technical(value: Option<&str>, symbol: bool) -> String {
    let Some(value) = value else {
        return "unknown".into();
    };
    if value.len() > 240
        || !value.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || " ._+-()".contains(ch)
                || (symbol && ":<>[],$*&~".contains(ch))
        })
    {
        return "[omitted]".into();
    }
    value.to_string()
}

fn stack(thread: &Value, images: &[Value], limit: usize) -> String {
    let mut text = String::new();
    if let Some(frames) = thread.get("frames").and_then(Value::as_array) {
        for (index, frame) in frames.iter().take(limit).enumerate() {
            let image = frame
                .get("imageIndex")
                .and_then(Value::as_u64)
                .and_then(|index| images.get(index as usize));
            let name = technical(
                image
                    .and_then(|image| image.get("name"))
                    .and_then(Value::as_str),
                false,
            );
            let symbol = frame
                .get("symbol")
                .and_then(Value::as_str)
                .map(|symbol| technical(Some(symbol), true))
                .unwrap_or_else(|| {
                    format!(
                        "image offset 0x{:x}",
                        frame
                            .get("imageOffset")
                            .and_then(Value::as_u64)
                            .unwrap_or(0)
                    )
                });
            text.push_str(&format!("{index:>2} {name}: {symbol}\n"));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "catabase-diagnostics-test-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn log(bundle: &str) -> Vec<u8> {
        let body = json!({
            "bundleInfo": {"CFBundleIdentifier": bundle, "CFBundleShortVersionString": "0.1.3"},
            "captureTime": "2026-11-19 11:35:56.6756 +0100", "pid": 123,
            "procPath": "/Users/private-person/Applications/Catabase.app", "incident": "private-device-id",
            "asi": {"message": "password=do-not-share SELECT secret FROM private_table"},
            "exception": {"type": "EXC_BAD_ACCESS", "signal": "SIGSEGV", "subtype": "private data at 0xff00"},
            "termination": {"namespace": "SIGNAL", "code": 11},
            "osVersion": {"train": "macOS 15.6"}, "faultingThread": 0,
            "usedImages": [{"name": "AppKit", "path": "/private-path", "uuid": "private-id"}, {"name": "catabase"}],
            "threads": [
                {"triggered": true, "name": "private-thread-name", "frames": [{"imageIndex": 0, "symbol": "-[NSPasteboard changeCount]"}, {"imageIndex": 1, "imageOffset": 512}]},
                {"frames": [{"imageIndex": 0, "symbol": "-[NSPasteboard writeObjects:]"}, {"imageIndex": 1, "imageOffset": 1024}]},
                {"frames": [{"imageIndex": 0, "symbol": "/Users/private-person/secret"}, {"imageIndex": 1, "imageOffset": 2048}]}
            ]
        });
        format!("{{\"name\":\"catabase\",\"user\":\"private-person\"}}\n{body}").into_bytes()
    }

    #[test]
    fn crash_summary_keeps_both_clipboard_threads_and_excludes_private_fields() {
        let crash = parse_crash(&log("com.catabase.app")).unwrap();
        assert!(crash.details.contains("EXC_BAD_ACCESS (SIGSEGV)"));
        assert!(crash.details.contains("-[NSPasteboard changeCount]"));
        assert!(crash.details.contains("-[NSPasteboard writeObjects:]"));
        assert!(crash.details.contains("image offset 0x200"));
        for private in [
            "private", "password", "SELECT", "secret", "0xff00", "uuid", "procPath",
        ] {
            assert!(!crash.details.contains(private), "leaked {private}");
        }
        assert!(parse_crash(&log("com.other.app")).is_none());
        assert!(parse_crash(b"not a crash report").is_none());
        assert!(parse_crash(b"{}\n{}").is_none());
    }

    #[test]
    fn startup_report_is_acknowledged_once_and_can_be_reopened_without_sql_in_body() {
        let temp = Fixture::new();
        let state = Diagnostics::new(&temp.0);
        let logs = temp.0.join("logs");
        fs::create_dir(&logs).unwrap();
        fs::write(logs.join("catabase-fixture.ips"), log("com.catabase.app")).unwrap();
        let now: SystemTime = DateTime::parse_from_rfc3339("2026-11-19T11:00:00Z")
            .unwrap()
            .into();
        let first = make_report(&state, "0.1.4", Some(&logs), false, now);
        assert!(first.new_crash);
        state.acknowledge(first.id.as_ref().unwrap()).unwrap();
        let reopened = make_report(&Diagnostics::new(&temp.0), "0.1.4", Some(&logs), false, now);
        assert!(!reopened.new_crash);
        assert_eq!(first.body, reopened.body);
        assert!(!reopened.body.contains("SELECT"));
        assert!(state.acknowledge("../../outside").is_err());
        assert!(!make_report(&state, "0.1.4", Some(&logs), false, now + REPORT_AGE).new_crash);
    }

    #[test]
    fn crash_sql_matches_process_and_time_and_survives_restart() {
        let temp = Fixture::new();
        let state = Diagnostics::new(&temp.0);
        let crash = parse_crash(&log("com.catabase.app")).unwrap();
        let context = QueryContext {
            sql: "select 'private sql'".into(),
            phase: "running".into(),
            truncated: false,
            pid: 123,
            started_at: crash.time - chrono::Duration::seconds(10),
        };
        write_private(&state.context_file, &context).unwrap();
        let restarted = Diagnostics::new(&temp.0);
        assert_eq!(restarted.crash_query(&crash).unwrap().sql, context.sql);
        let mut wrong = context.clone();
        wrong.pid = 124;
        write_private(&state.context_file, &wrong).unwrap();
        assert!(restarted.crash_query(&crash).is_none());
        wrong.pid = 123;
        wrong.started_at = crash.time + chrono::Duration::seconds(1);
        write_private(&state.context_file, &wrong).unwrap();
        assert!(restarted.crash_query(&crash).is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&state.context_file)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn missing_or_oversized_logs_do_not_block_manual_reporting() {
        let temp = Fixture::new();
        let state = Diagnostics::new(&temp.0);
        let missing = temp.0.join("missing");
        let report = make_report(&state, "0.1.4", Some(&missing), false, SystemTime::now());
        assert!(report.id.is_none());
        assert!(!report.new_crash);
        assert!(report.body.contains("Catabase: 0.1.4"));
        let large = temp.0.join("catabase-large.ips");
        File::create(&large)
            .unwrap()
            .set_len(MAX_LOG_BYTES + 1)
            .unwrap();
        assert!(latest_crash(&temp.0, SystemTime::now()).is_none());
    }

    #[test]
    fn manual_reports_can_include_current_sql_without_mislabeling_it_as_crash_sql() {
        let temp = Fixture::new();
        let state = Diagnostics::new(&temp.0);
        let logs = temp.0.join("logs");
        fs::create_dir(&logs).unwrap();
        fs::write(logs.join("catabase-fixture.ips"), log("com.catabase.app")).unwrap();
        let crash = parse_crash(&log("com.catabase.app")).unwrap();
        let old = QueryContext {
            sql: "select 'at crash'".into(),
            phase: "running".into(),
            truncated: false,
            pid: 123,
            started_at: crash.time - chrono::Duration::seconds(1),
        };
        write_private(&state.context_file, &old).unwrap();
        *state.query.lock().unwrap() = Some(QueryContext {
            sql: "select 'current private SQL'".into(),
            phase: "running".into(),
            truncated: false,
            pid: 456,
            started_at: crash.time + chrono::Duration::seconds(1),
        });
        let now: SystemTime = (crash.time + chrono::Duration::seconds(10)).into();
        let automatic = make_report(&state, "0.1.4", Some(&logs), false, now);
        assert!(automatic.query.as_ref().unwrap().from_crash);
        assert_eq!(automatic.query.as_ref().unwrap().sql, old.sql);
        let manual = make_report(&state, "0.1.4", Some(&logs), true, now);
        assert!(!manual.query.as_ref().unwrap().from_crash);
        assert!(
            manual
                .query
                .as_ref()
                .unwrap()
                .sql
                .contains("current private SQL")
        );
        assert!(!manual.body.contains("private SQL"));
        assert!(!manual.body.contains(&old.sql));
        assert!(!serde_json::to_string(&manual).unwrap().contains("\"pid\""));
    }

    #[test]
    fn issue_links_use_fixed_repo_and_encode_untrusted_text_as_data() {
        let title = "Crash & details #1";
        let body = "User notes\nhttps://example.com/?x=1&labels=admin\n`$(open)`";
        let url = url::Url::parse(&issue_url(title, body).unwrap()).unwrap();
        assert_eq!(url.host_str(), Some("github.com"));
        assert_eq!(url.path(), "/smolijar/catabase/issues/new");
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            [("title".into(), title.into()), ("body".into(), body.into())]
        );
        assert!(issue_url("", "report").is_err());
        assert!(issue_url("Crash", &"🦀".repeat(1000)).is_err());
    }
}

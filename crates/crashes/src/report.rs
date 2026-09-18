//! Opt-in crash reporting to Sentry.
//!
//! On startup, [`report_pending`] scans the logs directory for `CrashInfo`
//! JSON files that have not been acknowledged (`*.sent` sidecar missing) and
//! uploads each as a Sentry event. Reporting runs only when the user has
//! enabled telemetry in settings **and** a DSN is configured; otherwise the
//! reports stay on disk untouched.

use crate::CrashInfo;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Parses a Sentry DSN (`https://<key>@<host>/<project>`) into the envelope
/// endpoint URL and the auth header value.
fn parse_dsn(dsn: &str) -> Option<(String, String)> {
    let rest = dsn.strip_prefix("https://")?;
    let (key, tail) = rest.split_once('@')?;
    let (host, project) = tail.split_once('/')?;
    if key.is_empty() || host.is_empty() || project.is_empty() {
        return None;
    }
    let url = format!("https://{host}/api/{project}/envelope/");
    let auth = format!("Sentry sentry_version=7, sentry_key={key}, sentry_client=bit7zfm/0.1.0");
    Some((url, auth))
}

/// Scans `logs_dir` for unacknowledged crash reports and uploads them.
/// `app_version`/`release_channel` tag the events. Returns the number of
/// reports sent.
pub fn report_pending(
    logs_dir: &Path,
    dsn: &str,
    app_version: &str,
    release_channel: &str,
) -> usize {
    let Some((url, auth)) = parse_dsn(dsn) else {
        return 0;
    };
    let mut sent = 0;
    for info_path in pending_reports(logs_dir) {
        match std::fs::read_to_string(&info_path)
            .map_err(|e| e.to_string())
            .and_then(|json| serde_json::from_str::<CrashInfo>(&json).map_err(|e| e.to_string()))
        {
            Ok(info) => {
                let payload = envelope(&info, app_version, release_channel);
                match post(&url, &auth, &payload) {
                    Ok(()) => {
                        acknowledge(&info_path);
                        sent += 1;
                        log::info!("crash report uploaded: {}", info_path.display());
                    }
                    Err(err) => {
                        log::warn!("crash report upload failed: {err}");
                        // Keep the report on disk; retried on the next launch.
                    }
                }
            }
            Err(err) => {
                // Unparseable report files would be retried forever —
                // acknowledge them so they stop blocking the queue.
                log::warn!("crash report unreadable, discarding: {err}");
                acknowledge(&info_path);
            }
        }
    }
    sent
}

fn pending_reports(logs_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(logs_dir) else {
        return Vec::new();
    };
    let mut pending = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json")
            && !path.with_extension("sent").exists()
        {
            pending.push(path);
        }
    }
    pending.sort();
    pending
}

fn acknowledge(info_path: &Path) {
    if let Ok(mut sent) = std::fs::File::create(info_path.with_extension("sent")) {
        let _ = sent.write_all(b"");
    }
}

/// Builds a Sentry v7 envelope: one header line, one item header, the event.
fn envelope(info: &CrashInfo, app_version: &str, release_channel: &str) -> String {
    let event_id = uuid_v4();
    let event = serde_json::json!({
        "event_id": event_id.replace('-', ""),
        "timestamp": iso_timestamp(),
        "platform": "native",
        "level": "error",
        "environment": release_channel,
        "release": format!("bit7zfm@{app_version}"),
        "message": {"formatted": describe(info)},
        "extra": {"crash": info},
    });
    let event_json = serde_json::to_string(&event).unwrap_or_default();
    format!(
        "{{\"event_id\":\"{event_id}\",\"sent_at\":\"{}\"}}\n{{\"type\":\"event\",\"length\":{}}}\n{}\n",
        iso_timestamp(),
        event_json.len(),
        event_json
    )
}

fn describe(info: &CrashInfo) -> String {
    if let Some(panic) = &info.panic {
        return format!("panic: {} ({})", panic.message, panic.span);
    }
    if let Some(abort) = &info.abort_message {
        return format!("abort: {abort}");
    }
    if let Some(err) = &info.minidump_error {
        return format!("minidump error: {err}");
    }
    "crash".into()
}

fn post(url: &str, auth: &str, body: &str) -> Result<(), String> {
    let response = ureq::post(url)
        .set("X-Sentry-Auth", auth)
        .set("Content-Type", "application/x-sentry-envelope")
        .timeout(std::time::Duration::from_secs(15))
        .send_string(body);
    match response {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) if (200..300).contains(&code) => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

fn iso_timestamp() -> String {
    // No chrono in this crate: a plain unix-seconds value is accepted by
    // Sentry as a float timestamp; keep the envelope simple and numeric.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

fn uuid_v4() -> String {
    // Random enough for event ids: timestamp + address entropy, hex formatted.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let addr = &nanos as *const u128 as usize;
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (nanos >> 32) as u32,
        (nanos >> 16) as u16,
        nanos as u16 & 0x0FFF,
        (addr >> 8) as u16 & 0x0FFF,
        (nanos as u64) ^ (addr as u64)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsn_parsing() {
        let (url, auth) =
            parse_dsn("https://abc123@o42.ingest.sentry.io/777").expect("parses");
        assert_eq!(url, "https://o42.ingest.sentry.io/api/777/envelope/");
        assert!(auth.contains("sentry_key=abc123"));
        assert!(parse_dsn("not-a-dsn").is_none());
        assert!(parse_dsn("https://nohost").is_none());
    }

    #[test]
    fn envelope_contains_event_and_header() {
        let info = serde_json::from_str::<CrashInfo>(
            r#"{"init":{"session_id":"1","zed_version":"0.1.0","binary":"bit7zfm","release_channel":"Dev","commit_sha":"x"},"panic":{"message":"test panic","span":"main.rs:1"},"minidump_error":null,"abort_message":null,"gpus":[],"active_gpu":null,"user_info":null}"#,
        )
        .unwrap();
        let envelope = envelope(&info, "0.1.0", "Dev");
        assert!(envelope.contains("\"type\":\"event\""));
        assert!(envelope.contains("bit7zfm@0.1.0"));
        assert!(envelope.contains("panic:"));
    }
}

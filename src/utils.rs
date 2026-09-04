/// Returns the current UTC time as an ISO 8601 string (e.g. `2025-01-02T15:04:05Z`).
pub fn iso_timestamp() -> String {
    iso_timestamp_at(unix_now_secs())
}

/// Seconds since the Unix epoch, saturating to 0 if the clock is before it.
pub fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Formats `secs` seconds since the Unix epoch as an ISO 8601 string
/// (e.g. `2025-01-02T15:04:05Z`).
///
/// Implemented via Howard Hinnant's civil-date algorithm to avoid pulling in a
/// date-time dependency. The output is fixed-width UTC, so two of these
/// strings compare chronologically under plain lexicographic ordering.
pub fn iso_timestamp_at(secs: u64) -> String {
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let z = days as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Decode a lowercase / uppercase hex string into bytes. Returns `None`
/// for odd-length input or any non-hex character. Used by webhook HMAC
/// signature verification.
#[cfg(feature = "serve")]
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Best-effort text of a panic payload, for logging.
///
/// `catch_unwind` and `JoinError::into_panic` both hand back a
/// `Box<dyn Any + Send>`. In practice it holds a `&'static str` (from
/// `panic!("literal")`) or a `String` (from a formatted panic); anything
/// else is reported as such rather than dropped silently.
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic payload was not a string".to_string()
    }
}

/// Report a background task that did not end normally, at a level that
/// matches what happened.
///
/// A panicking `tokio::spawn`ed task prints nothing by itself: the payload is
/// parked in its `JoinHandle`, and a join site that discards the `Result`
/// throws it away. Routing every join through here means a panic reaches the
/// log at `error` with its message attached. Cancellation is an ordinary part
/// of shutdown and stays at `debug`.
pub fn log_join_error(context: &str, err: tokio::task::JoinError) {
    if err.is_cancelled() {
        tracing::debug!("{context}: background task cancelled");
        return;
    }
    match err.try_into_panic() {
        Ok(payload) => tracing::error!(
            "{context}: background task PANICKED: {}",
            panic_message(payload.as_ref())
        ),
        Err(err) => tracing::error!("{context}: background task ended abnormally: {err}"),
    }
}

/// Logs at `error` level if it is dropped while the thread is unwinding.
///
/// Hold one at the top of a spawned future when its `JoinHandle` is not
/// polled until shutdown. Without it, a panic mid-run is silent for as long
/// as the process keeps running: the task is gone, nothing joins it, and the
/// first sign is whatever stops happening. The panic payload itself still
/// arrives at the join site through [`log_join_error`]; this only makes the
/// death visible at the moment it happens.
pub struct PanicLogGuard {
    context: &'static str,
}

impl PanicLogGuard {
    pub fn new(context: &'static str) -> Self {
        Self { context }
    }
}

impl Drop for PanicLogGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            tracing::error!("{}: background task PANICKED", self.context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_timestamp_at_is_fixed_width_and_orders_lexicographically() {
        // /health compares ISO timestamps as plain strings, which only works
        // because the format is fixed-width UTC.
        let earlier = iso_timestamp_at(1_756_900_000);
        let later = iso_timestamp_at(1_756_900_900);
        assert_eq!(earlier.len(), 20);
        assert_eq!(later.len(), 20);
        assert!(earlier < later);
    }

    #[test]
    fn panic_message_reads_str_and_string_payloads() {
        let literal: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(literal.as_ref()), "boom");

        let formatted: Box<dyn std::any::Any + Send> = Box::new("boom 42".to_string());
        assert_eq!(panic_message(formatted.as_ref()), "boom 42");

        let other: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(
            panic_message(other.as_ref()),
            "panic payload was not a string"
        );
    }

    #[tokio::test]
    async fn panic_payload_survives_the_join_handle() {
        // The end-to-end path the join sites rely on: a spawned task panics,
        // the payload is parked in the JoinHandle, and the message comes back
        // out instead of being discarded.
        let handle = tokio::spawn(async {
            panic!("scheduler exploded: {}", 7);
        });
        let err = handle.await.expect_err("task should have panicked");
        assert!(!err.is_cancelled());
        let payload = err.into_panic();
        assert_eq!(panic_message(payload.as_ref()), "scheduler exploded: 7");
    }

    #[tokio::test]
    async fn cancelled_tasks_are_not_reported_as_panics() {
        let handle = tokio::spawn(async {
            std::future::pending::<()>().await;
        });
        handle.abort();
        let err = handle.await.expect_err("task should have been cancelled");
        assert!(err.is_cancelled());
        // Exercises the debug-level branch; it must not panic on a
        // cancellation JoinError, which has no payload to unwrap.
        log_join_error("test", err);
    }

    #[test]
    fn iso_timestamp_format() {
        let ts = iso_timestamp();
        assert!(ts.len() == 20, "unexpected length: {ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    #[cfg(feature = "serve")]
    fn hex_decode_valid() {
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex_decode(""), Some(vec![]));
        assert_eq!(hex_decode("00ff"), Some(vec![0x00, 0xff]));
    }

    #[test]
    #[cfg(feature = "serve")]
    fn hex_decode_invalid() {
        assert_eq!(hex_decode("xyz"), None); // odd length
        assert_eq!(hex_decode("zz"), None); // non-hex chars
        assert_eq!(hex_decode("abc"), None); // odd length
    }
}

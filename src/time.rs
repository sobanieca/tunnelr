//! Clock helpers: milliseconds since the epoch, local `HH:MM:SS` for log
//! lines and ISO 8601 UTC timestamps for the HTTP API.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Local time of day as `HH:MM:SS`.
pub fn local_clock() -> String {
    let seconds = (now_ms() / 1000).try_into().unwrap_or_default();
    // SAFETY: localtime_r only writes to the provided struct.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&seconds, &mut tm).is_null() };
    if !ok {
        let (_, _, _, h, m, s) = civil_from_ms(now_ms());
        return format!("{h:02}:{m:02}:{s:02}");
    }
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// (year, month, day, hour, minute, second) in UTC.
fn civil_from_ms(ms: u64) -> (i64, u32, u32, u32, u32, u32) {
    let seconds = (ms / 1000) as i64;
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, rest / 3600, rest / 60 % 60, rest % 60)
}

/// `new Date(ms).toISOString()`
pub fn iso_string(ms: u64) -> String {
    let (year, month, day, hour, minute, second) = civil_from_ms(ms);
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_iso_dates() {
        assert_eq!(iso_string(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_string(1_709_210_096_789), "2024-02-29T12:34:56.789Z");
        assert_eq!(iso_string(4_102_444_799_000), "2099-12-31T23:59:59.000Z");
    }
}

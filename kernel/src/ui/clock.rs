//! Turning a count of seconds into a date and time (no OS to ask!).

pub struct DateTime { pub year: i32, pub month: u32, pub day: u32, pub hour: u32, pub min: u32, pub sec: u32, pub weekday: u32 }

/// Unix seconds (since 1 Jan 1970 UTC) -> calendar date. This is Howard
/// Hinnant's "days from civil" algorithm, in reverse.
pub fn from_unix(t: i64) -> DateTime {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400) as u32;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
    let weekday = ((days + 4).rem_euclid(7)) as u32; // 1 Jan 1970 was a Thursday
    DateTime { year, month, day, hour: secs / 3600, min: secs / 60 % 60, sec: secs % 60, weekday }
}

pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

//! Numbers and times as a view reads them out: grouped digits, an
//! avatar's initial, and the reader's dates and clock.

/// `6230` → `6,230`.
pub fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// An avatar's letter: the first grapheme of `name`, uppercased where
/// that applies (`alice` → `A`, `김민지` → `김`), else `•`.
pub fn initial(name: &str) -> String {
    unicode_segmentation::UnicodeSegmentation::graphemes(name.trim_start(), true)
        .next()
        .map_or_else(|| "•".into(), str::to_uppercase)
}

thread_local! {
    static UTC_OFFSET_MINUTES: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

pub(crate) fn set_utc_offset(minutes: i32) {
    UTC_OFFSET_MINUTES.set(minutes);
}

/// A UTC time in milliseconds shifted into the reader's zone: the instant
/// whose UTC reading is the reader's wall clock. Day arithmetic on it
/// (`local(t) / 86_400_000`) falls on the reader's midnights.
pub fn local(millis: u64) -> u64 {
    let shift = i64::from(UTC_OFFSET_MINUTES.get()) * 60_000;
    millis.saturating_add_signed(shift)
}

/// A time in milliseconds as the reader's date: `24 Sep 2026, 05:12:07`.
pub fn date(millis: u64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let seconds = local(millis) / 1000;
    let (days, of_day) = (seconds / 86_400, seconds % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{day} {} {year}, {:02}:{:02}:{:02}",
        MONTHS[(month - 1) as usize],
        of_day / 3_600,
        of_day % 3_600 / 60,
        of_day % 60
    )
}

/// How long before `now` a time in milliseconds was: `2s`, `3m`, `4h`, `5d`.
pub fn ago(now: u64, then: u64) -> String {
    let seconds = now.saturating_sub(then) / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3_600 => format!("{}m", seconds / 60),
        3_600..86_400 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// A time in milliseconds as the reader's day: `24 Sep 2026`.
pub fn day(millis: u64) -> String {
    let date = date(millis);
    date.split_once(", ")
        .map_or(date.clone(), |(day, _)| day.to_owned())
}

/// A time in milliseconds as the reader's clock time: `3:42 PM`.
pub fn clock(millis: u64) -> String {
    let minutes = local(millis) / 60_000 % 1_440;
    let (hour, minute) = (minutes / 60, minutes % 60);
    let half = if hour < 12 { "AM" } else { "PM" };
    format!("{}:{minute:02} {half}", (hour + 11) % 12 + 1)
}

/// `1 block`, `1,200 blocks`.
pub fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{} {}", grouped(count), if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_grouped_and_agreed() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(6230), "6,230");
        assert_eq!(grouped(1_048_576), "1,048,576");
        assert_eq!(plural(1, "block", "blocks"), "1 block");
        assert_eq!(plural(1200, "block", "blocks"), "1,200 blocks");
    }

    #[test]
    fn a_time_reads_as_its_day_and_clock() {
        let at = 1_790_264_527_000;
        assert_eq!(date(at), "24 Sep 2026, 15:42:07");
        assert_eq!(day(at), "24 Sep 2026");
        assert_eq!(clock(at), "3:42 PM");
        assert_eq!(clock(0), "12:00 AM");
        assert_eq!(clock(12 * 3_600_000 + 5 * 60_000), "12:05 PM");
    }

    #[test]
    fn a_utc_instant_reads_in_the_readers_offset() {
        let at = 1_790_264_527_000;
        set_utc_offset(540);
        assert_eq!(date(at), "25 Sep 2026, 00:42:07");
        assert_eq!(day(at), "25 Sep 2026");
        assert_eq!(clock(at), "12:42 AM");
        set_utc_offset(-330);
        assert_eq!(clock(at), "10:12 AM");
        set_utc_offset(-60);
        assert_eq!(clock(0), "12:00 AM");
        set_utc_offset(0);
        assert_eq!(clock(at), "3:42 PM");
    }

    #[test]
    fn an_initial_is_the_first_grapheme() {
        assert_eq!(initial("alice park"), "A");
        assert_eq!(initial("김민지"), "김");
        assert_eq!(initial(" 한글"), "한");
        assert_eq!(initial("e\u{301}va"), "E\u{301}");
        assert_eq!(initial(""), "•");
    }
}

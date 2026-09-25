pub struct TimeFormatter;

impl TimeFormatter {
    pub fn parse_rfc3339_secs(ts: &str) -> Option<u64> {
        let clean = ts.trim_end_matches('Z');
        let (date_part, time_part) = clean.split_once('T').or_else(|| clean.split_once(' '))?;
        let mut d_iter = date_part.split('-');
        let (y, m, d) = (
            d_iter.next()?.parse::<i32>().ok()?,
            d_iter.next()?.parse::<u32>().ok()?,
            d_iter.next()?.parse::<u32>().ok()?,
        );
        let mut t_iter = time_part.split(':');
        let (h, min) = (
            t_iter.next()?.parse::<u32>().ok()?,
            t_iter.next()?.parse::<u32>().ok()?,
        );
        let sec = t_iter.next()?.split('.').next()?.parse::<u32>().ok()?;

        let days = civil_to_days(y, m, d);
        let secs = (days as i64 * 86400) + (h as i64 * 3600) + (min as i64 * 60) + sec as i64;
        if secs >= 0 { Some(secs as u64) } else { None }
    }

    pub fn normalize_rfc3339(ts: &str) -> String {
        let clean = ts.trim_end_matches('Z');
        if let Some((main, frac)) = clean.split_once('.') {
            let digits: String = frac.chars().filter(|c| c.is_ascii_digit()).collect();
            let mut padded = digits;
            padded.truncate(9);
            while padded.len() < 9 {
                padded.push('0');
            }
            format!("{}.{}Z", main, padded)
        } else {
            format!("{}.000000000Z", clean)
        }
    }

    pub fn unix_nanos_to_rfc3339(nanos: i64) -> String {
        let secs = nanos / 1_000_000_000;
        let nsec = (nanos % 1_000_000_000).abs();
        let days = (secs / 86400) + 719468;
        let time = (secs % 86400).rem_euclid(86400);

        let era = if days >= 0 { days } else { days - 146096 } / 146097;
        let doe = (days - era * 146097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
        let y = if m <= 2 { y + 1 } else { y };

        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z", y, m, d, time / 3600, (time % 3600) / 60, time % 60, nsec)
    }

    pub fn format_hh_mm(ts: &str) -> Option<String> {
        let clean = ts.trim_end_matches('Z');
        let (_, time_part) = clean.split_once('T').or_else(|| clean.split_once(' '))?;
        let mut parts = time_part.split(':');
        Some(format!("{}:{}", parts.next()?, parts.next()?))
    }

    pub fn format_duration(diff_secs: u64) -> String {
        if diff_secs < 60 {
            format!("{}s", diff_secs)
        } else if diff_secs < 3600 {
            format!("{}m", diff_secs / 60)
        } else if diff_secs < 86400 {
            let (h, m) = (diff_secs / 3600, (diff_secs % 3600) / 60);
            if m == 0 { format!("{}h", h) } else { format!("{}h {}m", h, m) }
        } else {
            format!("{}d {}h", diff_secs / 86400, (diff_secs % 86400) / 3600)
        }
    }
}

fn civil_to_days(mut y: i32, m: u32, d: u32) -> i32 {
    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i32 - 719468
}

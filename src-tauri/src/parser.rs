//! Value parsing per Goal.md "Value parsing" section.
//!
//! All OCR'd strings flow through here. Coin values are normalized to base
//! units per minute; wave and tier are plain integers.

/// Result of classifying the coin line per the shared rule in Goal.md:
/// `/min` suffix -> Rate, bare number+suffix -> Total (do not update
/// coin_per_minute), anything else -> Unreadable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CoinReading {
    /// Coins per minute, normalized to base units (e.g. "1.23K/min" -> 1230.0)
    Rate(f64),
    /// Total coin balance shown instead of a rate (e.g. "27.46q")
    Total(f64),
    Unreadable,
}

/// Multiplier for a unit suffix.
///
/// Ordered table from Goal.md: index * 3 = exponent.
/// Single letters are case-sensitive (q != Q, s != S). After "D" (index 11),
/// two-letter lowercase suffixes continue the sequence: aa, ab, ... az, ba, ...
pub fn suffix_multiplier(suffix: &str) -> Option<f64> {
    const SINGLE: [&str; 12] = ["", "K", "M", "B", "T", "q", "Q", "s", "S", "O", "N", "D"];
    if let Some(idx) = SINGLE.iter().position(|s| *s == suffix) {
        return Some(10f64.powi(idx as i32 * 3));
    }
    let bytes = suffix.as_bytes();
    if bytes.len() == 2 && bytes.iter().all(|b| b.is_ascii_lowercase()) {
        let idx = 12 + (bytes[0] - b'a') as i32 * 26 + (bytes[1] - b'a') as i32;
        return Some(10f64.powi(idx * 3));
    }
    None
}

/// Coin-icon prefixes OCR'd from the in-game coin currency glyph.
pub fn has_coin_icon_prefix(raw: &str) -> bool {
    let t = raw.trim();
    [
        "@", "C ", "c ", "©", "G ", "(C)", "(c)", "(Cc)", "(cc)", "(CC)",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

/// Suffix letters used for total coin *balance* (not typical /min rates at
/// mid-game). OCR often appends a spurious "/min" to these, e.g. "@ 6.00q/min".
pub fn is_balance_tier_suffix(suffix: &str) -> bool {
    matches!(suffix, "q" | "Q" | "s" | "S" | "O" | "N" | "D")
        || (suffix.len() == 2 && suffix.bytes().all(|b| b.is_ascii_lowercase()))
}

/// Suffix letters valid for a bare coin-rate line without a coin icon (e.g.
/// anchor crop "85.8T/min"). M/B without a coin icon are almost always cash.
fn is_rate_tier_suffix(suffix: &str) -> bool {
    matches!(suffix, "" | "K" | "T")
}

/// Split numeric body into (value, suffix letters).
fn split_number_suffix(text: &str) -> Option<(f64, String)> {
    let mut text = text.trim().to_string();
    while text.starts_with(['O', 'o']) {
        text.replace_range(0..1, "0");
    }
    let split = text
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let (num_part, suffix) = text.split_at(split);
    let num: f64 = num_part.replace(',', "").trim().parse().ok()?;
    Some((num, suffix.trim().to_string()))
}

/// Reject coin/min readings that match total-balance patterns or cash lines.
fn is_plausible_rate(body: &str, raw: &str) -> bool {
    let Some((num, suffix)) = split_number_suffix(body) else {
        return false;
    };
    // Total coin on screen: "6.00q", "27.46q" — OCR sometimes adds "/min".
    if is_balance_tier_suffix(&suffix) && num < 100.0 {
        return false;
    }
    // Cash /min line ($ stripped by OCR): "6.9M/min" — not the coin rate.
    if !has_coin_icon_prefix(raw) && !is_rate_tier_suffix(&suffix) {
        return false;
    }
    true
}

/// Fix common OCR confusions inside numeric coin bodies (e.g. `3A8T` -> `348T`).
fn fix_digit_lookalikes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let prev_digit = i > 0 && chars[i - 1].is_ascii_digit();
        let next_digit = i + 1 < chars.len() && chars[i + 1].is_ascii_digit();
        out.push(match c {
            'A' | 'a' if prev_digit || next_digit => '4',
            'O' | 'o' if prev_digit || next_digit => '0',
            'S' | 's' if prev_digit || next_digit => '5',
            'l' | 'I' if prev_digit || next_digit => '1',
            _ => c,
        });
    }
    out
}

/// OCR may split decimals: "3 48T" or "3 A8T" -> "3.48T".
fn fix_spaced_decimal(body: &str) -> String {
    let trimmed = body.trim();
    if let Some(space) = trimmed.find(' ') {
        let (left, right) = trimmed.split_at(space);
        let left = left.trim();
        let right = fix_digit_lookalikes(right.trim_start().replace(' ', "").as_str());
        if left.chars().all(|c| c.is_ascii_digit()) && !left.is_empty() && left.len() <= 2 {
            if let Some(i) = right.find(|c: char| c.is_ascii_alphabetic()) {
                let (num, suffix) = right.split_at(i);
                if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                    return format!("{left}.{num}{suffix}");
                }
            }
        }
    }
    fix_digit_lookalikes(&trimmed.replace(' ', ""))
}

/// Normalize common OCR mangling of the `/min` suffix on coin-rate lines.
fn normalize_coin_rate_ocr(text: &str) -> String {
    let mut t = text.trim().to_string();
    let lower_start = t.to_lowercase();
    if (lower_start.starts_with("@ o/") || lower_start.starts_with("@ 0/"))
        && lower_start.contains("min")
    {
        return "0/min".to_string();
    }
    if lower_start == "o/min" || lower_start.starts_with("o/min") {
        return "0/min".to_string();
    }
    if lower_start.contains("04min") {
        return "0/min".to_string();
    }
    if t.starts_with(['x', 'X']) {
        return t;
    }
    for prefix in ["(Cc)", "(CC)", "(cc)", "(C)", "(c)"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim_start().to_string();
            break;
        }
    }
    while let Some(first) = t.chars().next() {
        if first.is_ascii_digit() || matches!(first, '@' | 'C' | 'c' | 'O' | 'o' | '0') {
            break;
        }
        let len = first.len_utf8();
        t = t[len..].trim_start().to_string();
    }

    if is_wave_progress_line(&t) {
        return t;
    }

    // OCR sometimes splits decimals: "3 48T/min" -> "3.48T/min"
    if t.contains('/') {
        let slash = t.find('/').unwrap();
        let body = t[..slash].trim();
        let fixed = fix_spaced_decimal(body);
        if parse_number_with_suffix(&fixed).is_some() {
            let suffix = &t[slash..];
            let lower_suffix = suffix.to_lowercase();
            // Keep well-formed /min lines; let junk suffixes fall through to fixups below.
            if lower_suffix.starts_with("/min") {
                return format!("{fixed}{suffix}");
            }
            if lower_suffix.starts_with("/mi") {
                return format!("{fixed}/min");
            }
            if lower_suffix == "/m" {
                return format!("{fixed}/min");
            }
        }
    }

    let lower = t.to_lowercase();
    // "(min" / "(mine" — OCR reads /min as parenthesized junk.
    if lower.contains("(mine") || lower.contains("(min") {
        if let Some(idx) = lower.find("(mi") {
            let mut body = t[..idx].trim().to_string();
            for prefix in ["@ ", "@", "C ", "c ", "(Cc) ", "(CC) ", "(cc) "] {
                if let Some(rest) = body.strip_prefix(prefix) {
                    body = rest.trim().to_string();
                    break;
                }
            }
            if body
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
                && !body.is_empty()
            {
                return format!("{body}T/min");
            }
            if parse_number_with_suffix(&body).is_some() {
                return format!("{body}/min");
            }
        }
    }
    // "3.48 trninz" — T dropped and glued to junk after a space.
    if let Some(idx) = lower.find(" tr") {
        let mut body = t[..idx].trim().to_string();
        for prefix in ["@ ", "@", "C ", "c "] {
            if let Some(rest) = body.strip_prefix(prefix) {
                body = rest.trim().to_string();
                break;
            }
        }
        if body
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
            && !body.is_empty()
        {
            return format!("{body}T/min");
        }
    }
    // Any slash suffix after a rate body: "70.6T/rtf", "3.48T/mi".
    if let Some(idx) = lower.find('/') {
        let body = t[..idx].trim();
        if parse_number_with_suffix(body).is_some() {
            return format!("{body}/min");
        }
    }
    // "62.4T1mi", "83.3TA+i" — suffix glued to junk before "mi".
    if lower.contains("mi") {
        for ch in ['t', 'm', 'b', 'k'] {
            if let Some(pos) = lower.rfind(ch) {
                let stem = &t[..=pos];
                if parse_number_with_suffix(stem).is_some() {
                    return format!("{stem}/min");
                }
            }
        }
    }
    // "/n'lin", "/nA1", "/ny" — OCR reads /min as /n…
    if let Some(idx) = lower.find("/n") {
        let body = t[..idx].trim();
        if parse_number_with_suffix(body).is_some() {
            return format!("{body}/min");
        }
    }
    if let Some(idx) = lower.find("/m") {
        let body = &t[..idx];
        let tail = &lower[idx + 2..];
        if tail.is_empty()
            || tail.starts_with('i')
            || tail.starts_with('n')
            || tail.starts_with('!')
            || tail.starts_with('f')
            || tail.starts_with('r')
            || tail.starts_with('t')
            || tail.starts_with('y')
            || tail.starts_with('l')
            || tail.starts_with('\'')
            || tail.starts_with('a')
            || tail.starts_with('(')
        {
            return format!("{body}/min");
        }
    }
    if let Some(idx) = lower.rfind("/mi") {
        return format!("{}min", &t[..idx]);
    }
    if lower.ends_with("/mi") {
        return format!("{}n", t.trim());
    }
    if let Some(idx) = lower.rfind("mi") {
        let prefix = t[..idx].trim_end_matches(|c: char| {
            !c.is_ascii_digit() && c != '.' && !matches!(c, 'K' | 'M' | 'B' | 'T' | 'q' | 'Q')
        });
        if parse_number_with_suffix(prefix).is_some() {
            return format!("{prefix}/min");
        }
    }
    if lower.contains("/m") {
        let body = t.split('/').next().unwrap_or("").trim();
        if body == "O" || body == ": O" || body.ends_with(" O") {
            return "0/min".to_string();
        }
    }
    // Windows OCR: "/min" misread as glued junk after the unit suffix ("3.48TVfnjn").
    if !t.contains('/') {
        let mut body = t.as_str();
        for prefix in ["(Cc) ", "(CC) ", "(cc) ", "@ ", "@", "C ", "c ", "© ", "G "] {
            if let Some(rest) = body.strip_prefix(prefix) {
                body = rest.trim_start();
                break;
            }
        }
        for ch in ['T', 'M', 'B', 'K', 'q', 'Q'] {
            if let Some(pos) = body.rfind(ch) {
                let after = &body[pos + 1..];
                if after.is_empty() {
                    continue;
                }
                if after.chars().all(|c| c.is_ascii_alphabetic()) {
                    let stem = &body[..=pos];
                    if parse_number_with_suffix(stem).is_some() {
                        return format!("{stem}/min");
                    }
                }
            }
        }
    }
    t
}

/// Parse a number immediately followed by an optional unit suffix,
/// e.g. "85.8T" -> 85.8e12. Tolerates thousands separators in the digits and
/// the common OCR misread of leading zero as letter O ("O/min").
fn parse_number_with_suffix(text: &str) -> Option<f64> {
    let mut text = text.trim().to_string();
    // OCR often reads 0 as O/o at the start of the number.
    while text.starts_with(['O', 'o']) {
        text.replace_range(0..1, "0");
    }
    let split = text
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let (num_part, suffix) = text.split_at(split);
    let num: f64 = num_part.replace(',', "").trim().parse().ok()?;
    let mult = suffix_multiplier(suffix.trim())?;
    let result = num * mult;
    result.is_finite().then_some(result)
}

/// Classify and parse the coin line.
///
/// Accepts raw OCR text like "0/min", "1.23K/min", "C 3.48T/min", "27.46q".
/// Lines containing '$' are cash, not coins, and are rejected.
pub fn parse_coin_line(raw: &str) -> CoinReading {
    let normalized = normalize_coin_rate_ocr(raw);
    let mut text = normalized.as_str();
    if text.contains('$') {
        return CoinReading::Unreadable;
    }
    // Strip a leading currency glyph the OCR may pick up from the coin icon.
    // The "C" coin icon often reads as @, ©, C or G.
    for prefix in ["C ", "c ", "© ", "G ", "@ ", "@"] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim_start();
            break;
        }
    }
    let lower = text.to_lowercase();
    let min_pos = lower.rfind("min").and_then(|idx| {
        let sep = lower[..idx].chars().last()?;
        matches!(sep, '/' | '(' | '\\' | '|' | ' ').then(|| idx - sep.len_utf8())
    });
    if let Some(idx) = min_pos {
        let mut body = text[..idx].trim().to_string();
        if !is_plausible_rate(&body, raw) {
            return CoinReading::Unreadable;
        }
        if has_coin_icon_prefix(raw)
            && body
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
            && !body.is_empty()
        {
            body.push('T');
        }
        match parse_number_with_suffix(&body) {
            Some(v) => CoinReading::Rate(v),
            None => CoinReading::Unreadable,
        }
    } else if let Some(v) = parse_number_with_suffix(text) {
        CoinReading::Total(v)
    } else {
        CoinReading::Unreadable
    }
}

/// Fragments to try when the top bar shows total coins instead of `/min`.
fn coin_balance_fragments(line: &str) -> Vec<String> {
    let t = line.trim();
    let mut out = vec![t.to_string()];
    if let Some(idx) = t.find('/') {
        out.push(t[..idx].trim().to_string());
    }
    for sep in [' ', '@'] {
        if let Some(idx) = t.rfind(sep) {
            let tail = t[idx + sep.len_utf8()..].trim();
            if !tail.is_empty() {
                out.push(tail.to_string());
            }
        }
    }
    out
}

fn is_plausible_balance_fragment(fragment: &str, full_line: &str) -> bool {
    let Some((num, suffix)) = split_number_suffix(fragment.trim()) else {
        return false;
    };
    if is_balance_tier_suffix(&suffix) && num < 10_000.0 {
        // "2.22s" on upgrade panels is usually seconds, not coin balance.
        if matches!(suffix.as_str(), "s" | "S") && num < 60.0 && !has_coin_icon_prefix(full_line) {
            return false;
        }
        return true;
    }
    if has_coin_icon_prefix(full_line) && matches!(suffix.as_str(), "T" | "q" | "Q") {
        return true;
    }
    false
}

/// Parse a total-coin balance from OCR when `/min` is absent (Goal.md `total_coin`).
pub fn try_parse_balance_line(raw: &str) -> Option<CoinReading> {
    let t = raw.trim();
    if t.is_empty() || t.contains('$') || is_wave_progress_line(t) {
        return None;
    }
    let lower = t.to_lowercase();
    if lower.contains("tier")
        || lower.contains("wave")
        || lower.contains("utility")
        || lower.contains("recovery")
        || lower.contains("enemy")
    {
        return None;
    }
    let mut best: Option<(i32, CoinReading)> = None;
    for fragment in coin_balance_fragments(t) {
        if let CoinReading::Total(v) = parse_coin_line(&fragment) {
            if !is_plausible_balance_fragment(&fragment, t) {
                continue;
            }
            let mut score = 0;
            if has_coin_icon_prefix(t) {
                score += 10;
            }
            if let Some((_, suffix)) = split_number_suffix(fragment.trim()) {
                if is_balance_tier_suffix(&suffix) {
                    score += 20;
                }
            }
            if fragment.trim() == t {
                score += 5;
            }
            if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                best = Some((score, CoinReading::Total(v)));
            }
        }
    }
    best.map(|(_, r)| r)
}

/// Wave progress counter OCR'd into the coin crop, e.g. "1933 / 2002".
pub fn is_wave_progress_line(raw: &str) -> bool {
    let parts: Vec<&str> = raw.split('/').map(str::trim).collect();
    if parts.len() != 2 {
        return false;
    }
    parts[0].chars().all(|c| c.is_ascii_digit()) && parts[1].chars().all(|c| c.is_ascii_digit())
}

/// Parse a coin/min line from the dedicated coin OCR crop (no $ cash line).
/// Accepts M/B suffixes that full-frame parsing rejects as cash.
fn parse_coin_crop_rate(raw: &str) -> CoinReading {
    if raw.contains('$') || raw.starts_with(';') {
        return CoinReading::Unreadable;
    }
    let normalized = normalize_coin_rate_ocr(raw);
    let mut text = normalized.as_str();
    for prefix in [
        "(Cc)", "(CC)", "(cc)", "(C)", "(c)", "C ", "c ", "© ", "G ", "@ ", "@",
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim_start();
            break;
        }
    }
    let lower = text.to_lowercase();
    let min_pos = lower.rfind("min").and_then(|idx| {
        let sep = lower[..idx].chars().last()?;
        matches!(sep, '/' | '(' | '\\' | '|' | ' ' | '=').then(|| idx - sep.len_utf8())
    });
    if let Some(idx) = min_pos {
        let body = fix_spaced_decimal(&text[..idx]);
        if let Some((num, suffix)) = split_number_suffix(&body) {
            if is_balance_tier_suffix(&suffix) && num < 100.0 {
                return CoinReading::Unreadable;
            }
        }
        match parse_number_with_suffix(&body) {
            Some(v) => CoinReading::Rate(v),
            None => CoinReading::Unreadable,
        }
    } else if let Some(v) = parse_number_with_suffix(text) {
        CoinReading::Total(v)
    } else {
        CoinReading::Unreadable
    }
}

/// Parse coin/min from a tight anchor crop where OCR often drops "/min"
/// or appends junk, e.g. "@ 3.48\\" or "@ 3.48T".
pub fn parse_coin_anchor_crop(raw: &str) -> CoinReading {
    if is_wave_progress_line(raw) {
        return CoinReading::Unreadable;
    }
    if let reading @ CoinReading::Rate(_) = parse_coin_crop_rate(raw) {
        return reading;
    }
    if let CoinReading::Rate(v) = parse_coin_line(raw) {
        return CoinReading::Rate(v);
    }
    let mut text = raw.trim();
    for prefix in [
        "(Cc)", "(CC)", "(cc)", "(C)", "(c)", "C ", "c ", "© ", "G ", "@ ", "@",
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.trim_start();
            break;
        }
    }
    // Keep only the leading numeric token and optional rate suffix.
    let mut end = 0usize;
    for (i, c) in text.char_indices() {
        if c.is_ascii_digit() || c == '.' || c == ',' {
            end = i + c.len_utf8();
        } else if matches!(c, 'K' | 'M' | 'B' | 'T' | 'k' | 'm' | 'b' | 't') && end > 0 {
            end = i + c.len_utf8();
            break;
        } else if end > 0 {
            break;
        }
    }
    if end == 0 {
        return CoinReading::Unreadable;
    }
    let mut token = text[..end].to_string();
    if let Some((num, suffix)) = split_number_suffix(&token) {
        if is_balance_tier_suffix(&suffix) {
            if let Some(mult) = suffix_multiplier(&suffix) {
                return CoinReading::Total(num * mult);
            }
        }
    }
    if token
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        && has_coin_icon_prefix(raw)
    {
        token.push('T');
    }
    if let Some(v) = parse_number_with_suffix(&token) {
        CoinReading::Rate(v)
    } else {
        CoinReading::Unreadable
    }
}

/// Parse a wave reading like "Wave 4321" or bare "4321".
pub fn parse_wave(raw: &str) -> Option<u32> {
    let text = raw.trim();
    let lower = text.to_lowercase();
    if lower.contains("tier") && !lower.contains("wave") {
        return None;
    }
    let text = if let Some(idx) = lower.find("wave") {
        text[idx + 4..].trim_start()
    } else {
        text
    };
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// Parse a tier reading like "Tier 12" or the tournament variant "Tier 17+".
/// Returns (tier, is_tournament).
pub fn parse_tier(raw: &str) -> Option<(u32, bool)> {
    let text = raw.trim();
    let lower = text.to_lowercase();
    let text = if let Some(idx) = lower.find("tier") {
        text[idx..].trim_start()
    } else {
        text
    };
    let text = strip_prefix_ci(text, "tier").unwrap_or(text).trim();
    let tournament = text.contains('+');
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    Some((digits.parse().ok()?, tournament))
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    if text.len() < prefix.len() {
        return None;
    }
    if !text.get(..prefix.len())?.eq_ignore_ascii_case(prefix) {
        return None;
    }
    Some(text.get(prefix.len()..)?.trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Examples straight from Goal.md "Value parsing".
    #[test]
    fn coin_rate_examples_from_goal_md() {
        assert_eq!(parse_coin_line("456/min"), CoinReading::Rate(456.0));
        assert_eq!(parse_coin_line("C 1.23K/min"), CoinReading::Rate(1230.0));
        assert_eq!(parse_coin_line("1.23K/min"), CoinReading::Rate(1230.0));
        assert_eq!(parse_coin_line("85.8T/min"), CoinReading::Rate(85.8e12));
    }

    #[test]
    fn try_parse_balance_rejects_seconds_timer() {
        assert_eq!(
            try_parse_balance_line("2.22s"),
            None,
            "upgrade panel timers are not coin balances"
        );
    }

    // Raw values from reference fixture screenshots (see Goal.md).
    #[test]
    fn coin_values_from_fixtures() {
        // Coin_per_minute.png, intro_sprint.png
        assert_eq!(parse_coin_line("0/min"), CoinReading::Rate(0.0));
        // expected_state_full_game.png (3.48T/min -> 3480000000000)
        assert_eq!(parse_coin_line("3.48T/min"), CoinReading::Rate(3.48e12));
        // total_coin.png: balance, not a rate
        assert_eq!(parse_coin_line("27.46q"), CoinReading::Total(27.46e15));
        // tournament.png: balance, not a rate
        assert_eq!(parse_coin_line("3.06q"), CoinReading::Total(3.06e15));
    }

    #[test]
    fn coin_line_with_icon_prefix_and_cash_rejection() {
        assert_eq!(parse_coin_line("C 3.48T/min"), CoinReading::Rate(3.48e12));
        // Cash line must not be mistaken for coins.
        assert_eq!(parse_coin_line("$ 341M/min"), CoinReading::Unreadable);
        assert_eq!(parse_coin_line("garbage"), CoinReading::Unreadable);
    }

    // Raw lines exactly as the Windows OCR engine read them off the fixtures.
    #[test]
    fn wave_progress_line_is_not_coin() {
        assert!(is_wave_progress_line("1933 / 2002"));
        assert!(is_wave_progress_line("2010 / 2071"));
        assert_eq!(
            parse_coin_anchor_crop("1933 / 2002"),
            CoinReading::Unreadable
        );
    }

    #[test]
    fn coin_crop_accepts_m_suffix_without_icon() {
        assert_eq!(
            parse_coin_anchor_crop("512M/min"),
            CoinReading::Rate(512.0e6)
        );
        assert_eq!(
            parse_coin_anchor_crop("E408T/mi"),
            CoinReading::Rate(408.0e12)
        );
    }

    #[test]
    fn coin_windows_ocr_glued_suffix() {
        assert_eq!(parse_coin_line("@ 3.48TVfnjn"), CoinReading::Rate(3.48e12));
        assert_eq!(parse_coin_line("3.48TVfnjn"), CoinReading::Rate(3.48e12));
    }

    #[test]
    fn coin_live_ocr_quirks() {
        assert_eq!(
            parse_coin_anchor_crop("62.4T1mi"),
            CoinReading::Rate(62.4e12)
        );
        assert_eq!(
            parse_coin_anchor_crop("(Cc) 3 A8T /min="),
            CoinReading::Rate(3.48e12)
        );
        assert_eq!(
            parse_coin_anchor_crop("70.6T/rtf"),
            CoinReading::Rate(70.6e12)
        );
        assert_eq!(
            parse_coin_anchor_crop("542M/n'lin"),
            CoinReading::Rate(542.0e6)
        );
        assert_eq!(
            parse_coin_anchor_crop("546M(min"),
            CoinReading::Rate(546.0e6)
        );
        assert_eq!(
            parse_coin_anchor_crop(") 71T/nA1"),
            CoinReading::Rate(71.0e12)
        );
        assert_eq!(
            parse_coin_anchor_crop("492M/min"),
            CoinReading::Rate(492.0e6)
        );
        assert_eq!(
            parse_coin_anchor_crop("1933 / 2002"),
            CoinReading::Unreadable
        );
    }

    #[test]
    fn coin_anchor_crop_without_min_suffix() {
        assert_eq!(
            parse_coin_anchor_crop("@ 3.48\\"),
            CoinReading::Rate(3.48e12)
        );
        assert_eq!(
            parse_coin_anchor_crop("@ 3.48T"),
            CoinReading::Rate(3.48e12)
        );
        assert_eq!(
            parse_coin_anchor_crop("@ 68.8Tz"),
            CoinReading::Rate(68.8e12)
        );
        assert_eq!(parse_coin_anchor_crop("@ O/min"), CoinReading::Rate(0.0));
    }

    #[test]
    fn coin_line_ocr_quirks() {
        assert_eq!(parse_coin_line("3.48T/mi"), CoinReading::Rate(3.48e12));
        assert_eq!(parse_coin_line("67.2T/miI"), CoinReading::Rate(67.2e12));
        assert_eq!(parse_coin_line("74.2T/m!"), CoinReading::Rate(74.2e12));
        assert_eq!(parse_coin_line("70T/min„"), CoinReading::Rate(70.0e12));
        assert_eq!(parse_coin_line("72T/min_"), CoinReading::Rate(72.0e12));
        assert_eq!(parse_coin_line("71.4T/mir"), CoinReading::Rate(71.4e12));
        assert_eq!(parse_coin_line("52.8Timi"), CoinReading::Rate(52.8e12));
        assert_eq!(parse_coin_line("Y 72.6T/miI"), CoinReading::Rate(72.6e12));
        // Coin icon read as @, zero read as letter O
        assert_eq!(parse_coin_line("@ O/min"), CoinReading::Rate(0.0));
        // "/min" read as "(min"
        assert_eq!(parse_coin_line("@ 3.48 (min"), CoinReading::Rate(3.48e12));
        assert_eq!(parse_coin_line("@ 3.48 (mine"), CoinReading::Rate(3.48e12));
        assert_eq!(parse_coin_line("@ 3.48 trninz"), CoinReading::Rate(3.48e12));
        assert_eq!(parse_coin_line("@ O/ min-"), CoinReading::Rate(0.0));
        assert_eq!(parse_coin_line("0|min"), CoinReading::Rate(0.0));
        // Multiplier lines must never parse as coin values
        assert_eq!(parse_coin_line("x3312.65"), CoinReading::Unreadable);
    }

    /// Total coin balance misread with a spurious /min suffix.
    #[test]
    fn rejects_total_balance_as_rate() {
        assert_eq!(parse_coin_line("@ 6.00q/min"), CoinReading::Unreadable);
        assert_eq!(parse_coin_line("@ 27.46q/min"), CoinReading::Unreadable);
        assert_eq!(parse_coin_line("6.00q/min"), CoinReading::Unreadable);
        // Real rate at similar tier should still parse.
        assert_eq!(parse_coin_line("@ 85.8T/min"), CoinReading::Rate(85.8e12));
        assert_eq!(parse_coin_line("@ 100T/min"), CoinReading::Rate(100.0e12));
    }

    #[test]
    fn rejects_cash_rate_without_dollar_sign() {
        // Cash line when OCR drops the '$' prefix.
        assert_eq!(parse_coin_line("6.9M/min"), CoinReading::Unreadable);
    }

    #[test]
    fn suffix_table_from_goal_md() {
        assert_eq!(suffix_multiplier(""), Some(1.0));
        assert_eq!(suffix_multiplier("K"), Some(1e3));
        assert_eq!(suffix_multiplier("M"), Some(1e6));
        assert_eq!(suffix_multiplier("B"), Some(1e9));
        assert_eq!(suffix_multiplier("T"), Some(1e12));
        assert_eq!(suffix_multiplier("q"), Some(1e15));
        assert_eq!(suffix_multiplier("Q"), Some(1e18));
        assert_eq!(suffix_multiplier("s"), Some(1e21));
        assert_eq!(suffix_multiplier("S"), Some(1e24));
        assert_eq!(suffix_multiplier("O"), Some(1e27));
        assert_eq!(suffix_multiplier("N"), Some(1e30));
        assert_eq!(suffix_multiplier("D"), Some(1e33));
        assert_eq!(suffix_multiplier("aa"), Some(1e36));
        assert_eq!(suffix_multiplier("ab"), Some(1e39));
        assert_eq!(suffix_multiplier("ac"), Some(1e42));
        // Pattern continues
        assert_eq!(suffix_multiplier("az"), Some(10f64.powi((12 + 25) * 3)));
        assert_eq!(suffix_multiplier("ba"), Some(10f64.powi((12 + 26) * 3)));
        assert_eq!(suffix_multiplier("ZZ"), None);
    }

    #[test]
    fn wave_parsing() {
        assert_eq!(parse_wave("Wave 4321"), Some(4321));
        assert_eq!(parse_wave("Wave 10"), Some(10)); // Wave_and_Tier.png
        assert_eq!(parse_wave("Wave 650"), Some(650)); // intro_sprint.png
        assert_eq!(parse_wave("wave 865"), Some(865)); // tournament.png
        assert_eq!(parse_wave("4321"), Some(4321));
        assert_eq!(parse_wave("Wave 4571 2.370"), Some(4571));
        assert_eq!(parse_wave("Wave"), None);
        assert_eq!(parse_wave("Tier 12"), None);
    }

    #[test]
    fn tier_parsing() {
        assert_eq!(parse_tier("Tier 12"), Some((12, false)));
        assert_eq!(parse_tier("| Tier 12 160.52T"), Some((12, false)));
        assert_eq!(parse_tier("Tier 14"), Some((14, false))); // Wave_and_Tier.png
                                                              // tournament.png: "Tier 17+" -> 17, tournament
        assert_eq!(parse_tier("Tier 17+"), Some((17, true)));
        assert_eq!(parse_tier("17+"), Some((17, true)));
        assert_eq!(parse_tier("Tier"), None);
    }
}

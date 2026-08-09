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
/// Exact power of ten, computed via the correctly-rounded decimal parser.
///
/// `10f64.powi(n)` is iterated multiplication and is not guaranteed to be the
/// correctly-rounded nearest `f64`, so for `n >= 23` it can differ from the
/// literal `1eN` by an ULP — and that rounding depends on the optimization
/// level, which made tests pass in `--release` but fail in debug. Parsing
/// `"1eN"` matches the compiler's float literals bit-for-bit on every platform.
fn pow10(exp: i32) -> f64 {
    format!("1e{exp}").parse::<f64>().unwrap_or(f64::INFINITY)
}

pub fn suffix_multiplier(suffix: &str) -> Option<f64> {
    const SINGLE: [&str; 12] = ["", "K", "M", "B", "T", "q", "Q", "s", "S", "O", "N", "D"];
    if let Some(idx) = SINGLE.iter().position(|s| *s == suffix) {
        return Some(pow10(idx as i32 * 3));
    }
    let bytes = suffix.as_bytes();
    if bytes.len() == 2 && bytes.iter().all(|b| b.is_ascii_lowercase()) {
        let idx = 12 + (bytes[0] - b'a') as i32 * 26 + (bytes[1] - b'a') as i32;
        return Some(pow10(idx * 3));
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

/// Minimum `/min` rate magnitude before a balance-tier suffix is treated as a
/// real coin rate rather than total-balance OCR with a spurious "/min".
fn min_balance_tier_rate_threshold(suffix: &str) -> Option<f64> {
    if !is_balance_tier_suffix(suffix) {
        return None;
    }
    Some(if matches!(suffix, "q" | "Q") {
        // Total-balance false positives are usually under ~28q; tier-18+ rates are 30q+/min.
        30.0
    } else {
        100.0
    })
}

/// Reject coin/min readings that match total-balance patterns or cash lines.
fn is_plausible_rate(body: &str, raw: &str) -> bool {
    let Some((num, suffix)) = split_number_suffix(body) else {
        return false;
    };
    // Total coin on screen: "6.00q", "27.46q" — OCR sometimes adds "/min".
    if let Some(threshold) = min_balance_tier_rate_threshold(&suffix) {
        if num < threshold {
            return false;
        }
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
    if has_coin_icon_prefix(full_line) && matches!(suffix.as_str(), "q" | "Q") {
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

/// Enemy/spawn stat row — `69.76T/s`, not coin/min.
pub fn is_spawn_rate_line(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    !lower.contains("/min") && lower.contains("/s")
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
            if let Some(threshold) = min_balance_tier_rate_threshold(&suffix) {
                if num < threshold {
                    return CoinReading::Unreadable;
                }
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
    if is_wave_progress_line(raw) || is_spawn_rate_line(raw) {
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

/// Max waves reported by the in-game "Wave Skipped! xN" overlay.
pub const MAX_WAVE_SKIP_COUNT: u32 = 20;

/// OCR reading of the in-game wave-skip banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WaveSkipOverlay {
    /// True when OCR sees "Wave Skipped!" (with or without `xN`).
    pub seen: bool,
    /// `Some(N)` when `xN` is shown. The game never shows `x1`; a lone banner means one skip.
    pub multiplier: Option<u32>,
}

impl WaveSkipOverlay {
    pub fn expected_skip_count(self) -> Option<u32> {
        if !self.seen {
            return None;
        }
        Some(self.multiplier.unwrap_or(1))
    }
}

/// Banner ×N for display when OCR parsed it; lone +1 banner only.
pub fn wave_skip_banner_multiplier(overlay: WaveSkipOverlay, wave_delta: u32) -> Option<u32> {
    if let Some(n) = overlay.multiplier {
        return Some(n);
    }
    if overlay.seen && wave_delta == 1 {
        return Some(1);
    }
    None
}

/// Parse "Wave Skipped!" with optional `xN` multiplier (2–20; never `x1`).
/// A banner without `xN` means a single skip. Values above 20 are rejected.
pub fn parse_wave_skip_overlay(lines: &[String]) -> WaveSkipOverlay {
    let lowered: Vec<String> = lines.iter().map(|l| l.to_lowercase()).collect();
    let mut banner_idx: Option<usize> = None;
    for (i, lower) in lowered.iter().enumerate() {
        if !is_wave_skip_banner_line(lower) {
            continue;
        }
        banner_idx = Some(i);
        if let Some(c) = extract_skip_multiplier_from_banner(lower) {
            return WaveSkipOverlay {
                seen: true,
                multiplier: Some(c),
            };
        }
    }
    let Some(idx) = banner_idx else {
        return WaveSkipOverlay::default();
    };
    // Multiplier may be on the next line or a few lines below (e.g. `C 0/min` during Intro Sprint).
    for offset in 1..=4usize {
        if idx + offset >= lowered.len() {
            break;
        }
        let line = lowered[idx + offset].trim();
        if line.contains("/min") || line.contains('@') {
            continue;
        }
        if is_standalone_skip_multiplier_line(line) {
            if let Some(c) = extract_skip_multiplier_from_banner(line) {
                return WaveSkipOverlay {
                    seen: true,
                    multiplier: Some(c),
                };
            }
        }
    }
    WaveSkipOverlay {
        seen: true,
        multiplier: None,
    }
}

/// OCR reading of the in-game Golden Combo HUD line.
/// Expected shape: `Golden Combo: 0.03% ^166 = x0.05`
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct GoldenComboReading {
    pub seen: bool,
    /// Chance shown before `%` (e.g. `0.03`).
    pub chance_percent: Option<f64>,
    /// Stack / caret count after `^` (OCR often reads `^` as `A`).
    pub caret_count: Option<u32>,
    /// Multiplier after `= x` (may be fractional, e.g. `0.05`).
    pub multiplier: Option<f64>,
}

impl GoldenComboReading {
    /// Merge a newer OCR hit into a latched reading (keep prior fields if this frame omitted them).
    pub fn merge_with(self, newer: Self) -> Self {
        if !newer.seen {
            return self;
        }
        Self {
            seen: true,
            chance_percent: self.chance_percent.or(newer.chance_percent),
            // Activations only grow during a run; never let a flicker like `301`→`1` win.
            caret_count: merge_caret_count(self.caret_count, newer.caret_count),
            // Prefer a newly parsed multiplier so a bad early latch (e.g. `@x2S…`)
            // can be corrected when OCR later yields `xo.08`.
            multiplier: newer.multiplier.or(self.multiplier),
        }
    }
}

/// Latch GC activations across polls.
/// - Prefer growth over shrink (guards `A301`→`A1` flicker).
/// - Reject OCR digit-swap inflation (`303`→`803`, `227`→`827`) even for small jumps.
/// - When a swollen read is a confused twin of a nearby real stack, keep/recover that stack
///   (`302` + OCR `803` → `303`).
/// - Cold-start leading `8` on a 3-digit caret demangles to `3` (`816`→`316`) — dominant
///   yellow-toast error while stacks are still in the 3xx band.
fn merge_caret_count(prev: Option<u32>, newer: Option<u32>) -> Option<u32> {
    match (prev, newer) {
        (None, n) => n.map(demangle_leading_eight_cold),
        (p, None) => p,
        (Some(p), Some(n)) if p == n => Some(p),
        (Some(p), Some(n)) if n > p => {
            if let Some(fixed) = caret_ocr_inflation_correction(p, n) {
                return Some(fixed);
            }
            Some(n)
        }
        (Some(p), Some(n)) => {
            // n < p: allow correcting a corrupted high latch (`827`→`227`, `803`→`303`).
            if caret_digit_confusion(p, n) {
                return Some(n);
            }
            if let Some(fixed) = caret_ocr_inflation_correction(n, p) {
                return Some(fixed);
            }
            Some(p)
        }
    }
}

/// Cold latch: prefer `3xx` over `8xx` (yellow toast often reads `3` as `8`).
fn demangle_leading_eight_cold(n: u32) -> u32 {
    let s = n.to_string();
    if s.len() == 3 && s.starts_with('8') {
        300 + (n % 100)
    } else {
        n
    }
}

/// When `newer` looks like `prev` (or a small real bump from it) with a leading-digit
/// OCR swell — e.g. prev `302` + OCR `803` → real `303` (`3`→`8`), or `227`+`827`→`227`.
fn caret_ocr_inflation_correction(prev: u32, newer: u32) -> Option<u32> {
    if newer <= prev {
        return None;
    }
    let ns = newer.to_string();
    if ns.len() == 3 && ns.starts_with('8') {
        let rest = newer % 100;
        // Prefer the 2xx/3xx twin nearest the prior latch (within a modest authentic bump).
        let mut best: Option<u32> = None;
        for lead in [2u32, 3u32] {
            let cand = lead * 100 + rest;
            if cand >= prev && cand - prev <= 100 {
                best = Some(best.map_or(cand, |b| {
                    // Closer to prev wins; tie → higher (real growth).
                    if cand.abs_diff(prev) < b.abs_diff(prev) {
                        cand
                    } else if cand.abs_diff(prev) > b.abs_diff(prev) {
                        b
                    } else {
                        cand.max(b)
                    }
                }));
            }
        }
        if let Some(cand) = best {
            return Some(cand);
        }
    }
    // Exact single-digit twin that's far above prev (`227`→`827`).
    if caret_digit_confusion(newer, prev) && newer - prev >= 400 {
        return Some(prev);
    }
    None
}

fn caret_digit_confusion(a: u32, b: u32) -> bool {
    let sa = a.to_string();
    let sb = b.to_string();
    if sa.len() != sb.len() || sa.len() < 2 {
        return false;
    }
    let mut diffs = 0usize;
    for (ca, cb) in sa.chars().zip(sb.chars()) {
        if ca == cb {
            continue;
        }
        diffs += 1;
        if diffs > 1 || !ocr_confused_digits(ca, cb) {
            return false;
        }
    }
    diffs == 1
}

fn ocr_confused_digits(a: char, b: char) -> bool {
    matches!(
        (a, b),
        ('2', '8')
            | ('8', '2')
            | ('3', '8')
            | ('8', '3')
            | ('0', '8')
            | ('8', '0')
            | ('5', '6')
            | ('6', '5')
            | ('1', '7')
            | ('7', '1')
            | ('4', '9')
            | ('9', '4')
            | ('0', '6')
            | ('6', '0')
    )
}

/// True when a line should be fed into [`parse_golden_combo`] (label or strong fields).
pub fn is_golden_combo_candidate_line(line: &str) -> bool {
    let lower = normalize_gc_ocr(&line.to_lowercase());
    if is_golden_tower_noise(&lower) || is_gc_neighbor_poison(&lower) {
        return false;
    }
    is_golden_combo_line(&lower)
}

/// Parse Golden Combo HUD text from OCR lines (tolerates common Windows OCR mangling).
pub fn parse_golden_combo(lines: &[String]) -> GoldenComboReading {
    let mut best = GoldenComboReading::default();
    let lowers: Vec<String> = lines
        .iter()
        .map(|l| normalize_gc_ocr(&l.to_lowercase()))
        .collect();

    for (i, lower) in lowers.iter().enumerate() {
        if is_golden_tower_noise(lower) {
            continue;
        }
        let prev_golden = i > 0 && golden_token_present(&lowers[i - 1]);
        let paired_next = i + 1 < lowers.len()
            && golden_token_present(lower)
            && combo_token_present(&lowers[i + 1]);
        let paired_prev = combo_token_present(lower)
            && !golden_token_present(lower)
            && prev_golden;
        // Do NOT treat bare "Gold"/"Golden" alone as enough — that was latching
        // Wave Skip / coin neighbors (`A 446`) as carets. Require combo pairing
        // or same-line field cues via `is_golden_combo_line`.
        if !is_golden_combo_line(lower) && !paired_next && !paired_prev {
            continue;
        }

        // Fold a tight window. Pulling wave counters / TV / skip lines in as neighbors
        // made caret latch onto values like 2340 or 446 from `A 446`.
        let start = if paired_prev
            || (combo_token_present(lower) && prev_golden)
            || (i > 0
                && looks_like_gc_field_fragment(&lowers[i - 1])
                && !is_gc_neighbor_poison(&lowers[i - 1]))
        {
            i.saturating_sub(1)
        } else {
            i
        };
        let mut end = (i + 1).min(lowers.len());
        while end < lowers.len() && end < i + 3 {
            if is_gc_neighbor_poison(&lowers[end]) {
                break;
            }
            if looks_like_gc_field_fragment(&lowers[end])
                || lowers[end].contains("xo.")
                || lowers[end].contains("x0.")
                || (combo_token_present(&lowers[end]) && !golden_token_present(lower))
            {
                end += 1;
                continue;
            }
            // One more line only when the hit itself lacks fields.
            if end == i + 1
                && !looks_like_gc_field_fragment(lower)
                && !lower.contains('%')
                && !lower.contains('=')
            {
                end += 1;
                continue;
            }
            break;
        }
        let mut blob = String::new();
        for (j, part) in lowers[start..end].iter().enumerate() {
            if j > 0 {
                blob.push(' ');
            }
            // Skip wave-progress / skip-overlay / enemy-health neighbors.
            if is_wave_progress_line(part)
                || is_wave_label_line(part)
                || is_gc_neighbor_poison(part)
            {
                continue;
            }
            blob.push_str(part);
        }
        if blob.trim().is_empty() {
            continue;
        }

        let reading = extract_golden_combo_fields(&blob);
        best = best.merge_with(reading);
        if best.chance_percent.is_some()
            && best.caret_count.is_some()
            && best.multiplier.is_some()
        {
            break;
        }
    }
    best
}

fn is_wave_label_line(lower: &str) -> bool {
    let t = lower.trim();
    t.starts_with("wave ") && t.chars().any(|c| c.is_ascii_digit())
}

/// Neighbor / band lines that must never contribute caret digits.
fn is_gc_neighbor_poison(lower: &str) -> bool {
    let compact = alphanumeric_compact(lower);
    compact.contains("waveskip")
        || compact.contains("skipped")
        || compact.contains("enemyhealth")
        || compact.contains("enemyattack")
        || compact.contains("healthlevel")
        || compact.contains("attacklevel")
        || compact.contains("utilityupgrade")
        || compact.contains("attackupgrade")
        || compact.contains("executed")
        || lower.contains("wave skipped")
        || lower.contains("enemy health")
        || lower.contains("enemy attack")
        || lower.contains("/min")
        // Coin/TV rate crumbs (`@ 1.46T`) — require a digit so `@ i.iot` OCR junk
        // next to a real `xo.07` neighbor is not treated as poison.
        || (lower.contains('@')
            && lower.chars().any(|c| c.is_ascii_digit())
            && (lower.contains('t') || lower.contains('q') || lower.contains('b'))
            && !lower.contains("xo")
            && !lower.contains("x0"))
}

/// Enough label signal to trust chance / caret / mult from this blob.
fn gc_label_confident(blob: &str) -> bool {
    let g = golden_token_present(blob);
    let c = combo_token_present(blob);
    if g && c {
        return true;
    }
    // Golden + field cues on the same blob (Combo OCR-dropped).
    if g
        && (blob.contains('%')
            || blob.contains("0/0")
            || blob.contains("/0")
            || blob.contains("xo.")
            || blob.contains("x0.")
            || blob.contains('=')
            || looks_like_gc_field_fragment(blob))
    {
        return true;
    }
    // Label-less but strong: chance + bonus together.
    looks_like_strong_gc_fields(blob)
}

/// Fold common OCR confusions / accented glyphs into ASCII-ish text.
fn fold_gc_glyphs(lower: &str) -> String {
    lower
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'é' | 'è' | 'ë' | 'ê' | 'ē' | 'ė' | 'ę' => 'e',
            'í' | 'ì' | 'ï' | 'î' | 'ī' | 'į' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' | 'õ' | 'ø' | 'ō' | 'ő' => 'o',
            'ú' | 'ù' | 'ü' | 'û' | 'ū' | 'ů' => 'u',
            'ç' | 'ć' | 'č' => 'c',
            'ñ' | 'ń' => 'n',
            'ý' | 'ÿ' => 'y',
            'ß' => 's',
            '„' | '‚' | '“' | '”' | '‘' | '’' | '«' | '»' => ' ',
            '\u{00a0}' | '\u{2007}' | '\u{202f}' => ' ',
            _ => c,
        })
        .collect()
}

/// Collapse the worst Windows-OCR spellings before matching.
fn normalize_gc_ocr(lower: &str) -> String {
    let mut s = fold_gc_glyphs(lower);
    for (from, to) in [
        ("goten", "golden"),
        ("goteh", "golden"),
        ("colden", "golden"),
        ("gol#n", "golden"),
        ("goldach", "golden"),
        ("goldacn", "golden"),
        ("goldacm", "golden"),
        ("goldbh", "golden"),
        ("goldem", "golden"),
        ("goldefi", "golden"),
        ("goldeh", "golden"),
        ("goldeni", "golden"),
        ("goldenc", "golden"),
        ("gdlde", "golden"),
        ("g0lden", "golden"),
        ("g01den", "golden"),
        ("g010de", "golden"),
        ("g010d", "golden"),
        ("go den", "golden"),
        ("gol e", "golden"),
        ("gol ", "golden "),
        ("@tden", "golden"),
        ("@ den", "golden"),
        ("@den", "golden"),
        ("ocqptden", "golden"),
        ("tidercombo", "golden combo"),
        ("colibb", "combo"),
        ("colib", "combo"),
        ("gbmbo", "combo"),
        ("cpmbop", "combo"),
        ("gombo", "combo"),
        ("-ombo", " combo"),
        // Windows OCR often reads `x0.05` as `3<0.05` / `<0.05` / glued `Bxo.09`.
        // Map `3<0.` → `x0.` (not `xo.0`) so `3<0.05` becomes `x0.05`, not `xo.005`.
        ("3<0.", "x0."),
        ("3<o.", "x0."),
        ("bxo.", " xo."),
        ("axo.", " xo."),
        (" a xo.", " xo."),
        ("<0.", "xo.0"),
        ("<o.", "xo.0"),
        ("xo.d", "xo.0"),
        ("xo.o", "xo.0"),
        ("xo.oi", "xo.01"),
        ("xo.ou", "xo.0"),
        ("xo.u", "xo.0"),
        ("xoo.", "xo.0"),
        ("xo ", "xo."),
        ("xo0.", "xo.0"),
        // `0.030<304` — `<` standing in for `%` before the caret.
        ("0<", "%"),
        ("o<", "%"),
    ] {
        if s.contains(from) {
            s = s.replace(from, to);
        }
    }
    s
}

fn is_golden_tower_noise(lower: &str) -> bool {
    let compact = alphanumeric_compact(lower);
    if compact.contains("combo")
        || compact.contains("cotnb")
        || compact.contains("epeibo")
        || compact.contains("ombo")
        || looks_like_gc_field_fragment(lower)
    {
        return false;
    }
    compact.contains("goldentower")
        || compact.contains("goldentow")
        || compact.contains("goldenbonus")
        || compact.contains("towerbonus")
        || (compact.contains("tower") && golden_token_present(lower))
        || (compact.contains("bonus") && golden_token_present(lower))
}

fn alphanumeric_compact(lower: &str) -> String {
    lower
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn golden_token_present(lower: &str) -> bool {
    let compact = alphanumeric_compact(lower);
    compact.contains("golden")
        || compact.contains("golde")
        || compact.contains("goiden")
        || compact.contains("goidel")
        || compact.contains("voiden")
        || compact.contains("g01den")
        || compact.contains("g0lden")
        || compact.contains("g010de")
        || compact.contains("qotaen")
        || compact.contains("olden")
        || compact.contains("tden")
        || compact.contains("goldn")
        || compact.contains("glden")
        || compact.contains("gdlde")
        || (compact.contains("gold") && compact.contains("den"))
        || lower.contains("gold")
        // Truncated first token: "Gol" / "Go" on its own line before "Combo:".
        || compact == "gol"
        || compact == "go"
}

fn combo_token_present(lower: &str) -> bool {
    let compact = alphanumeric_compact(lower);
    compact.contains("combo")
        || compact.contains("cotnb")
        || compact.contains("cpmbo")
        || compact.contains("cpmb")
        || compact.contains("gbmbo")
        || compact.contains("epeibo")
        || compact.contains("btnbo")
        || compact.contains("comhbo")
        || compact.contains("qmb")
        || compact.contains("ombo")
        || compact.contains("aubo")
        || compact.contains("c0bo")
        || compact.contains("cobo")
        || compact.contains("cqbo")
        || compact.contains("cqobo")
        || lower.contains("co bo")
        || lower.contains("com bo")
        || lower.contains("com o")
        || lower.contains("go bo")
        || lower.contains("@ bo")
        || lower.contains(".com")
        || lower.contains("com ")
        || lower.ends_with("com")
        // Truncated "Golden Co" / "Golden Co:"
        || lower.trim_end_matches(':').trim_end().ends_with(" co")
        || compact.ends_with("co") && golden_token_present(lower)
}

fn looks_like_gc_field_fragment(lower: &str) -> bool {
    let t = lower.trim();
    if t.is_empty() {
        return false;
    }
    t.contains('%')
        || t.contains("0/0")
        || t.contains("/0")
        || t.contains("xo.")
        || t.contains("x0.")
        || t.contains("xo ")
        || (t.contains('=') && !t.contains("tier"))
        // Glued chance+caret with no percent marker: `0.0307263`
        || (t.contains("0.03")
            && t.chars().filter(|c| c.is_ascii_digit()).count() >= 5)
        || (t.starts_with('a') && t.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
        || (t.starts_with('q') && t.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
        || (t.starts_with('^') && t.chars().nth(1).is_some_and(|c| c.is_ascii_digit()))
        // `00 A162 g` — caret token not at line start.
        || t.split_whitespace().any(|tok| {
            let tok = tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '^');
            let mut chars = tok.chars();
            match chars.next() {
                Some('a' | 'q' | '^') => chars.next().is_some_and(|c| c.is_ascii_digit()),
                _ => false,
            }
        })
}

/// Standalone OCR crumbs that are almost certainly GC fields (no label).
fn looks_like_strong_gc_fields(lower: &str) -> bool {
    let t = lower.trim();
    let has_chance = t.contains("0.03")
        || (t.contains("0.0") && (t.contains('%') || t.contains("/0") || t.contains("0/0")));
    let has_bonus = t.contains("xo.") || t.contains("x0.0");
    let has_caret = looks_like_gc_field_fragment(t)
        && t.chars().any(|c| c == 'a' || c == 'q' || c == '^' || c.is_ascii_digit());
    let caretish = {
        let chars: Vec<char> = t.chars().collect();
        chars.windows(3).any(|w| {
            matches!(w[0], 'a' | 'q' | '^')
                && w[1].is_ascii_digit()
                && w[2].is_ascii_digit()
        }) || (t.contains('=')
            && t.chars()
                .filter(|c| c.is_ascii_digit())
                .take(6)
                .count()
                >= 2)
    };
    (has_chance && (has_bonus || caretish)) || (has_bonus && (has_chance || caretish)) || (has_bonus && has_caret && t.contains('='))
}

fn is_golden_combo_line(lower: &str) -> bool {
    let compact = alphanumeric_compact(lower);
    if compact.contains("goldencombo")
        || compact.contains("g01dencombo")
        || compact.contains("goidelcombo")
        || compact.contains("goidencombo")
        || compact.contains("goldficombo")
        || compact.contains("goldencom")
        || compact.contains("goldencotnb")
        || compact.contains("goldenombo")
        || compact.contains("goldecombo")
        || compact.contains("voidencombo")
        || compact.contains("enicombo")
    {
        return true;
    }
    let has_golden = golden_token_present(lower);
    let has_combo = combo_token_present(lower);
    if has_golden && has_combo {
        return true;
    }
    // Combo line with chance/bonus cues even when "Golden" was dropped.
    if has_combo && looks_like_gc_field_fragment(lower) {
        return true;
    }
    // OCR often drops "Combo" into noise but keeps chance / caret / xo on the Golden line.
    if has_golden
        && (lower.contains('%')
            || lower.contains("0/0")
            || lower.contains("xo.")
            || lower.contains("x0.")
            || lower.contains("xo ")
            || lower.contains('=')
            || looks_like_gc_field_fragment(lower))
    {
        return true;
    }
    // Label-less field crumb: `0.0 0/0 275=xo.09!`
    if looks_like_strong_gc_fields(lower) {
        return true;
    }
    false
}

fn extract_golden_combo_fields(blob: &str) -> GoldenComboReading {
    if is_golden_tower_noise(blob) {
        return GoldenComboReading::default();
    }
    let confident = gc_label_confident(blob);
    let chance_percent = if confident || golden_token_present(blob) || combo_token_present(blob)
    {
        extract_golden_combo_chance(blob)
    } else {
        None
    };
    // Caret is the most poisoned field (skip overlays use `A NNN`). Only trust it
    // on a confident GC blob.
    let caret_count = if confident {
        extract_golden_combo_caret(blob)
    } else {
        None
    };
    let multiplier = if confident {
        extract_golden_combo_multiplier(blob)
    } else {
        None
    };
    let seen = chance_percent.is_some()
        || caret_count.is_some()
        || multiplier.is_some()
        || golden_token_present(blob)
        || combo_token_present(blob);
    GoldenComboReading {
        seen,
        chance_percent,
        caret_count,
        multiplier,
    }
}

fn percent_marker_len(rest: &str) -> Option<usize> {
    let t = rest.trim_start();
    if t.starts_with('%') {
        return Some(rest.len() - t.len() + 1);
    }
    // OCR often substitutes `<` for `%` (`0.03<304`).
    if t.starts_with('<') {
        return Some(rest.len() - t.len() + 1);
    }
    for marker in ["/0", "0/0", "p/0", "o/0", "/o", "/oe"] {
        if t.starts_with(marker) {
            return Some(rest.len() - t.len() + marker.len());
        }
    }
    None
}

fn extract_golden_combo_chance(blob: &str) -> Option<f64> {
    // `0 03` / `0 03070` — space where the decimal point should be.
    if let Some(v) = chance_from_spaced_decimal(blob) {
        return Some(v);
    }
    // `Comboo.030/0` — leading 0 of chance glued onto "Combo" as `o`.
    if let Some(v) = chance_from_o_decimal(blob) {
        return Some(v);
    }

    let bytes = blob.as_bytes();
    let mut i = 0usize;
    let mut best: Option<f64> = None;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let Ok(num) = std::str::from_utf8(&bytes[start..i]) else {
                continue;
            };
            let Ok(mut v) = num.parse::<f64>() else {
                continue;
            };
            // Glued `0.0300259` / `0.03070` where chance+caret ran together.
            if v >= 1.0 && num.starts_with("0") && num.contains('.') {
                // e.g. unexpected
            } else if !num.contains('.') && num.starts_with('0') && num.len() >= 3 {
                // `003070` after normalize failures — treat as 0.03 if followed by caret digits.
                if let Ok(head) = num[..3].parse::<f64>() {
                    if head == 3.0 || head == 30.0 {
                        // fall through
                    }
                }
            }
            // `0.0300259` → chance 0.03, caret parsed separately from trailing digits.
            if num.contains('.') {
                if let Some(dot) = num.find('.') {
                    let frac = &num[dot + 1..];
                    if frac.len() >= 4 {
                        if let Ok(short) = format!("0.{}", &frac[..2]).parse::<f64>() {
                            if short > 0.0 && short < 1.0 {
                                v = short;
                            }
                        }
                    }
                }
            }
            if !(0.0 < v && v <= 10.0) {
                continue;
            }
            let rest = &blob[i..];
            if percent_marker_len(rest).is_some() {
                return Some(v);
            }
            if v < 1.0 && num.contains('.') {
                best = Some(v);
            }
            continue;
        }
        i += 1;
    }
    best
}

/// `Comboo.030/0` / `comboo.03%` — OCR glued the chance onto the word Combo.
fn chance_from_o_decimal(blob: &str) -> Option<f64> {
    let bytes = blob.as_bytes();
    let mut i = 0usize;
    while i + 3 < bytes.len() {
        let c = bytes[i];
        if (c == b'o' || c == b'O') && bytes[i + 1] == b'.' && bytes[i + 2].is_ascii_digit() {
            // Don't treat `xo.05` multiplier as chance.
            if i > 0 && bytes[i - 1] == b'x' {
                i += 1;
                continue;
            }
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let frac = std::str::from_utf8(&bytes[i + 2..j]).ok()?;
            let head: String = frac.chars().take(2).collect();
            if let Ok(n) = head.parse::<u32>() {
                let v = n as f64 / 100.0;
                if v > 0.0 && v <= 0.25 {
                    let rest = &blob[j..];
                    if percent_marker_len(rest).is_some() || frac.len() >= 3 {
                        return Some(v);
                    }
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    None
}

fn chance_from_spaced_decimal(blob: &str) -> Option<f64> {
    // `0 03` / `0 03070` near golden/combo text.
    let chars: Vec<char> = blob.chars().collect();
    for i in 0..chars.len() {
        if chars[i] != '0' {
            continue;
        }
        let mut j = i + 1;
        let mut skipped_space = false;
        while j < chars.len() && chars[j].is_whitespace() {
            skipped_space = true;
            j += 1;
        }
        // Require a real gap — otherwise `xo.05` looks like spaced `0`+`5`.
        if !skipped_space || j >= chars.len() || !chars[j].is_ascii_digit() {
            continue;
        }
        let start = j;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        let digits: String = chars[start..j].iter().collect();
        if digits.is_empty() {
            continue;
        }
        // Prefer `0 03…` (chance ~0.03) over noisy `0 530/0` → 0.53.
        let head: String = if digits.starts_with('0') && digits.len() >= 3 {
            digits.chars().skip(1).take(2).collect()
        } else {
            digits.chars().take(2).collect()
        };
        if let Ok(frac) = head.parse::<u32>() {
            if (1..100).contains(&frac) {
                let v = frac as f64 / 100.0;
                // GC chance is a small percent (typically ≤ ~0.2).
                if v > 0.0 && v <= 0.25 {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn extract_golden_combo_caret(blob: &str) -> Option<u32> {
    // Merge candidates with the same latch rules as across polls so a crumb `30`
    // loses to `A310`, while `827` does not beat a cleaner `227` on the same blob.
    let mut best: Option<u32> = None;
    let bump = |best: &mut Option<u32>, n: u32| {
        *best = merge_caret_count(*best, Some(n));
    };

    let chars: Vec<char> = blob.chars().collect();
    for i in 0..chars.len() {
        if !is_caret_marker_at(&chars, i) {
            continue;
        }
        if let Some(n) = caret_digits_after_marker(&chars, i) {
            bump(&mut best, n);
        }
    }

    if let Some(n) = caret_digits_after_percent(blob) {
        bump(&mut best, n);
    }

    if let Some(n) = caret_digits_before_equals(blob) {
        bump(&mut best, n);
    }

    best
}

/// Digits after `^` / `A` / `Q`. Skips one OCR-junk glyph (`o`/`l`/`i`) so `Ao310`→310,
/// but rejects 2-digit values that only appear after that junk (`Ao 30` must not latch).
fn caret_digits_after_marker(chars: &[char], marker_idx: usize) -> Option<u32> {
    let mut j = marker_idx + 1;
    while j < chars.len() && (chars[j].is_whitespace() || chars[j] == '-') {
        j += 1;
    }
    let mut skipped_junk = false;
    if j < chars.len() && !chars[j].is_ascii_digit() {
        // `A310` OCR'd as `Ao310` / `Al310` / `A|310` — one phantom glyph before digits.
        if matches!(chars[j], 'o' | 'l' | 'i' | '|' | '!' | '/') {
            skipped_junk = true;
            j += 1;
            while j < chars.len() && (chars[j].is_whitespace() || chars[j] == '-') {
                j += 1;
            }
        }
    }
    let start = j;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    if j <= start {
        return None;
    }
    let digits: String = chars[start..j].iter().collect();
    // After junk, only trust a full 3-digit stack — `Ao 30` is almost always a dropped
    // middle digit from `A310`, not a real ^30.
    if skipped_junk && digits.len() < 3 {
        return None;
    }
    parse_plausible_caret(&digits)
}

/// `^166` / OCR `A151` / `Q77` — require a token boundary so mid-word `a` never matches.
fn is_caret_marker_at(chars: &[char], i: usize) -> bool {
    let c = chars[i];
    if c == '^' || c == 'λ' || c == 'Λ' {
        return true;
    }
    // Windows OCR often reads `^` as `A` or `Q`.
    if c != 'a' && c != 'q' {
        return false;
    }
    i == 0 || !chars[i - 1].is_ascii_alphanumeric()
}

fn parse_plausible_caret(digits: &str) -> Option<u32> {
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    // OCR often pads stacks with a leading zero (`0289`, `026`); trim then require
    // a real 2–3 digit value so crumbs like `01`→1 cannot wipe a latched stack.
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() || trimmed.len() > 3 {
        return None;
    }
    let n: u32 = trimmed.parse().ok()?;
    if (10..=999).contains(&n) {
        Some(n)
    } else {
        None
    }
}

/// Find `A260` / `Q77` / `^166` / `Ao310` inside a post-percent fragment.
fn caret_marker_in_text(text: &str) -> Option<u32> {
    let chars: Vec<char> = text.chars().collect();
    let mut best: Option<u32> = None;
    for i in 0..chars.len() {
        if !is_caret_marker_at(&chars, i) {
            continue;
        }
        if let Some(n) = caret_digits_after_marker(&chars, i) {
            best = merge_caret_count(best, Some(n));
        }
    }
    best
}

fn caret_digits_before_equals(blob: &str) -> Option<u32> {
    let eq = blob.find('=')?;
    let before = blob[..eq].trim_end();
    // Require the GC label somewhere before the equals.
    if !golden_token_present(before) && !combo_token_present(before) {
        return None;
    }
    let digits: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    // Unmarked digits before `=` are a weak signal. Require a full 3-digit stack so
    // mangled `…/Ao 30 =` (true ^310) cannot latch as ^30. Two-digit stacks still
    // parse via an explicit `A`/`^`/`Q` marker.
    if digits.len() != 3 {
        return None;
    }
    // Don't take the fractional tail of `0.03` — reject if the char before the digits is `.`.
    let digit_start = before.len().saturating_sub(digits.len());
    if digit_start > 0 {
        let prev = before[..digit_start].chars().last();
        if matches!(prev, Some('.')) {
            return None;
        }
    }
    parse_plausible_caret(&digits)
}

fn caret_digits_after_percent(blob: &str) -> Option<u32> {
    let bytes = blob.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let rest = &blob[i..];
            if let Some(marker_len) = percent_marker_len(rest) {
                let after = rest[marker_len..].trim_start();
                // Prefer an explicit caret marker in the remainder (`…/0 A260`, `…/60A260`).
                if let Some(n) = caret_marker_in_text(after) {
                    return Some(n);
                }
                let after = after.trim_start_matches(|c: char| {
                    c == 'a'
                        || c == 'q'
                        || c == '^'
                        || c == '*'
                        || c == '·'
                        || c == '-'
                        || c == 'e'
                        || c == 'm' // OCR often reads `^` as `m`
                        || c.is_whitespace()
                });
                let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
                // Unmarked digits after `%`/`/0` need a full 3-digit stack. Two-digit
                // crumbs (`…/Ao 30`) are usually a dropped digit from ^1xx/^2xx/^3xx.
                if digits.len() >= 3 {
                    if let Some(n) = parse_plausible_caret(&digits) {
                        return Some(n);
                    }
                }
                // Glued chance+caret without marker gap: `0.0300259` already shortened
                // chance; also handle `0.030/oe226`.
            }
            // Digits glued onto a long fractional chance: `0.0300259` → caret 259 / 0259.
            continue;
        }
        i += 1;
    }

    // `0.0300259` / `0.03070` — take trailing 2–4 digits after a 0.03-like prefix.
    caret_from_glued_fraction(blob)
}

fn caret_from_glued_fraction(blob: &str) -> Option<u32> {
    // Only when this looks like a GC chance glue, not a random `0.08` multiplier/coin.
    if !golden_token_present(blob) && !combo_token_present(blob) {
        return None;
    }
    let bytes = blob.as_bytes();
    let mut i = 0usize;
    while i + 4 < bytes.len() {
        if bytes[i] == b'0' && bytes[i + 1] == b'.' && bytes[i + 2] == b'0' {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            let frac = std::str::from_utf8(&bytes[i + 2..j]).ok()?;
            // Need a long frac so we are not reading `0.03` or `0.08` alone.
            if frac.len() >= 5 {
                // `0300259` → skip `03`, remaining `00259` / `0259` / `259`
                let rest = &frac[2..];
                let trimmed = rest.trim_start_matches('0');
                let digits = if trimmed.is_empty() {
                    rest
                } else if trimmed.len() >= 3 {
                    &trimmed[..trimmed.len().min(3)]
                } else if rest.len() >= 3 {
                    &rest[rest.len() - 3..]
                } else {
                    trimmed
                };
                if let Some(n) = parse_plausible_caret(digits) {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

fn extract_golden_combo_multiplier(blob: &str) -> Option<f64> {
    let normalized = blob
        .replace("xo ", "xo.")
        .replace("x o.", "xo.")
        .replace("xo.d", "xo.0")
        .replace("xo.o", "xo.0")
        .replace("xo.ou", "xo.0")
        .replace("xoo.", "xo.0")
        // Dilated OCR often drops `x` → `'(0.10!` / `(0.10!` / `'0.10!`
        .replace("'(0.", "x0.")
        .replace("(0.", "x0.")
        .replace("'0.", "x0.");
    // Prefer text after `=`, but always also scan the full blob for `xo.0N`
    // (OCR often drops the equals or puts the bonus on a neighbor line).
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(eq) = normalized.find('=') {
        candidates.push(&normalized[eq + 1..]);
    }
    candidates.push(normalized.as_str());

    for (ci, search) in candidates.iter().enumerate() {
        let fractional_only = ci + 1 == candidates.len() && normalized.find('=').is_none();
        let chars: Vec<char> = search.chars().collect();
        for i in 0..chars.len() {
            let c = chars[i];
            let star_as_x = c == '*';
            if c != 'x' && c != '×' && !star_as_x {
                continue;
            }
            if i > 0 {
                let prev = chars[i - 1];
                if prev == '@' || prev.is_ascii_alphanumeric() {
                    continue;
                }
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let mut num = String::new();
            let mut used_xo = star_as_x;
            if j < chars.len() && (chars[j] == 'o' || chars[j] == 'd') && !star_as_x {
                num.push('0');
                j += 1;
                used_xo = true;
            }
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                num.push(chars[j]);
                j += 1;
            }
            if num.is_empty() || num == "." {
                continue;
            }
            let has_frac = num.contains('.');
            if fractional_only && !used_xo && !has_frac {
                continue;
            }
            if !has_frac && !used_xo && j < chars.len() && chars[j].is_ascii_alphabetic() {
                continue;
            }
            if let Ok(v) = num.parse::<f64>() {
                // Real GC bonus is a small fractional multiplier (x0.05), not x2 / x245.
                if used_xo || has_frac {
                    if v > 0.0 && v < 1.0 {
                        return Some(v);
                    }
                } else if v > 0.0 && v <= 20.0 {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn is_standalone_skip_multiplier_line(lower: &str) -> bool {
    parse_standalone_x_multiplier(lower.trim()).is_some()
}

/// True when OCR shows the in-game wave-skip banner (tolerates merged words / typos).
fn is_wave_skip_banner_line(lower: &str) -> bool {
    if is_upgrade_skip_line(lower) {
        return false;
    }
    if lower.contains("wave skip") || lower.contains("waveskip") {
        return true;
    }
    if lower.contains("mave skip") || lower.contains("maveskip") {
        return true;
    }
    if (lower.contains("vave") || lower.contains("wate")) && lower.contains("skip") {
        return true;
    }
    if lower.contains("wav") && lower.contains("skipped") {
        return true;
    }
    if lower.contains("wav") && lower.contains("skived") {
        return true;
    }
    if lower.contains("wav") && lower.contains("skip") && lower.contains('!') {
        return true;
    }
    if lower.contains("skipped") && !lower.contains("level") && !lower.contains("/min") {
        return true;
    }
    let t = lower.trim();
    if t.starts_with("skipped") && !lower.contains("level") && !lower.contains("/min") {
        return true;
    }
    if is_partial_wave_skip_banner_line(lower) {
        return true;
    }
    false
}

/// OCR often truncates the banner ("Wave Sk", "ave Skipped!", "Wave S Ippe").
fn is_partial_wave_skip_banner_line(lower: &str) -> bool {
    if is_upgrade_skip_line(lower) {
        return false;
    }
    if lower.contains("ave skip") && !lower.contains("level") {
        return true;
    }
    if lower.contains("wave") {
        let after_wave = lower.split("wave").nth(1).unwrap_or("");
        let letters: String = after_wave
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect();
        if letters.starts_with("skip")
            || letters.starts_with("sk")
            || (letters.starts_with('s') && letters.contains("ip"))
        {
            return true;
        }
    }
    false
}

fn is_upgrade_skip_line(lower: &str) -> bool {
    (lower.contains("level") || lower.contains("enemy") || lower.contains("health"))
        && lower.contains("skip")
        && !lower.contains("skipped")
}

fn extract_skip_multiplier_from_banner(lower: &str) -> Option<u32> {
    if lower.contains('@') || lower.contains("/min") {
        return None;
    }
    if let Some(c) = parse_standalone_x_multiplier(lower.trim()) {
        return Some(c);
    }
    if let Some(pos) = lower.find("skip") {
        if let Some(c) = find_x_multiplier_in_suffix(&lower[pos..]) {
            return validate_wave_skip_count(c);
        }
    }
    find_x_multiplier_in_suffix(lower).and_then(validate_wave_skip_count)
}

fn parse_standalone_x_multiplier(text: &str) -> Option<u32> {
    let rest = text
        .strip_prefix('x')
        .or_else(|| text.strip_prefix('×'))?
        .trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    let after = rest[digits.len()..].trim();
    if !after.is_empty()
        && after
            .chars()
            .any(|c| c.is_ascii_alphanumeric() && c != '!' && c != '.')
    {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    validate_wave_skip_count(n)
}

fn find_x_multiplier_in_suffix(s: &str) -> Option<u32> {
    let lower = s.to_lowercase();
    for (byte_idx, _) in s.char_indices() {
        let lc = lower[byte_idx..].chars().next()?;
        if lc != 'x' && lc != '×' {
            continue;
        }
        let prev = lower[..byte_idx].chars().last();
        if prev.map(|p| p.is_ascii_alphabetic()).unwrap_or(false) {
            continue;
        }
        let rest = &s[byte_idx + lc.len_utf8()..].trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || digits.len() > 2 {
            continue;
        }
        let after = rest[digits.len()..].trim();
        if !after.is_empty()
            && after
                .chars()
                .any(|c| c.is_ascii_alphanumeric() && c != '!' && c != '.')
        {
            continue;
        }
        if let Ok(n) = digits.parse::<u32>() {
            if let Some(v) = validate_wave_skip_count(n) {
                return Some(v);
            }
        }
    }
    None
}

fn validate_wave_skip_count(n: u32) -> Option<u32> {
    if (1..=MAX_WAVE_SKIP_COUNT).contains(&n) {
        Some(n)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

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

    #[test]
    fn spawn_rate_line_is_not_coin_per_min() {
        assert!(is_spawn_rate_line("390.79M' 69.76T/s @"));
        assert_eq!(
            parse_coin_anchor_crop("390.79M' 69.76T/s @"),
            CoinReading::Unreadable
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

    /// Tier-18+ dissonance upgrade screens show coin as "@ 35.8q/min" (not bare "@ 124Q").
    #[test]
    fn accepts_high_tier_q_per_min_rates() {
        for (line, expected) in [
            ("@ 35.8q/min", 35.8e15),
            ("@ 58.2q/min", 58.2e15),
            ("@ 58.5q/min", 58.5e15),
            ("@ 124.84Q/min", 124.84e18),
        ] {
            match parse_coin_line(line) {
                CoinReading::Rate(v) => assert!(
                    (v - expected).abs() < expected * 1e-9,
                    "{line}: expected {expected}, got {v}"
                ),
                other => panic!("{line}: expected Rate, got {other:?}"),
            }
        }
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
        // Pattern continues. idx("az") = 12 + 0*26 + 25 = 37 -> 10^111;
        // idx("ba") = 12 + 1*26 + 0 = 38 -> 10^114.
        assert_eq!(suffix_multiplier("az"), Some(1e111));
        assert_eq!(suffix_multiplier("ba"), Some(1e114));
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
    fn wave_skip_overlay_parsing() {
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Skipped!", "x5"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(5),
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Skipped! x12"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(12),
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Skipped!"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Skipped!", "x25"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["@ 3.48T/min", "x3312"])),
            WaveSkipOverlay::default()
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["WaveSkipbed! x3"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(3),
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Skipped!", "$216.28K"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert!(
            !is_wave_skip_banner_line("enemy health level skip")
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Skipped!", "Enemy Attack Level Skip x4"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Vtave Skipped.", "Tier 15"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["VVAve Skipped! x5", "Tier 15"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(5),
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["aee Skipped!", "Tier 15", "Wave 2137"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Enemy Attack Level Skip x4", "Wave Skipped!"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Sk", "$388.18K"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["ave Skipped!", "$1904.09KY"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["0 ; 620 0 Wave Skipped! x", "$388.18K"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave S Ippe x2", "Tier 15"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(2),
            }
        );
        assert!(
            !is_wave_skip_banner_line("Enemy Attack Level Skip")
        );
    }

    #[test]
    fn wave_skip_multiplier_after_zero_min_line() {
        assert_eq!(
            parse_wave_skip_overlay(&s(&[
                "Wave Skipped!",
                "C 0/min",
                "x10",
                "Intro Sprint",
            ])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(10),
            }
        );
    }

    #[test]
    fn wave_skip_ocr_typos_from_live_logs() {
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Skived", "Tier 15", "Wave 40"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: None,
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&["Wave Skippe4! x9", "$1.07M"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(9),
            }
        );
        assert_eq!(
            parse_wave_skip_overlay(&s(&[", ave Skipped! x9", "Tier 15", "Wave 100"])),
            WaveSkipOverlay {
                seen: true,
                multiplier: Some(9),
            }
        );
    }

    #[test]
    fn golden_combo_clean_line() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% ^166 = x0.05"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(166));
        assert_eq!(g.multiplier, Some(0.05));
    }

    #[test]
    fn golden_combo_candidate_line_accepts_label_less_chance_crumb() {
        assert!(is_golden_combo_candidate_line("0.03%288 = xo.09!"));
        assert!(is_golden_combo_candidate_line("Golden Comb D'O(030/0 A288 ="));
        assert!(!is_golden_combo_candidate_line("oo"));
        assert!(!is_golden_combo_candidate_line("Wave Skipped! x2"));
    }

    #[test]
    fn golden_combo_ocr_caret_as_a() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% A151 =", "$107.90K"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(151));
    }

    #[test]
    fn golden_combo_ocr_xo_for_x0() {
        let g = parse_golden_combo(&s(&["Goidel? Combo: 01030/0 A152 = xo.05!"]));
        assert!(g.seen);
        assert_eq!(g.caret_count, Some(152));
        assert_eq!(g.multiplier, Some(0.05));
    }

    #[test]
    fn golden_combo_ignores_unrelated() {
        assert!(!parse_golden_combo(&s(&["Golden Tower", "1m Ils"])).seen);
        assert!(!parse_golden_combo(&s(&["Golden tower bonus x1.5"])).seen);
    }

    #[test]
    fn golden_combo_rejects_coin_rate_at_x2_noise() {
        // Live OCR often glues the next TV line: `44.76B/s@x2S911.96`.
        let g = parse_golden_combo(&s(&[
            "Golden Combo: 0.03% A219 =",
            "@ 646.96B",
            "887.85TV 44.76B/s@x2S911.96 Tier 15",
        ]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(219));
        assert_eq!(g.multiplier, None);
    }

    #[test]
    fn golden_combo_same_line_tv_noise_after_equals() {
        let g = parse_golden_combo(&s(&[
            "Golden Combo: 0.03% A219 = @ 646.96B 887.85TV 44.76B/s@x2S911.96 Tier 15",
        ]));
        assert_eq!(g.multiplier, None);
    }

    #[test]
    fn golden_combo_keeps_real_xo_multiplier() {
        let g = parse_golden_combo(&s(&[
            "Golden Combo: 0.03%242 = xo.08!",
            "0409.66B",
        ]));
        assert_eq!(g.multiplier, Some(0.08));
        assert_eq!(g.caret_count, Some(242));
    }

    #[test]
    fn golden_combo_ocr_split_co_bo_and_glued_caret() {
        let g = parse_golden_combo(&s(&["Golden Co�bo: 0.03%296 ="]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(296));
    }

    #[test]
    fn golden_combo_ocr_mangled_epeibo_with_xo() {
        let g = parse_golden_combo(&s(&["0 Golden epeibo: 0.03%242 = xo.08!"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(242));
        assert_eq!(g.multiplier, Some(0.08));
    }

    #[test]
    fn golden_combo_ocr_slash_zero_percent_and_star_multiplier() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% = *0.07!"]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.multiplier, Some(0.07));
    }

    #[test]
    fn golden_combo_ocr_bare_golden_with_caret_before_equals() {
        let g = parse_golden_combo(&s(&["Golden 253 ="]));
        assert!(g.seen);
        assert_eq!(g.caret_count, Some(253));
    }

    #[test]
    fn golden_combo_ocr_split_golden_c9_fields_on_next_line() {
        let g = parse_golden_combo(&s(&[
            "EXIT BATTLE",
            "Golden C9",
            "0.03% A196",
            "$0",
        ]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(196));
    }

    #[test]
    fn golden_combo_ocr_gol_combo_split_lines() {
        let g = parse_golden_combo(&s(&["Gol", "Combo:", "0.03%", "A110"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(110));
    }

    #[test]
    fn golden_combo_ocr_accented_and_xo_neighbor() {
        let g = parse_golden_combo(&s(&[
            "EXIT BATTLE",
            "Golden combq: 0: 00",
            "@ i.iot",
            "xo.07!",
        ]));
        assert!(g.seen);
        assert_eq!(g.multiplier, Some(0.07));
    }

    #[test]
    fn golden_combo_ocr_olden_missing_g() {
        let g = parse_golden_combo(&s(&["olden Combo: 0.03% A162 = xo.05"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(162));
        assert_eq!(g.multiplier, Some(0.05));
    }

    #[test]
    fn golden_combo_ocr_glued_chance_without_percent() {
        let g = parse_golden_combo(&s(&["Golden o bq: 0.0307263"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        let n = g.caret_count.expect("glued caret");
        assert!((10..=9999).contains(&n), "caret={n}");
    }

    #[test]
    fn golden_combo_ocr_chance_glued_onto_combo_word() {
        let g = parse_golden_combo(&s(&["Golden Comboo.030/0 = xo.05"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.multiplier, Some(0.05));
    }

    #[test]
    fn golden_combo_rejects_wave_number_as_caret() {
        let g = parse_golden_combo(&s(&[
            "2347 / 2386",
            "EXIT BATTLE",
            "Golden Combo:",
            "$0",
            "@ 1.38T",
            "Tier 15",
            "Wave 2340",
        ]));
        assert!(g.seen);
        assert_eq!(g.caret_count, None);
    }

    #[test]
    fn golden_combo_rejects_wave_skip_a_number_as_caret() {
        let g = parse_golden_combo(&s(&[
            "Gold",
            "Enemy Health Level Skip x2",
            "Wave Skipped! x2",
            "A 446",
            "@ 27.41T",
        ]));
        assert_ne!(g.caret_count, Some(446));
        assert_ne!(g.caret_count, Some(27));
    }

    #[test]
    fn golden_combo_rejects_bare_gold_with_coin_neighbor() {
        let g = parse_golden_combo(&s(&["Gold", "xo 091", "on 294", "@ 1.66T"]));
        assert_ne!(g.caret_count, Some(294));
        assert_ne!(g.caret_count, Some(91));
    }

    #[test]
    fn golden_combo_still_reads_labeled_line_with_caret() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% A208 = xo.08"]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(208));
        assert_eq!(g.multiplier, Some(0.08));
    }

    #[test]
    fn golden_combo_reads_dilated_toast_a306_quote_paren_mult() {
        // Windows OCR on cyan-noisy toast after yellow dilate: `^306`→`A306`, `x0.10`→`'(0.10`.
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% A306 = '(0.10!"]));
        assert!(g.seen);
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(306));
        assert_eq!(g.multiplier, Some(0.1));
    }

    #[test]
    fn golden_combo_merges_dilate_and_thin_ocr_variants() {
        let g = parse_golden_combo(&s(&[
            "Golden Combo: 0.03% A306 = '(0.10!",
            "Golden Combo: 0.03%006 = xo.10!",
        ]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(306));
        assert_eq!(g.multiplier, Some(0.1));
    }

    #[test]
    fn golden_combo_clean_line_with_caret_and_mult() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% ^306 = x0.10!"]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(306));
        assert_eq!(g.multiplier, Some(0.1));
    }

    #[test]
    fn golden_combo_rejects_coin_fraction_as_caret() {
        let g = parse_golden_combo(&s(&[
            "148 times @",
            "2348 / 2386",
            "Golden Combo:",
            "@1.25",
        ]));
        assert_ne!(g.caret_count, Some(25));
        assert_ne!(g.caret_count, Some(2348));
        assert_ne!(g.caret_count, Some(2386));
    }

    #[test]
    fn golden_combo_rejects_implausible_four_digit_caret() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% 6244 = xo.08!"]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.multiplier, Some(0.08));
        assert_eq!(g.caret_count, None);
    }

    #[test]
    fn golden_combo_ocr_caret_on_previous_line() {
        let g = parse_golden_combo(&s(&["00 A162 g", "Golden Combo:", "8", "00"]));
        assert!(g.seen);
        assert_eq!(g.caret_count, Some(162));
    }

    #[test]
    fn golden_combo_ocr_m_as_caret_marker_after_percent() {
        let g = parse_golden_combo(&s(&["Golden 0.03% m263 ="]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(263));
    }

    #[test]
    fn golden_combo_merge_keeps_higher_caret_against_flicker() {
        let good = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(301),
            multiplier: Some(0.08),
        };
        let flicker = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(1),
            multiplier: None,
        };
        let merged = good.merge_with(flicker);
        assert_eq!(merged.caret_count, Some(301));
    }

    #[test]
    fn golden_combo_merge_rejects_2_to_8_ocr_swap_227_as_827() {
        let good = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(227),
            multiplier: Some(0.08),
        };
        let bad = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(827),
            multiplier: None,
        };
        assert_eq!(good.merge_with(bad).caret_count, Some(227));
    }

    #[test]
    fn golden_combo_merge_302_plus_ocr_803_recovers_303() {
        // Real stack grew 302→303; dilated OCR read unmarked `%803`.
        let prev = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(302),
            multiplier: Some(0.09),
        };
        let bad = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(803),
            multiplier: None,
        };
        assert_eq!(prev.merge_with(bad).caret_count, Some(303));
    }

    #[test]
    fn golden_combo_parse_then_merge_rejects_percent_803_after_302() {
        let prev = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(302),
            multiplier: Some(0.09),
        };
        let frame = parse_golden_combo(&s(&["Golden Combo: 0.03%803 ="]));
        // Parse demangles leading 8→3 on cold/low stacks.
        assert_eq!(frame.caret_count, Some(303));
        assert_eq!(prev.merge_with(frame).caret_count, Some(303));
    }

    #[test]
    fn golden_combo_cold_start_demangles_816_to_316() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03%816 = xo.10!"]));
        assert_eq!(g.caret_count, Some(316));
    }

    #[test]
    fn golden_combo_merge_310_plus_ocr_816_becomes_316() {
        let prev = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(310),
            multiplier: Some(0.1),
        };
        let bad = GoldenComboReading {
            seen: true,
            chance_percent: None,
            caret_count: Some(816),
            multiplier: None,
        };
        assert_eq!(prev.merge_with(bad).caret_count, Some(316));
    }

    #[test]
    fn golden_combo_keeps_8xx_when_already_in_high_regime() {
        let prev = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(780),
            multiplier: None,
        };
        let newer = GoldenComboReading {
            seen: true,
            chance_percent: None,
            caret_count: Some(816),
            multiplier: None,
        };
        assert_eq!(prev.merge_with(newer).caret_count, Some(816));
    }

    #[test]
    fn golden_combo_merge_corrects_803_latch_when_303_arrives() {
        let bad = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(803),
            multiplier: None,
        };
        let good = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(303),
            multiplier: Some(0.1),
        };
        assert_eq!(bad.merge_with(good).caret_count, Some(303));
    }

    #[test]
    fn golden_combo_merge_corrects_827_latch_when_227_arrives() {
        let bad = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(827),
            multiplier: None,
        };
        let good = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(227),
            multiplier: Some(0.08),
        };
        assert_eq!(bad.merge_with(good).caret_count, Some(227));
    }

    #[test]
    fn golden_combo_merge_allows_large_catchup_without_digit_confusion() {
        let prev = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(200),
            multiplier: None,
        };
        let catchup = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(350),
            multiplier: None,
        };
        assert_eq!(prev.merge_with(catchup).caret_count, Some(350));
    }

    #[test]
    fn golden_combo_rejects_leading_zero_caret_crumb() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03%01 = xo.08"]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, None);
        assert_eq!(g.multiplier, Some(0.08));
    }

    #[test]
    fn golden_combo_keeps_three_digit_caret_301() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% A301 = xo.08"]));
        assert_eq!(g.caret_count, Some(301));
    }

    #[test]
    fn golden_combo_ocr_q_caret_and_slash_zero_junk() {
        let g = parse_golden_combo(&s(&["Golden 030/0 Q77 ="]));
        assert!(g.seen);
        assert_eq!(g.caret_count, Some(77));
    }

    #[test]
    fn golden_combo_ocr_caret_after_junk_digits_with_a_marker() {
        let g = parse_golden_combo(&s(&["Golden Combo, 0.030/60A260 ="]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(260));
    }

    #[test]
    fn golden_combo_ocr_less_than_as_percent_before_caret() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.030<304 ="]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(304));
    }

    #[test]
    fn golden_combo_ocr_padded_zero_caret_0289() {
        let g = parse_golden_combo(&s(&["Goldencombo: 0.03%0289 ="]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(289));
    }

    #[test]
    fn golden_combo_ocr_colden_and_angle_multiplier() {
        let g = parse_golden_combo(&s(&["Colden Combo: 0.03% A151 = 3<0.05!"]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(151));
        assert_eq!(g.multiplier, Some(0.05));
    }

    #[test]
    fn golden_combo_ocr_label_less_fields_with_xo() {
        let g = parse_golden_combo(&s(&["0.0 0/0 275=xo.09!"]));
        assert!(g.seen);
        assert_eq!(g.caret_count, Some(275));
        assert_eq!(g.multiplier, Some(0.09));
    }

    #[test]
    fn golden_combo_ocr_bxo_glued_after_caret() {
        let g = parse_golden_combo(&s(&["Golden co .bo: 0.03% A283Bxo.09!"]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_eq!(g.caret_count, Some(283));
        assert_eq!(g.multiplier, Some(0.09));
    }

    #[test]
    fn golden_combo_rejects_mangled_ao_30_crumb_from_310() {
        // Live OCR for ^310 came through as `0.030/Ao 30 =` — must not latch ^30.
        let g = parse_golden_combo(&s(&["Golden Combo: 0.030/Ao 30 ="]));
        assert_eq!(g.chance_percent, Some(0.03));
        assert_ne!(g.caret_count, Some(30));
        assert_eq!(g.caret_count, None);
    }

    #[test]
    fn golden_combo_recovers_ao310_junk_glyph() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% Ao310 = xo.08"]));
        assert_eq!(g.caret_count, Some(310));
    }

    #[test]
    fn golden_combo_prefers_310_over_30_crumb_on_same_line() {
        let g = parse_golden_combo(&s(&["Golden Combo: 0.03% A310 = xo.08", "30 ="]));
        assert_eq!(g.caret_count, Some(310));
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

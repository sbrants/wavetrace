//! Classify a set of OCR lines into a PollInput, following the
//! "Game mode edge cases" decision flow in Goal.md.

use crate::parser::{
    has_coin_icon_prefix, is_wave_progress_line, parse_coin_anchor_crop, parse_coin_line,
    CoinReading,
};
use crate::state_machine::{GameMode, PollInput};

/// True when an OCR line plausibly comes from the top-bar coin display, not
/// combat stats, health bars, or progress indicators elsewhere on screen.
fn is_likely_coin_line(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();

    // Cash, combat, and UI labels are never the coin line.
    if lower.contains('$')
        || lower.contains("wave")
        || lower.contains("tier")
        || lower.contains("/s")
        || lower.contains("skip")
        || lower.contains("damage")
        || lower.contains("recovery")
        || lower.contains("boss")
        || lower.contains("stats")
        || lower.contains("retry")
        || lower.contains("home")
    {
        return false;
    }
    // Multipliers like "x3312.65" or merged stat rows.
    if lower.starts_with('x') || lower.contains("@x") || lower.contains(" x") {
        return false;
    }
    // Health / shield bars: "591.53T / 35.85T"
    if t.contains(" / ") {
        return false;
    }
    // Wave progress: "865 / 900" or "650 / 675"
    if !lower.contains("min") {
        let slash_parts: Vec<&str> = t.split('/').collect();
        if slash_parts.len() == 2 {
            let all_numeric = slash_parts.iter().all(|p| {
                p.trim()
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '.' || c.is_whitespace())
            });
            if all_numeric {
                return false;
            }
        }
    }

    // Strong signals: coin icon prefix or a /min rate suffix.
    for prefix in ["@", "C ", "c ", "©", "G "] {
        if t.starts_with(prefix) {
            return true;
        }
    }
    // /min lines must be the coin rate, not cash (cash uses $) or random UI text.
    if lower.contains("min") {
        if has_coin_icon_prefix(t) {
            return true;
        }
        // Bare rate token without coin icon, e.g. anchor crop "85.8T/min".
        return is_bare_coin_rate_line(t);
    }

    // Standalone top-bar coin *balance* (total_coin mode): one token, huge-coin
    // suffix (q and above). Mid-screen combat stats use K/M/B/T and are excluded.
    if t.split_whitespace().count() == 1 {
        if let Some((byte_idx, _)) = t.char_indices().find(|(_, c)| c.is_ascii_alphabetic()) {
            let suffix = &t[byte_idx..];
            const COIN_BALANCE_SUFFIXES: &[&str] = &["q", "Q", "s", "S", "O", "N", "D"];
            if COIN_BALANCE_SUFFIXES.contains(&suffix)
                || (suffix.len() == 2 && suffix.bytes().all(|b| b.is_ascii_lowercase()))
            {
                return true;
            }
        }
    }

    false
}

/// A /min line with no coin icon — only accept if it looks like "85.8T/min",
/// not cash ("6.9M/min") or other UI text containing "min".
fn is_bare_coin_rate_line(text: &str) -> bool {
    let lower = text.to_lowercase();
    let Some(min_idx) = lower.rfind("min") else {
        return false;
    };
    let Some(sep) = lower[..min_idx].chars().last() else {
        return false;
    };
    if !matches!(sep, '/' | '(' | '\\' | '|' | ' ') {
        return false;
    }
    let body = &text[..min_idx - sep.len_utf8()];
    // Only digits, dots, and at most one rate-tier suffix (K/M/B/T).
    let body = body.trim();
    if body.is_empty() || body.split_whitespace().count() > 1 {
        return false;
    }
    let digit_or_dot = body
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .count();
    if digit_or_dot == 0 {
        return false;
    }
    let suffix = &body[digit_or_dot..];
    matches!(suffix, "" | "K" | "T")
}

/// Find "<keyword> <int>[+]" anywhere inside a line, tolerating separators.
/// OCR can merge the Tier/Wave panel with neighboring stats into one line,
/// e.g. "5.85q 44.65B/s@x3312.65 Tier 17+".
fn find_int_after(line: &str, keyword: &str) -> Option<(u32, bool)> {
    let lower = line.to_lowercase();
    let pos = lower.find(keyword)?;
    let rest = line[pos + keyword.len()..].trim_start_matches([' ', ':', '.']);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let plus = rest[digits.len()..].starts_with('+');
    Some((digits.parse().ok()?, plus))
}

/// Tier from the panel crop: "Tier 14" or a standalone "12" line.
pub(crate) fn find_tier_panel(line: &str) -> Option<(u32, bool)> {
    if let Some(v) = find_int_after(line, "tier") {
        return Some(v);
    }
    let lower = line.to_lowercase();
    for prefix in ["7ier ", "lier ", "tler ", "tier ", "er "] {
        if let Some(pos) = lower.find(prefix) {
            let rest = &line[pos + prefix.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(t) = digits.parse::<u32>() {
                if (1..=30).contains(&t) {
                    return Some((t, false));
                }
            }
        }
    }
    let trimmed = line.trim();
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        let t: u32 = trimmed.parse().ok()?;
        if (1..=30).contains(&t) {
            return Some((t, false));
        }
    }
    None
}

/// Wave from the panel crop: OCR often drops the leading W ("lave 4571", "Ive 3831").
pub(crate) fn find_wave_panel(line: &str) -> Option<u32> {
    if let Some((w, _)) = find_int_after(line, "wave") {
        return Some(w);
    }
    let lower = line.to_lowercase();
    for prefix in ["lave ", "ave ", "wve ", "vave ", "ive "] {
        if let Some(pos) = lower.find(prefix) {
            if let Some(w) = leading_wave_digits(&line[pos + prefix.len()..]) {
                return Some(w);
            }
        }
    }
    // "3825 66.09N" — wave number merged with per-wave coin stat.
    if let Some(w) = find_wave_stat_line(line) {
        return Some(w);
    }
    None
}

fn leading_wave_digits(rest: &str) -> Option<u32> {
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Wave as the first token when followed by a per-wave coin stat ("3825 66.09N").
fn find_wave_stat_line(line: &str) -> Option<u32> {
    let mut parts = line.split_whitespace();
    let first = parts.next()?;
    if !first.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let w: u32 = first.parse().ok()?;
    if w < 100 {
        return None;
    }
    let second = parts.next()?;
    let has_decimal = second.contains('.');
    let ends_coin_suffix = second
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_alphabetic());
    if has_decimal && ends_coin_suffix {
        Some(w)
    } else {
        None
    }
}

/// Bare wave number from a panel crop line (e.g. "4571" without "Wave").
fn find_bare_wave_panel(line: &str) -> Option<u32> {
    let trimmed = line.trim();
    // Timer strings like "0400" or "2:00" fragments — not wave numbers.
    if trimmed.len() > 1 && trimmed.starts_with('0') {
        return None;
    }
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(w) = trimmed.parse::<u32>() {
            if w >= 100 {
                return Some(w);
            }
        }
    }
    None
}

/// Classify a poll. Anchor crops are preferred when non-empty; otherwise the
/// main `lines` from the full window capture are used as fallback.
pub fn classify(
    lines: &[String],
    coin_lines: Option<&[String]>,
    tier_wave_lines: Option<&[String]>,
) -> PollInput {
    let mut tier: Option<u32> = None;
    let mut tournament = false;
    let mut wave: Option<u32> = None;
    let mut coin = CoinReading::Unreadable;
    let mut end_of_run = false;
    let mut intro_sprint = false;

    for line in lines {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();

        // End-of-run takes priority (Retry / GAME STATS screen).
        if lower == "retry" || lower.contains("game stats") {
            end_of_run = true;
        }
        if lower.contains("intro sprint") {
            intro_sprint = true;
        }
    }

    let panel_context = tier_wave_lines.is_some_and(|tw| !tw.is_empty());
    let tier_wave_source: Vec<&str> = match tier_wave_lines {
        Some(tw) if !tw.is_empty() => tw.iter().map(|s| s.as_str()).collect(),
        _ => lines.iter().map(|s| s.as_str()).collect(),
    };
    for line in tier_wave_source {
        let trimmed = line.trim();
        if tier.is_none() {
            if let Some((t, plus)) = find_tier_panel(trimmed) {
                tier = Some(t);
                tournament |= plus;
            }
        }
        if wave.is_none() {
            if let Some(w) = find_wave_panel(trimmed) {
                wave = Some(w);
            } else if panel_context {
                wave = find_bare_wave_panel(trimmed);
            }
        }
    }

    let coin_source: Vec<&str> = match coin_lines {
        Some(cl) if !cl.is_empty() => cl.iter().map(|s| s.as_str()).collect(),
        _ => lines.iter().map(|s| s.as_str()).collect(),
    };
    for line in coin_source {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Restricted coin crop: every non-empty line is the coin display.
        // Full-frame fallback: filter out combat stats and other UI noise.
        let from_anchor = coin_lines.is_some_and(|cl| !cl.is_empty());
        if from_anchor
            && (trimmed.starts_with(';')
                || trimmed.contains('$')
                || is_wave_progress_line(trimmed))
        {
            continue;
        }
        if from_anchor || is_likely_coin_line(trimmed) {
            let parsed = if from_anchor {
                parse_coin_anchor_crop(trimmed)
            } else {
                parse_coin_line(trimmed)
            };
            match parsed {
                r @ CoinReading::Rate(_) => {
                    coin = r;
                    if from_anchor {
                        break;
                    }
                }
                CoinReading::Total(v) if coin == CoinReading::Unreadable => {
                    coin = CoinReading::Total(v);
                }
                _ => {}
            }
        }
    }

    let mode = if end_of_run {
        GameMode::EndOfRun
    } else if tournament {
        GameMode::Tournament
    } else if intro_sprint {
        GameMode::IntroSprint
    } else {
        match coin {
            CoinReading::Rate(_) => GameMode::Normal,
            CoinReading::Total(_) => GameMode::TotalCoin,
            CoinReading::Unreadable => {
                if tier.is_some() || wave.is_some() {
                    GameMode::Normal
                } else {
                    GameMode::Unknown
                }
            }
        }
    };

    PollInput { mode, tier, wave, coin }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    // Scenarios mirror fixtures/expected.json.

    #[test]
    fn normal_full_game() {
        // expected_state_full_game.png
        let input = classify(&s(&[
            "$ 6.9M/min",
            "C 3.48T/min",
            "34567",
            "Tier 12",
            "Wave 4571",
        ]), None, None);
        assert_eq!(input.mode, GameMode::Normal);
        assert_eq!(input.tier, Some(12));
        assert_eq!(input.wave, Some(4571));
        assert_eq!(input.coin, CoinReading::Rate(3.48e12));
    }

    #[test]
    fn total_coin_screen() {
        // total_coin.png: balance instead of rate, gems as bare int
        let input = classify(&s(&[
            "$ 605.76M",
            "27.46q",
            "7938",
            "Tier 14",
            "Wave 450",
        ]), None, None);
        assert_eq!(input.mode, GameMode::TotalCoin);
        assert_eq!(input.tier, Some(14));
        assert_eq!(input.wave, Some(450));
        assert_eq!(input.coin, CoinReading::Total(27.46e15));
    }

    #[test]
    fn intro_sprint_screen() {
        // intro_sprint.png
        let input = classify(&s(&[
            "$ 341M/min",
            "C 0/min",
            "5029",
            "Intro Sprint",
            "Tier 14",
            "Wave 650",
        ]), None, None);
        assert_eq!(input.mode, GameMode::IntroSprint);
        assert_eq!(input.coin, CoinReading::Rate(0.0));
        assert_eq!(input.wave, Some(650));
    }

    #[test]
    fn tournament_screen() {
        // tournament.png: Tier 17+, total coins in top bar
        let input = classify(&s(&["$ 1.79B", "3.06q", "Tier 17+", "Wave 865"]), None, None);
        assert_eq!(input.mode, GameMode::Tournament);
        assert_eq!(input.tier, Some(17));
        assert_eq!(input.wave, Some(865));
        assert_eq!(input.coin, CoinReading::Total(3.06e15));
    }

    #[test]
    fn end_of_run_screen() {
        // end_of_run.png: GAME STATS + RETRY take priority
        let input = classify(&s(&[
            "GAME STATS",
            "Wave 5001",
            "Tier 11",
            "total coins: 31.82T",
            "RETRY",
            "HOME",
        ]), None, None);
        assert_eq!(input.mode, GameMode::EndOfRun);
    }

    #[test]
    fn gems_are_not_coins() {
        let input = classify(&s(&["5029", "Tier 14", "Wave 10"]), None, None);
        assert_eq!(input.coin, CoinReading::Unreadable);
        assert_eq!(input.mode, GameMode::Normal);
    }

    /// When OCR misses the coin/min line, combat stats must not flip mode.
    #[test]
    fn combat_stats_do_not_trigger_total_coin() {
        let input = classify(&s(&[
            "$ 6.9M/min",
            "2.98T",
            "8.78T",
            "591.53T / 35.85T",
            "Tier 14",
            "Wave 450",
        ]), None, None);
        assert_eq!(input.coin, CoinReading::Unreadable);
        assert_eq!(input.mode, GameMode::Normal);
    }

    /// Production path: coin parsed only from the anchor crop, not full frame.
    #[test]
    fn restricted_coin_crop_ignores_combat_stats() {
        let screen = s(&[
            "$ 6.9M/min",
            "2.98T",
            "8.78T",
            "591.53T / 35.85T",
            "Tier 14",
            "Wave 450",
        ]);
        let coin_crop = s(&["@ 3.48T/min"]);
        let input = classify(&screen, Some(&coin_crop), None);
        assert_eq!(input.mode, GameMode::Normal);
        assert_eq!(input.coin, CoinReading::Rate(3.48e12));
        assert_eq!(input.tier, Some(14));
        assert_eq!(input.wave, Some(450));
    }

    #[test]
    fn coin_rate_with_combat_stats_present() {
        let input = classify(&s(&[
            "@ 3.48T/min",
            "2.98T",
            "8.78T",
            "Tier 14",
            "Wave 450",
        ]), None, None);
        assert_eq!(input.mode, GameMode::Normal);
        assert_eq!(input.coin, CoinReading::Rate(3.48e12));
    }

    #[test]
    fn total_balance_with_fake_min_not_a_rate() {
        let input = classify(&s(&[
            "$ 6.9M/min",
            "@ 6.00q/min",
            "6.9M/min",
            "Tier 14",
            "Wave 450",
        ]), None, None);
        assert_eq!(input.coin, CoinReading::Unreadable);
        assert_eq!(input.mode, GameMode::Normal);
    }

    // Lines exactly as Windows OCR read the tournament.png fixture: the tier
    // panel merges with neighbor stats into a single line.
    #[test]
    fn tournament_real_ocr_lines() {
        let input = classify(&s(&[
            "$ 1.79B",
            "@ 3.Q6q-••-;-—",
            "2240",
            "865 / 900",
            "5.85q 44.65B/s@x3312.65 Tier 17+",
            "Wave 865",
            "x20.o",
        ]), None, None);
        assert_eq!(input.mode, GameMode::Tournament);
        assert_eq!(input.tier, Some(17));
        assert_eq!(input.wave, Some(865));
    }

    // Lines as OCR read expected_state_full_game.png: coin icon as "@",
    // "/min" as "(min".
    #[test]
    fn panel_crop_ocr_quirks() {
        let input = classify(
            &s(&[]),
            None,
            Some(&s(&["12", "lave 4571"])),
        );
        assert_eq!(input.tier, Some(12));
        assert_eq!(input.wave, Some(4571));
    }

    #[test]
    fn panel_crop_bare_wave_number() {
        let input = classify(&s(&[]), None, Some(&s(&["12", "4571"])));
        assert_eq!(input.tier, Some(12));
        assert_eq!(input.wave, Some(4571));
    }

    #[test]
    fn panel_crop_ive_prefix_is_wave() {
        let input = classify(&s(&[]), None, Some(&s(&["Ive 3831"])));
        assert_eq!(input.wave, Some(3831));
    }

    #[test]
    fn panel_crop_wave_stat_line() {
        let input = classify(&s(&[]), None, Some(&s(&["3825 66.09N"])));
        assert_eq!(input.wave, Some(3825));
    }

    #[test]
    fn panel_crop_ive_with_stat_suffix() {
        let input = classify(&s(&[]), None, Some(&s(&["Ive 3826 66.14N"])));
        assert_eq!(input.wave, Some(3826));
    }

    #[test]
    fn bare_wave_rejects_timer_zeros() {
        let input = classify(&s(&[]), None, Some(&s(&["0400"])));
        assert_eq!(input.wave, None);
    }

    /// Lines captured from a live 978×2084 emulator session (scanner.log).
    #[test]
    fn live_emulator_tier_wave_lines() {
        let input = classify(
            &s(&[]),
            None,
            Some(&s(&[
                "BATTLE",
                "x6.3 +",
                "4.17q",
                "Ive 3831",
                "73.66N",
                "Max",
                "overy",
            ])),
        );
        assert_eq!(input.wave, Some(3831));
        // Tier label was not in the live OCR crop; wave is the reliable field here.
        assert_eq!(input.tier, None);
    }

    #[test]
    fn full_game_real_ocr_lines() {
        let input = classify(&s(&[
            "@ 3.48 (min",
            "711.73T / 93.65T",
            "Tier 12",
            "Wave 4571",
        ]), None, None);
        assert_eq!(input.mode, GameMode::Normal);
        assert_eq!(input.tier, Some(12));
        assert_eq!(input.wave, Some(4571));
        // Suffix was lost by OCR here; magnitude is wrong but mode is right.
        assert_eq!(input.coin, CoinReading::Rate(3.48));
    }
}

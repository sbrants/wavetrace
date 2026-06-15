//! Classify a set of OCR lines into a PollInput, following the
//! "Game mode edge cases" decision flow in Goal.md.

use crate::parser::{has_coin_icon_prefix, parse_coin_line, CoinReading};
use crate::state_machine::{GameMode, PollInput};

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

/// Classify a full-frame Windows OCR poll.
///
/// * **Tier** — first number after the word `Tier` (case-insensitive).
/// * **Wave** — first number after the word `Wave` (case-insensitive).
/// * **Coin** — requires at least two lines containing `/min`; the **second**
///   match is parsed as the coin rate. Zero or one `/min` line → coin ignored.
pub fn classify(lines: &[String]) -> PollInput {
    let mut tournament = false;
    let mut end_of_run = false;
    let mut intro_sprint = false;

    for line in lines {
        let lower = line.trim().to_lowercase();
        if lower == "retry" || lower.contains("game stats") {
            end_of_run = true;
        }
        if lower.contains("intro sprint") {
            intro_sprint = true;
        }
    }

    let tier = extract_tier(lines, &mut tournament);
    let wave = extract_wave(lines);
    let coin = extract_coin_second_min(lines, tournament || end_of_run);

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

    PollInput {
        mode,
        tier,
        wave,
        coin,
    }
}

fn extract_tier(lines: &[String], tournament: &mut bool) -> Option<u32> {
    for line in lines {
        if let Some((t, plus)) = find_int_after(line, "tier") {
            *tournament |= plus;
            return Some(t);
        }
    }
    None
}

fn extract_wave(lines: &[String]) -> Option<u32> {
    for line in lines {
        if let Some((w, _)) = find_int_after(line, "wave") {
            return Some(w);
        }
    }
    None
}

/// Second `/min` line wins (first is usually cash `$…/min`).
/// Windows OCR often drops "/min" on the coin row (`@ 3.48T…`); fall back to `@` lines.
fn extract_coin_second_min(lines: &[String], prefer_total: bool) -> CoinReading {
    let min_lines: Vec<&str> = lines
        .iter()
        .map(|s| s.as_str())
        .filter(|l| {
            let lower = l.to_lowercase();
            if lower.contains("/min") {
                return true;
            }
            if crate::parser::is_spawn_rate_line(l) {
                return false;
            }
            has_coin_icon_prefix(l) && (lower.contains("/mi") || lower.contains("/m"))
        })
        .collect();
    if min_lines.len() >= 2 {
        return parse_coin_line(min_lines[1].trim());
    }
    if min_lines.len() == 1 && has_coin_icon_prefix(min_lines[0]) {
        return parse_coin_line(min_lines[0].trim());
    }

    let at_lines: Vec<&str> = lines
        .iter()
        .map(|s| s.as_str())
        .filter(|l| {
            let t = l.trim();
            !t.contains('$')
                && !crate::parser::is_spawn_rate_line(t)
                && (t.starts_with('@')
                    || t.starts_with("C ")
                    || t.starts_with("c ")
                    || t.starts_with("(Cc)")
                    || t.starts_with("(CC)")
                    || t.starts_with("(cc)"))
        })
        .collect();
    if let Some(line) = at_lines.first() {
        let reading = if prefer_total {
            parse_coin_line(line.trim())
        } else {
            crate::parser::parse_coin_anchor_crop(line.trim())
        };
        if matches!(reading, CoinReading::Rate(_) | CoinReading::Total(_)) {
            return reading;
        }
        let reading = parse_coin_line(line.trim());
        if matches!(reading, CoinReading::Rate(_)) {
            return reading;
        }
    }

    for line in lines {
        let t = line.trim().to_lowercase();
        if t.contains('$') {
            continue;
        }
        if (t.starts_with('o') || t.starts_with('0')) && t.contains("min") {
            let reading = parse_coin_line(line.trim());
            if matches!(reading, CoinReading::Rate(_)) {
                return reading;
            }
        }
    }

    extract_coin_balance(lines)
}

/// When `/min` and `@` heuristics miss, scan for bare total-coin balances (e.g. `2.72q`).
fn extract_coin_balance(lines: &[String]) -> CoinReading {
    let mut best: Option<(i32, CoinReading)> = None;
    for line in lines {
        if let Some(reading) = crate::parser::try_parse_balance_line(line) {
            let mut score = 0;
            if crate::parser::has_coin_icon_prefix(line) {
                score += 10;
            }
            if line.trim().chars().filter(|c| *c == '.').count() <= 1
                && !line.contains('/')
            {
                score += 5;
            }
            if best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                best = Some((score, reading));
            }
        }
    }
    best.map(|(_, r)| r).unwrap_or(CoinReading::Unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn windows_ocr_at_line_without_min_suffix() {
        let input = classify(&s(&["$ 6.9M/9-", "@ 3.48TVfnjn", "Tier 12", "Wave 4571"]));
        assert_eq!(input.tier, Some(12));
        assert_eq!(input.wave, Some(4571));
        assert_eq!(input.coin, CoinReading::Rate(3.48e12));
    }

    #[test]
    fn uses_second_min_line_for_coin() {
        let input = classify(&s(&["$ 6.9M/min", "C 3.48T/min", "Tier 12", "Wave 4571"]));
        assert_eq!(input.tier, Some(12));
        assert_eq!(input.wave, Some(4571));
        assert_eq!(input.coin, CoinReading::Rate(3.48e12));
    }

    #[test]
    fn ignores_frame_with_one_or_zero_min_lines() {
        let one = classify(&s(&["$ 6.9M/min", "Tier 14", "Wave 450"]));
        assert_eq!(one.coin, CoinReading::Unreadable);

        let none = classify(&s(&["Tier 14", "Wave 450"]));
        assert_eq!(none.coin, CoinReading::Unreadable);
    }

    #[test]
    fn intro_sprint_screen() {
        let input = classify(&s(&[
            "$ 341M/min",
            "C 0/min",
            "Intro Sprint",
            "Tier 14",
            "Wave 650",
        ]));
        assert_eq!(input.mode, GameMode::IntroSprint);
        assert_eq!(input.coin, CoinReading::Rate(0.0));
    }

    #[test]
    fn tournament_tier_plus() {
        let input = classify(&s(&["Tier 17+", "Wave 865"]));
        assert_eq!(input.mode, GameMode::Tournament);
        assert_eq!(input.tier, Some(17));
        assert_eq!(input.wave, Some(865));
    }

    #[test]
    fn end_of_run_screen() {
        let input = classify(&s(&["GAME STATS", "Wave 5001", "Tier 11", "RETRY"]));
        assert_eq!(input.mode, GameMode::EndOfRun);
    }

    #[test]
    fn tier_and_wave_require_labels() {
        let input = classify(&s(&["12", "4571", "lave 3831"]));
        assert_eq!(input.tier, None);
        assert_eq!(input.wave, None);

        let labeled = classify(&s(&["Tier 14", "Wave 1900"]));
        assert_eq!(labeled.tier, Some(14));
        assert_eq!(labeled.wave, Some(1900));
    }

    #[test]
    fn total_coin_balance_without_min_suffix() {
        let input = classify(&s(&["$ 6.9M/min", "2.72q", "Tier 14", "Wave 3556"]));
        assert_eq!(input.mode, GameMode::TotalCoin);
        assert_eq!(input.tier, Some(14));
        assert_eq!(input.wave, Some(3556));
        assert_eq!(input.coin, CoinReading::Total(2.72e15));
    }

    #[test]
    fn total_coin_balance_in_slash_compound_line() {
        let input = classify(&s(&["7.38q / 970.63T", "Tier 14", "Wave 450"]));
        assert_eq!(input.mode, GameMode::TotalCoin);
        assert_eq!(input.coin, CoinReading::Total(7.38e15));
    }

    #[test]
    fn total_coin_3_rejects_spawn_rate_and_balance_as_coin_per_min() {
        let lines = s(&[
            "$ 69.02B",
            "24.07q / 3.17q",
            "390.79M' 69.76T/s @",
            "2.47q / 151.10T",
            "7.06q",
            "Tier 14",
            "Wave 4217",
        ]);
        let input = classify(&lines);
        assert_eq!(input.mode, GameMode::TotalCoin);
        assert_eq!(input.tier, Some(14));
        assert_eq!(input.wave, Some(4217));
        assert_eq!(input.coin, CoinReading::Total(7.06e15));
        assert!(!matches!(input.coin, CoinReading::Rate(_)));
    }
}

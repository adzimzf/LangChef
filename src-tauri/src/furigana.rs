use lindera::mode::Mode;
use lindera::tokenizer::{Tokenizer, TokenizerBuilder};

/// Furigana annotator using lindera morphological analysis.
pub struct FuriganaAnnotator {
    tokenizer: Tokenizer,
}

impl FuriganaAnnotator {
    /// Create a new annotator with embedded IPADIC dictionary.
    pub fn new() -> Result<Self, String> {
        let mut builder = TokenizerBuilder::new()
            .map_err(|e| format!("Failed to create TokenizerBuilder: {}", e))?;
        builder.set_segmenter_dictionary("embedded://ipadic");
        builder.set_segmenter_mode(&Mode::Normal);
        let tokenizer = builder
            .build()
            .map_err(|e| format!("Failed to build tokenizer: {}", e))?;
        Ok(Self { tokenizer })
    }

    /// Annotate Japanese text with furigana readings.
    /// Produces `{kanji|reading}` format compatible with the frontend rendering pipeline.
    /// Only annotates kanji characters, leaving okurigana (trailing hiragana) unannotated.
    pub fn annotate(&mut self, text: &str) -> String {
        // Strip any existing annotations (safety measure)
        let plain = strip_existing_furigana(text);

        let tokens = match self.tokenizer.tokenize(&plain) {
            Ok(tokens) => tokens,
            Err(_) => return plain,
        };

        let mut result = String::with_capacity(plain.len() * 2);

        for mut token in tokens {
            let surface = token.surface.to_string();

            // Get reading field from IPADIC
            let reading_kata = token
                .get("reading")
                .filter(|r| *r != "*" && !r.is_empty());

            if let Some(reading_kata) = reading_kata {
                if has_kanji(&surface) {
                    let reading_hira = katakana_to_hiragana(reading_kata);
                    result.push_str(&annotate_kanji_only(&surface, &reading_hira));
                    continue;
                }
            }

            // No annotation needed: pure kana, punctuation, numbers, unknown words
            result.push_str(&surface);
        }

        result
    }
}

/// Annotate only the kanji portions of a surface form, preserving okurigana.
///
/// Examples:
/// - `食べ物` + `たべもの` → `{食|た}べ{物|もの}`
/// - `先生` + `せんせい` → `{先生|せんせい}`
/// - `行きました` + `いきました` → `{行|い}きました`
/// - `日本語` + `にほんご` → `{日本語|にほんご}`
fn annotate_kanji_only(surface: &str, reading: &str) -> String {
    let surface_chars: Vec<char> = surface.chars().collect();
    let reading_chars: Vec<char> = reading.chars().collect();
    let mut result = String::with_capacity(surface.len() * 3);
    let mut ri: usize = 0;

    let mut si: usize = 0;
    while si < surface_chars.len() {
        let ch = surface_chars[si];

        if is_kanji(ch) {
            // Collect consecutive kanji run
            let kanji_start = si;
            while si < surface_chars.len() && is_kanji(surface_chars[si]) {
                si += 1;
            }
            let kanji: String = surface_chars[kanji_start..si].iter().collect();

            // Collect trailing okurigana (hiragana after kanji)
            let okuri_start = si;
            while si < surface_chars.len() && is_hiragana(surface_chars[si]) {
                si += 1;
            }
            let okuri_len = si - okuri_start;

            // Determine the reading for the kanji portion
            let kanji_reading = if okuri_len > 0 {
                let okuri: String = surface_chars[okuri_start..si].iter().collect();
                // Find okurigana in reading to split between kanji reading and okurigana
                match find_substring_in_reading(&reading_chars[ri..], &okuri) {
                    Some(offset) => {
                        let kr: String = reading_chars[ri..ri + offset].iter().collect();
                        ri += offset;
                        kr
                    }
                    None => {
                        // Fallback: remaining reading minus okurigana length
                        let kr_len = reading_chars.len().saturating_sub(ri).saturating_sub(okuri.chars().count());
                        let kr: String = reading_chars[ri..ri + kr_len].iter().collect();
                        ri += kr_len;
                        kr
                    }
                }
            } else {
                // No okurigana — entire remaining reading belongs to this kanji run
                let kr: String = reading_chars[ri..].iter().collect();
                ri = reading_chars.len();
                kr
            };

            if !kanji_reading.is_empty() {
                result.push_str(&format!("{{{}|{}}}", kanji, kanji_reading));
            } else {
                result.push_str(&kanji);
            }

            // Append okurigana without annotation
            if okuri_len > 0 {
                let okuri: String = surface_chars[okuri_start..si].iter().collect();
                result.push_str(&okuri);
                ri += okuri.chars().count();
            }
        } else if is_hiragana(ch) {
            // Standalone hiragana (e.g., particles) — no annotation
            result.push(ch);
            if ri < reading_chars.len() {
                ri += 1;
            }
            si += 1;
        } else {
            // Katakana, punctuation, latin, digits — no annotation
            result.push(ch);
            si += 1;
        }
    }

    result
}

/// Find the position of `substr` chars within `reading` chars.
fn find_substring_in_reading(reading: &[char], substr: &str) -> Option<usize> {
    let substr_chars: Vec<char> = substr.chars().collect();
    if substr_chars.is_empty() || reading.len() < substr_chars.len() {
        return None;
    }
    'outer: for start in 0..=reading.len() - substr_chars.len() {
        for (i, &sc) in substr_chars.iter().enumerate() {
            if reading[start + i] != sc {
                continue 'outer;
            }
        }
        return Some(start);
    }
    None
}

fn is_kanji(ch: char) -> bool {
    matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}

fn is_hiragana(ch: char) -> bool {
    matches!(ch, '\u{3040}'..='\u{309F}')
}

fn has_kanji(text: &str) -> bool {
    text.chars().any(is_kanji)
}

/// Convert katakana to hiragana.
/// Range 0x30A1-0x30F6 → 0x3041-0x3096 (subtract 0x60).
fn katakana_to_hiragana(katakana: &str) -> String {
    katakana
        .chars()
        .map(|ch| {
            if ('\u{30A1}'..='\u{30F6}').contains(&ch) {
                char::from_u32(ch as u32 - 0x60).unwrap_or(ch)
            } else {
                ch
            }
        })
        .collect()
}

/// Strip existing `{kanji|reading}` annotations, returning plain text.
fn strip_existing_furigana(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut found_pipe = false;
            for inner_ch in chars.by_ref() {
                if inner_ch == '|' {
                    found_pipe = true;
                } else if inner_ch == '}' {
                    break;
                } else if !found_pipe {
                    result.push(inner_ch);
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_katakana_to_hiragana() {
        assert_eq!(katakana_to_hiragana("ニホンゴ"), "にほんご");
        assert_eq!(katakana_to_hiragana("タベモノ"), "たべもの");
        assert_eq!(katakana_to_hiragana("センセイ"), "せんせい");
        assert_eq!(katakana_to_hiragana("イキマシタ"), "いきました");
    }

    #[test]
    fn test_is_kanji() {
        assert!(is_kanji('日'));
        assert!(is_kanji('食'));
        assert!(!is_kanji('あ'));
        assert!(!is_kanji('ア'));
        assert!(!is_kanji('1'));
    }

    #[test]
    fn test_has_kanji() {
        assert!(has_kanji("食べ物"));
        assert!(has_kanji("日本語"));
        assert!(!has_kanji("たべもの"));
        assert!(!has_kanji("hello"));
    }

    #[test]
    fn test_annotate_kanji_only() {
        // Kanji + okurigana split
        assert_eq!(annotate_kanji_only("食べ物", "たべもの"), "{食|た}べ{物|もの}");
        // Consecutive kanji as one unit
        assert_eq!(annotate_kanji_only("先生", "せんせい"), "{先生|せんせい}");
        // Single kanji with okurigana
        assert_eq!(annotate_kanji_only("行きました", "いきました"), "{行|い}きました");
        // Consecutive kanji, no okurigana
        assert_eq!(annotate_kanji_only("日本語", "にほんご"), "{日本語|にほんご}");
        // Pure kana — no annotation
        assert_eq!(annotate_kanji_only("おいしい", "おいしい"), "おいしい");
    }

    #[test]
    fn test_strip_existing_furigana() {
        assert_eq!(
            strip_existing_furigana("{食|た}べ{物|もの}"),
            "食べ物"
        );
        assert_eq!(
            strip_existing_furigana("{日本|にほん}の{食|た}べ物"),
            "日本の食べ物"
        );
        assert_eq!(strip_existing_furigana("plain text"), "plain text");
    }
}

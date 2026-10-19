use lindera::mode::Mode;
use lindera::tokenizer::{Tokenizer, TokenizerBuilder};
use std::path::Path;

const IPADIC_SOURCE_URL: &str =
    "https://Lindera.dev/mecab-ipadic-2.7.0-20250920.tar.gz";
const IPADIC_SOURCE_DIR: &str = "mecab-ipadic-2.7.0-20250920";
const IPADIC_METADATA: &str =
    include_str!("../../lindera/lindera-ipadic/metadata.json");

pub struct FuriganaAnnotator {
    tokenizer: Tokenizer,
}

impl FuriganaAnnotator {
    pub fn from_path(dict_dir: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        let uri = format!("file:///{}", dict_dir.display()).replace('\\', "/");
        #[cfg(not(windows))]
        let uri = format!("file://{}", dict_dir.display());

        let mut builder = TokenizerBuilder::new()
            .map_err(|e| format!("TokenizerBuilder: {e}"))?;
        builder.set_segmenter_dictionary(&uri);
        builder.set_segmenter_mode(&Mode::Normal);
        let tokenizer = builder.build().map_err(|e| format!("build tokenizer: {e}"))?;
        Ok(Self { tokenizer })
    }

    pub fn annotate(&mut self, text: &str) -> String {
        let plain = strip_existing_furigana(text);

        let tokens = match self.tokenizer.tokenize(&plain) {
            Ok(tokens) => tokens,
            Err(_) => return plain,
        };

        let mut result = String::with_capacity(plain.len() * 2);

        for mut token in tokens {
            let surface = token.surface.to_string();

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

            result.push_str(&surface);
        }

        result
    }

    /// (surface, kana reading if known, is a particle) for each token, for romaji.
    pub fn readings(&mut self, text: &str) -> Vec<(String, Option<String>, bool)> {
        let plain = strip_existing_furigana(text);
        match self.tokenizer.tokenize(&plain) {
            Ok(tokens) => tokens
                .into_iter()
                .map(|mut token| {
                    let surface = token.surface.to_string();
                    let reading = token
                        .get("reading")
                        .filter(|r| *r != "*" && !r.is_empty())
                        .map(katakana_to_hiragana);
                    let particle = token.get("part_of_speech").is_some_and(|p| p == "助詞");
                    (surface, reading, particle)
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    pub fn tokenize(&mut self, text: &str) -> Vec<(String, String)> {
        let plain = strip_existing_furigana(text);
        match self.tokenizer.tokenize(&plain) {
            Ok(tokens) => tokens
                .into_iter()
                .map(|mut token| {
                    let surface = token.surface.to_string();
                    let base_form = token
                        .get("base_form")
                        .filter(|b| *b != "*" && !b.is_empty())
                        .unwrap_or(&surface)
                        .to_string();
                    (surface, base_form)
                })
                .collect(),
            Err(_) => vec![],
        }
    }
}

/// Returns true if the dict binary files already exist at `dir`.
pub fn is_dict_ready(dir: &Path) -> bool {
    dir.join("metadata.json").exists()
}

/// Ensures the IPADIC binary dict is present at `dir`.
/// Downloads the MeCab source from Lindera.dev and compiles it on first run.
/// `progress` is called with (stage, message, percent 0-100) throughout.
/// Subsequent calls return immediately via the `progress("ready", ...)` path.
pub fn ensure_dict(
    dir: &Path,
    progress: impl Fn(&str, &str, u8),
) -> Result<(), String> {
    if is_dict_ready(dir) {
        return Ok(());
    }
    download_and_build(dir, progress)
}

fn download_and_build(
    output_dir: &Path,
    progress: impl Fn(&str, &str, u8),
) -> Result<(), String> {
    use std::fs;
    use std::io::Read;

    let parent = output_dir.parent().ok_or("Invalid dict output path")?;
    let build_dir = parent.join("ipadic_build");

    if build_dir.exists() {
        fs::remove_dir_all(&build_dir).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&build_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

    // Download source tar.gz with streaming progress (0–70%)
    progress("downloading", "Downloading Japanese dictionary…", 0);
    let mut response = reqwest::blocking::get(IPADIC_SOURCE_URL)
        .map_err(|e| format!("Download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Download HTTP {}", response.status()));
    }
    let content_length = response.content_length();
    let mut raw: Vec<u8> = match content_length {
        Some(len) => Vec::with_capacity(len as usize),
        None => Vec::new(),
    };
    let mut buf = [0u8; 65536];
    let mut downloaded = 0u64;
    loop {
        let n = response.read(&mut buf).map_err(|e| format!("Download read: {e}"))?;
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        downloaded += n as u64;
        if let Some(total) = content_length {
            let pct = (downloaded.saturating_mul(70) / total).min(70) as u8;
            let mb_done = downloaded / 1_048_576;
            let mb_total = total / 1_048_576;
            progress(
                "downloading",
                &format!("Downloading… {mb_done} / {mb_total} MB"),
                pct,
            );
        }
    }

    // Extract tar.gz (70–80%)
    progress("extracting", "Extracting archive…", 71);
    let gz = flate2::read::GzDecoder::new(raw.as_slice());
    let mut archive = tar::Archive::new(gz);
    archive
        .unpack(&build_dir)
        .map_err(|e| format!("Extract failed: {e}"))?;
    progress("extracting", "Extracted.", 80);

    // Find the extracted source directory (could be named differently than expected)
    let input_dir = {
        let mut found = None;
        for entry in fs::read_dir(&build_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                found = Some(entry.path());
                break;
            }
        }
        found.ok_or_else(|| {
            format!(
                "No directory found in extracted archive at {}",
                build_dir.display()
            )
        })?
    };

    // Sanity-check that char.def exists before kicking off the slow build
    if !input_dir.join("char.def").exists() {
        let contents: Vec<_> = fs::read_dir(&input_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        return Err(format!(
            "char.def not found in {}. Contents: {:?}",
            input_dir.display(),
            &contents[..contents.len().min(10)]
        ));
    }

    // Build binary dict (80–99%) — this is CPU-bound and takes 30–120s
    progress("building", "Compiling dictionary (30–120 s)…", 81);
    let tmp_out = build_dir.join("out");
    let metadata: lindera::dictionary::Metadata = serde_json::from_str(IPADIC_METADATA)
        .map_err(|e| format!("Parse metadata: {e}"))?;
    let builder = lindera::dictionary::DictionaryBuilder::new(metadata);
    builder
        .build_dictionary(&input_dir, &tmp_out)
        .map_err(|e| format!("Build dict: {e}"))?;
    progress("building", "Dictionary compiled.", 99);

    // Move built files into output_dir
    for entry in fs::read_dir(&tmp_out).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        fs::copy(entry.path(), output_dir.join(entry.file_name()))
            .map_err(|e| e.to_string())?;
    }
    fs::remove_dir_all(&build_dir).ok();
    Ok(())
}

fn annotate_kanji_only(surface: &str, reading: &str) -> String {
    let surface_chars: Vec<char> = surface.chars().collect();
    let reading_chars: Vec<char> = reading.chars().collect();
    let mut result = String::with_capacity(surface.len() * 3);
    let mut ri: usize = 0;

    let mut si: usize = 0;
    while si < surface_chars.len() {
        let ch = surface_chars[si];

        if is_kanji(ch) {
            let kanji_start = si;
            while si < surface_chars.len() && is_kanji(surface_chars[si]) {
                si += 1;
            }
            let kanji: String = surface_chars[kanji_start..si].iter().collect();

            let okuri_start = si;
            while si < surface_chars.len() && is_hiragana(surface_chars[si]) {
                si += 1;
            }
            let okuri_len = si - okuri_start;

            let kanji_reading = if okuri_len > 0 {
                let okuri: String = surface_chars[okuri_start..si].iter().collect();
                match find_substring_in_reading(&reading_chars[ri..], &okuri) {
                    Some(offset) => {
                        let kr: String = reading_chars[ri..ri + offset].iter().collect();
                        ri += offset;
                        kr
                    }
                    None => {
                        let kr_len = reading_chars
                            .len()
                            .saturating_sub(ri)
                            .saturating_sub(okuri.chars().count());
                        let kr: String = reading_chars[ri..ri + kr_len].iter().collect();
                        ri += kr_len;
                        kr
                    }
                }
            } else {
                let kr: String = reading_chars[ri..].iter().collect();
                ri = reading_chars.len();
                kr
            };

            if !kanji_reading.is_empty() {
                result.push_str(&format!("{{{}|{}}}", kanji, kanji_reading));
            } else {
                result.push_str(&kanji);
            }

            if okuri_len > 0 {
                let okuri: String = surface_chars[okuri_start..si].iter().collect();
                result.push_str(&okuri);
                ri += okuri.chars().count();
            }
        } else if is_hiragana(ch) {
            result.push(ch);
            if ri < reading_chars.len() {
                ri += 1;
            }
            si += 1;
        } else {
            result.push(ch);
            si += 1;
        }
    }

    result
}

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
        assert_eq!(annotate_kanji_only("食べ物", "たべもの"), "{食|た}べ{物|もの}");
        assert_eq!(annotate_kanji_only("先生", "せんせい"), "{先生|せんせい}");
        assert_eq!(annotate_kanji_only("行きました", "いきました"), "{行|い}きました");
        assert_eq!(annotate_kanji_only("日本語", "にほんご"), "{日本語|にほんご}");
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

//! Offline Japanese voice: speech made on the device with piper-plus (VITS model, ONNX Runtime), so the JLPT
//! listening section has real audio on every platform, including Android where the webview has no Japanese voice.
//!
//! Nothing is bundled and nothing loads at start-up. "Download" (`tts_install`) fetches, into `<app data>/tts`:
//! - the voice model (Tsukuyomi-chan 6-language MB-iSTFT, the Tsukuyomi model piper-plus's own registry recommends);
//! - the NAIST-JDIC source, compiled on the device into the dictionary jpreprocess reads (like the furigana dictionary);
//! - on desktop, ONNX Runtime from Microsoft's release. Android has it in the APK (`onnxruntime-android` in Gradle).
//!
//! ONNX Runtime is always loaded at run time (`ort` `load-dynamic`): a missing or broken runtime is an error code, never a
//! crash, and the page falls back to the device voice or the script.

use crate::locale::app_error;
use piper_plus::{PiperVoice, SynthesisParams};
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{Emitter, Manager};

// The same files as `tsukuyomi-6lang-v2` in piper-plus's model registry. The earlier `releases/zs-v1` export is a
// fine-tune of the experimental zero-shot base, which expects a reference speaker embedding: without one it sounds robotic.
const MODEL_URL: &str = "https://huggingface.co/ayousanz/piper-plus-tsukuyomi-chan/resolve/main/tsukuyomi-chan-6lang-fp16.onnx";
const CONFIG_URL: &str = "https://huggingface.co/ayousanz/piper-plus-tsukuyomi-chan/resolve/main/config.json";
const DICT_URL: &str = "https://github.com/jpreprocess/naist-jdic/archive/refs/tags/v0.1.3.tar.gz";
/// Matches the ONNX Runtime API that `ort` 2.0.0-rc.15 is built against, and `onnxruntime-android` in Gradle.
const ORT_VERSION: &str = "1.31.1";

const MODEL_FILE: &str = "tsukuyomi-6lang.onnx";
const CONFIG_FILE: &str = "tsukuyomi-6lang.onnx.json";
/// The zs-v1 voice of earlier versions, deleted when the new one is downloaded.
const OLD_MODEL_FILES: [&str; 2] = ["tsukuyomi.onnx", "tsukuyomi.onnx.json"];
const DICT_DIR: &str = "naist-jdic";
/// Written last when the dictionary is complete, so a half-built one is never used.
const DICT_DONE: &str = ".built";
const ORT_DIR: &str = "onnxruntime";

/// Pause between the lines of a recording.
const LINE_GAP_SECONDS: f32 = 0.55;
/// Male lines are played slower than they were made, which lowers the pitch: the voice has one (female) speaker.
/// Kept small (about 1.4 semitones): a bigger shift moves the formants too and sounds like a slowed-down tape.
const MALE_PITCH: f32 = 0.92;

/// The loaded voice, kept between recordings (loading takes a moment and ~200 MB).
#[derive(Default)]
pub struct TtsState(Mutex<Option<PiperVoice>>);

fn tts_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("tts"))
}

// ---------------------------------------------------------------------------
// ONNX Runtime
// ---------------------------------------------------------------------------

/// Where Microsoft publishes ONNX Runtime for this desktop platform, and the library inside the archive.
/// None on Android (it's in the APK) and where no build exists (Intel Macs since 1.2x, Windows not shipped).
fn ort_download() -> Option<(String, &'static str)> {
    let (platform, lib) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("osx-arm64", "libonnxruntime.dylib"),
        ("linux", "x86_64") => ("linux-x64", "libonnxruntime.so"),
        ("linux", "aarch64") => ("linux-aarch64", "libonnxruntime.so"),
        _ => return None,
    };
    Some((
        format!("https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}/onnxruntime-{platform}-{ORT_VERSION}.tgz"),
        lib,
    ))
}

fn supported() -> bool {
    cfg!(target_os = "android") || ort_download().is_some()
}

/// The ONNX Runtime library to load: by name on Android (found in the APK), else the downloaded copy.
fn ort_library(dir: &Path) -> Option<PathBuf> {
    if cfg!(target_os = "android") {
        return Some(PathBuf::from("libonnxruntime.so"));
    }
    ort_download().map(|(_, lib)| dir.join(ORT_DIR).join(lib))
}

static ORT_LOADED: OnceLock<Result<(), String>> = OnceLock::new();

/// Loads ONNX Runtime once. Must succeed before any piper-plus call: without it `ort` panics on first use.
fn load_ort(dir: &Path) -> Result<(), String> {
    ORT_LOADED
        .get_or_init(|| {
            let lib = ort_library(dir).ok_or_else(|| app_error("tts_unsupported", std::env::consts::OS))?;
            ort::init_from(&lib).map(|_| ()).map_err(|e| app_error("tts_runtime_missing", e.to_string()))
        })
        .clone()
}

// ---------------------------------------------------------------------------
// Status and install
// ---------------------------------------------------------------------------

fn model_ready(dir: &Path) -> bool {
    dir.join(MODEL_FILE).is_file() && dir.join(CONFIG_FILE).is_file()
}

fn dict_ready(dir: &Path) -> bool {
    dir.join(DICT_DIR).join(DICT_DONE).is_file()
}

fn runtime_ready(dir: &Path) -> bool {
    cfg!(target_os = "android") || ort_library(dir).is_some_and(|p| p.is_file())
}

fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| match e.metadata() {
                    Ok(m) if m.is_dir() => dir_size(&e.path()),
                    Ok(m) => m.len(),
                    Err(_) => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

#[derive(Serialize)]
pub struct TtsStatus {
    /// This platform can run the offline voice.
    pub supported: bool,
    /// Everything is downloaded.
    pub installed: bool,
    /// Bytes used on the device.
    pub bytes: u64,
}

#[tauri::command]
pub fn tts_status(app: tauri::AppHandle) -> Result<TtsStatus, String> {
    let dir = tts_dir(&app)?;
    Ok(TtsStatus {
        supported: supported(),
        installed: supported() && model_ready(&dir) && dict_ready(&dir) && runtime_ready(&dir),
        bytes: dir_size(&dir),
    })
}

/// Progress of `tts_install`, sent as the `tts-progress` event. `stage`: voice, dictionary, building, runtime, checking.
#[derive(Serialize, Clone)]
struct Progress {
    stage: &'static str,
    percent: u8,
}

/// Downloads `url` into `dest` (through a `.part` file), reporting 0–100.
fn download(url: &str, dest: &Path, progress: &dyn Fn(u8)) -> Result<(), String> {
    let mut resp = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(None)
        .build()
        .and_then(|c| c.get(url).send())
        .map_err(|e| app_error("tts_download_failed", format!("{url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(app_error("tts_download_failed", format!("{url}: HTTP {}", resp.status())));
    }
    let total = resp.content_length();
    let part = dest.with_extension("part");
    let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = resp.read(&mut buf).map_err(|e| app_error("tts_download_failed", format!("{url}: {e}")))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if let Some(total) = total.filter(|t| *t > 0) {
            progress((done * 100 / total).min(100) as u8);
        }
    }
    drop(file);
    fs::rename(&part, dest).map_err(|e| e.to_string())
}

/// Compiles the NAIST-JDIC source (MeCab CSV) into the dictionary jpreprocess reads.
fn build_dictionary(source_tgz: &Path, dir: &Path) -> Result<(), String> {
    use lindera_core::dictionary_builder::DictionaryBuilder;
    let work = dir.join("naist-jdic-src");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let tgz = fs::File::open(source_tgz).map_err(|e| e.to_string())?;
    tar::Archive::new(flate2::read::GzDecoder::new(tgz))
        .unpack(&work)
        .map_err(|e| app_error("tts_dictionary_failed", format!("extract: {e}")))?;
    let source = fs::read_dir(&work)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("char.def").is_file())
        .ok_or_else(|| app_error("tts_dictionary_failed", "char.def not found in the archive"))?;
    let out = dir.join(DICT_DIR);
    let _ = fs::remove_dir_all(&out);
    let builder = jpreprocess_dictionary_builder::ipadic_builder::IpadicBuilder::new(Box::new(
        jpreprocess_dictionary::serializer::jpreprocess::JPreprocessSerializer,
    ));
    builder
        .build_dictionary(&source, &out)
        .map_err(|e| app_error("tts_dictionary_failed", e.to_string()))?;
    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_file(source_tgz);
    fs::write(out.join(DICT_DONE), b"ok").map_err(|e| e.to_string())
}

/// The real ONNX Runtime library in Microsoft's archive: `<root>/lib/libonnxruntime.so.1.31.1` or
/// `<root>/lib/libonnxruntime.1.31.1.dylib`. Not the version symlinks, `libonnxruntime_providers_shared.so`, the
/// pkg-config file or the same-named debug copy inside `.dSYM/`.
fn is_runtime_file(path: &Path, lib: &str) -> bool {
    let wanted = match lib.strip_suffix(".dylib") {
        Some(stem) => format!("{stem}.{ORT_VERSION}.dylib"),
        None => format!("{lib}.{ORT_VERSION}"),
    };
    let parent_is_lib = path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "lib");
    let in_root_lib = path.components().count() == 3;
    parent_is_lib && in_root_lib && path.file_name().is_some_and(|n| n.to_string_lossy() == wanted)
}

/// Takes the ONNX Runtime library out of Microsoft's release archive.
fn extract_runtime(tgz: &Path, dir: &Path, lib: &str) -> Result<(), String> {
    let file = fs::File::open(tgz).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let out_dir = dir.join(ORT_DIR);
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if entry.header().entry_type().is_file() && is_runtime_file(&path, lib) {
            entry.unpack(out_dir.join(lib)).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(tgz);
            return Ok(());
        }
    }
    Err(app_error("tts_runtime_missing", format!("{lib} not found in the ONNX Runtime archive")))
}

/// Keeps only Japanese in the multilingual voice's `language_id_map`. piper-plus then reads every line with the
/// Japanese phonemizer: a line of kanji only (何曜日ですか without kana would be one) is never read as Chinese, and
/// Latin letters are read by Open JTalk. The model still gets Japanese's language id.
fn japanese_only_config(path: &Path) -> Result<(), String> {
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| app_error("tts_load_failed", e.to_string()))?;
    if let Some(map) = config.get_mut("language_id_map").and_then(|m| m.as_object_mut()) {
        let ja = map.get("ja").cloned().ok_or_else(|| app_error("tts_load_failed", "no ja in language_id_map"))?;
        map.clear();
        map.insert("ja".into(), ja);
    }
    fs::write(path, serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// Takes out the spaces JLPT scripts put between words for beginners (飲みものを 買いましょうか): Open JTalk reads
/// each space as a break, which chops a sentence into flat, separate pieces. Spaces next to Latin text stay.
fn join_words(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let latin = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    chars
        .iter()
        .enumerate()
        .filter(|(i, c)| !(c.is_whitespace() && !latin(chars.get(i.wrapping_sub(1))) && !latin(chars.get(i + 1))))
        .map(|(_, c)| *c)
        .collect()
}

fn set_dictionary_env(dir: &Path) {
    // piper-plus finds the jpreprocess dictionary through this variable, and must never fetch one itself.
    std::env::set_var("JPREPROCESS_DICT", dir.join(DICT_DIR));
    std::env::set_var("PIPER_PLUS_OFFLINE_MODE", "1");
}

/// The compiled dictionary really reads Japanese: a missing one would otherwise turn into a silent pass-through.
fn check_dictionary(dir: &Path) -> Result<(), String> {
    set_dictionary_env(dir);
    let config: piper_plus::VoiceConfig = serde_json::from_value(serde_json::json!({
        "audio": { "sample_rate": 22050 }, "phoneme_type": "openjtalk", "phoneme_id_map": { "_": [0] }, "num_symbols": 1, "num_speakers": 1
    }))
    .map_err(|e| e.to_string())?;
    let phonemizer = PiperVoice::create_phonemizer(&config, None).map_err(|e| app_error("tts_dictionary_failed", e.to_string()))?;
    let (tokens, _) = phonemizer
        .phonemize_with_prosody("こんにちは")
        .map_err(|e| app_error("tts_dictionary_failed", e.to_string()))?;
    if tokens.iter().any(|t| t == "k") { Ok(()) } else { Err(app_error("tts_dictionary_failed", format!("{tokens:?}"))) }
}

fn load_voice(dir: &Path) -> Result<PiperVoice, String> {
    load_ort(dir)?;
    set_dictionary_env(dir);
    let model = dir.join(MODEL_FILE);
    let config = dir.join(CONFIG_FILE);
    // piper-plus may panic on a broken model; that must not take the app down.
    std::panic::catch_unwind(|| PiperVoice::load(&model, Some(&config), "cpu"))
        .map_err(|_| app_error("tts_load_failed", "panic while loading the voice"))?
        .map_err(|e| app_error("tts_load_failed", e.to_string()))
}

/// Downloads whatever is missing, compiles the dictionary and checks the voice loads. Progress: `tts-progress`.
#[tauri::command]
pub async fn tts_install(app: tauri::AppHandle) -> Result<TtsStatus, String> {
    if !supported() {
        return Err(app_error("tts_unsupported", std::env::consts::OS));
    }
    let dir = tts_dir(&app)?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let emit = |stage: &'static str, percent: u8| {
            let _ = handle.emit("tts-progress", Progress { stage, percent });
        };
        if !model_ready(&dir) {
            for old in OLD_MODEL_FILES {
                let _ = fs::remove_file(dir.join(old));
            }
            download(CONFIG_URL, &dir.join(CONFIG_FILE), &|_| {})?;
            japanese_only_config(&dir.join(CONFIG_FILE))?;
            download(MODEL_URL, &dir.join(MODEL_FILE), &|p| emit("voice", p))?;
        }
        if !dict_ready(&dir) {
            let tgz = dir.join("naist-jdic.tar.gz");
            download(DICT_URL, &tgz, &|p| emit("dictionary", p))?;
            emit("building", 0);
            build_dictionary(&tgz, &dir)?;
        }
        if !runtime_ready(&dir) {
            if let (Some((url, lib)), false) = (ort_download(), cfg!(target_os = "android")) {
                let tgz = dir.join("onnxruntime.tgz");
                download(&url, &tgz, &|p| emit("runtime", p))?;
                extract_runtime(&tgz, &dir, lib)?;
            }
        }
        emit("checking", 0);
        check_dictionary(&dir)?;
        let voice = load_voice(&dir)?;
        if let Ok(mut slot) = handle.state::<TtsState>().0.lock() {
            *slot = Some(voice);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    tts_status(app)
}

/// Deletes the downloaded voice, dictionary and runtime.
#[tauri::command]
pub fn tts_remove(app: tauri::AppHandle, state: tauri::State<'_, TtsState>) -> Result<TtsStatus, String> {
    if let Ok(mut slot) = state.0.lock() {
        *slot = None;
    }
    let dir = tts_dir(&app)?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    tts_status(app)
}

// ---------------------------------------------------------------------------
// Speaking
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Clone, Copy)]
enum Speaker {
    Male,
    Female,
    Narrator,
}

/// Splits the 男：/女： (or 男:) speaker mark off a script line.
fn speaker_of(line: &str) -> (Speaker, &str) {
    let line = line.trim();
    for (mark, who) in [("男", Speaker::Male), ("女", Speaker::Female)] {
        for sep in ["：", ":"] {
            if let Some(rest) = line.strip_prefix(mark).and_then(|r| r.strip_prefix(sep)) {
                return (who, rest.trim());
            }
        }
    }
    (Speaker::Narrator, line)
}

/// Plays `samples` slower by `factor` (< 1): lower pitch and longer, by linear interpolation.
fn lower_pitch(samples: &[i16], factor: f32) -> Vec<i16> {
    if samples.len() < 2 || !(0.1..1.0).contains(&factor) {
        return samples.to_vec();
    }
    let out_len = (samples.len() as f32 / factor) as usize;
    (0..out_len)
        .map(|i| {
            let pos = i as f32 * factor;
            let j = pos.floor() as usize;
            let frac = pos - j as f32;
            let a = samples[j.min(samples.len() - 1)] as f32;
            let b = samples[(j + 1).min(samples.len() - 1)] as f32;
            (a + (b - a) * frac).round() as i16
        })
        .collect()
}

/// A mono 16-bit PCM WAV file.
fn wav_bytes(sample_rate: u32, samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Speaks the lines one after another with a pause between them, men a little lower. Returns (sample rate, samples).
fn speak(voice: &mut PiperVoice, lines: &[String]) -> Result<(u32, Vec<i16>), String> {
    let mut audio: Vec<i16> = Vec::new();
    let mut rate = 22050;
    for line in lines {
        let (who, text) = speaker_of(line);
        let text = join_words(text);
        if text.is_empty() {
            continue;
        }
        let params = SynthesisParams {
            language_override: Some("ja".to_string()),
            // Men are slowed afterwards to lower the pitch, so they're made that much faster here: same speed in the end.
            length_scale: if who == Speaker::Male { MALE_PITCH } else { 1.0 },
            // piper-plus's own noise defaults (tuned for its models).
            ..Default::default()
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| voice.synthesize_with_params(&text, &params)))
            .map_err(|_| app_error("tts_failed", "panic while speaking"))?
            .map_err(|e| app_error("tts_failed", e.to_string()))?;
        rate = result.sample_rate;
        if !audio.is_empty() {
            audio.extend(std::iter::repeat_n(0, (rate as f32 * LINE_GAP_SECONDS) as usize));
        }
        if who == Speaker::Male {
            audio.extend(lower_pitch(&result.audio, MALE_PITCH));
        } else {
            audio.extend(result.audio);
        }
    }
    Ok((rate, audio))
}

/// Speaks a recording's lines with the offline voice and returns a WAV file.
#[tauri::command]
pub async fn tts_speak(app: tauri::AppHandle, lines: Vec<String>) -> Result<tauri::ipc::Response, String> {
    let dir = tts_dir(&app)?;
    if !(supported() && model_ready(&dir) && dict_ready(&dir) && runtime_ready(&dir)) {
        return Err(app_error("tts_not_installed", ""));
    }
    let handle = app.clone();
    let wav = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let state = handle.state::<TtsState>();
        let mut slot = state.0.lock().map_err(|e| e.to_string())?;
        if slot.is_none() {
            *slot = Some(load_voice(&dir)?);
        }
        let voice = slot.as_mut().expect("loaded above");
        let (rate, samples) = speak(voice, &lines)?;
        Ok(wav_bytes(rate, &samples))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(wav))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_speaker_marks() {
        assert_eq!(speaker_of("男：飲みものを 買いましょうか。"), (Speaker::Male, "飲みものを 買いましょうか。"));
        assert_eq!(speaker_of(" 女:はい。"), (Speaker::Female, "はい。"));
        assert_eq!(speaker_of("男の 人と 女の 人が 話して います。"), (Speaker::Narrator, "男の 人と 女の 人が 話して います。"));
    }

    #[test]
    fn joins_spaced_words_but_not_latin() {
        assert_eq!(join_words("飲みものを 買いましょうか。"), "飲みものを買いましょうか。");
        assert_eq!(join_words("あした　ピクニックに 行きます"), "あしたピクニックに行きます");
        assert_eq!(join_words("この T シャツ"), "この T シャツ");
        assert_eq!(join_words("New York"), "New York");
    }

    #[test]
    fn config_keeps_only_japanese() {
        let dir = std::env::temp_dir().join(format!("langchef-tts-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.json");
        fs::write(&path, r#"{"num_languages":6,"language_id_map":{"ja":0,"en":1,"zh":2},"audio":{"sample_rate":22050}}"#).unwrap();
        japanese_only_config(&path).unwrap();
        let c: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(c["language_id_map"], serde_json::json!({"ja": 0}));
        assert_eq!(c["num_languages"], 6, "the model still has 6 language ids");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lower_pitch_stretches_and_keeps_the_shape() {
        let s: Vec<i16> = (0..100).map(|i| (i * 10) as i16).collect();
        let out = lower_pitch(&s, 0.5);
        assert_eq!(out.len(), 200);
        assert_eq!(out[0], 0);
        assert_eq!(out[2], 10);
        assert_eq!(out[1], 5, "interpolated between samples");
        assert_eq!(lower_pitch(&s, 1.0), s, "no change at factor 1");
    }

    #[test]
    fn wav_header_is_valid() {
        let wav = wav_bytes(22050, &[0, 1, -1]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 22050);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(wav.len(), 44 + 6);
        assert_eq!(i16::from_le_bytes([wav[48], wav[49]]), -1);
    }

    #[test]
    fn runtime_download_matches_the_platform() {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => {
                let (url, lib) = ort_download().unwrap();
                assert!(url.ends_with(&format!("onnxruntime-linux-x64-{ORT_VERSION}.tgz")));
                assert_eq!(lib, "libonnxruntime.so");
            }
            ("macos", "x86_64") => assert!(ort_download().is_none(), "no Intel Mac build of ONNX Runtime"),
            _ => {}
        }
    }

    #[test]
    fn picks_the_real_runtime_library_from_the_archive() {
        let v = ORT_VERSION;
        let linux = |p: &str| is_runtime_file(Path::new(p), "libonnxruntime.so");
        assert!(linux(&format!("onnxruntime-linux-x64-{v}/lib/libonnxruntime.so.{v}")));
        assert!(!linux(&format!("onnxruntime-linux-x64-{v}/lib/libonnxruntime_providers_shared.so")));
        assert!(!linux(&format!("onnxruntime-linux-x64-{v}/lib/libonnxruntime.so")));
        assert!(!linux(&format!("onnxruntime-linux-x64-{v}/lib/pkgconfig/libonnxruntime.pc")));
        let mac = |p: &str| is_runtime_file(Path::new(p), "libonnxruntime.dylib");
        assert!(mac(&format!("onnxruntime-osx-arm64-{v}/lib/libonnxruntime.{v}.dylib")));
        assert!(!mac(&format!("onnxruntime-osx-arm64-{v}/lib/libonnxruntime.{v}.dylib.dSYM/Contents/Resources/DWARF/libonnxruntime.{v}.dylib")));
        assert!(!mac(&format!("onnxruntime-osx-arm64-{v}/lib/pkgconfig/libonnxruntime.pc")));
    }

    /// End to end with real files, run by hand:
    /// `TTS_TEST_DIR=<dir with tsukuyomi-6lang.onnx(.json), naist-jdic/ and onnxruntime/> cargo test --lib tts -- --ignored`
    /// (`naist-jdic.tar.gz` / `onnxruntime.tgz` there are compiled / extracted first when missing).
    #[test]
    #[ignore]
    fn speaks_with_real_files() {
        let dir = PathBuf::from(std::env::var("TTS_TEST_DIR").expect("TTS_TEST_DIR"));
        if !dict_ready(&dir) {
            build_dictionary(&dir.join("naist-jdic.tar.gz"), &dir).unwrap();
        }
        if let (false, Some((_, lib))) = (runtime_ready(&dir), ort_download()) {
            extract_runtime(&dir.join("onnxruntime.tgz"), &dir, lib).unwrap();
        }
        check_dictionary(&dir).unwrap();
        let mut voice = load_voice(&dir).unwrap();
        let lines = vec!["男：あしたの ピクニック、何を 持って いきますか。".to_string(), "女：くだものを おねがいします。".to_string()];
        let (rate, audio) = speak(&mut voice, &lines).unwrap();
        assert!(rate > 0 && audio.len() > rate as usize, "at least a second of audio");
        fs::write(dir.join("test.wav"), wav_bytes(rate, &audio)).unwrap();
    }
}

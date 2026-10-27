//! Offline Japanese voice: speech made on the device with VOICEVOX CORE, so the JLPT listening section has real audio
//! on every platform, including Android where the webview has no Japanese voice.
//!
//! Nothing is bundled and nothing loads at start-up. "Download" (`tts_install`) fetches, into `<app data>/tts`:
//! - the voice model: VOICEVOX Nemo (`n0.vvm`), plain adult voices made for reading aloud, men and women;
//! - the Open JTalk dictionary (reading and pitch accent);
//! - VOICEVOX's build of ONNX Runtime. VOICEVOX models are encrypted and only this build can open them, so it is
//!   downloaded on Android too, not taken from the APK.
//!
//! ONNX Runtime is always loaded at run time (`load-onnxruntime`): a missing or broken runtime is an error code, never a
//! crash, and the page falls back to the device voice or the script.

use crate::locale::app_error;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use voicevox_core::blocking::{Onnxruntime, OpenJtalk, Synthesizer, VoiceModelFile};
use voicevox_core::StyleId;

/// Release of `VOICEVOX/voicevox_vvm` the model comes from (VOICEVOX CORE 0.17 reads models 0.16–0.17).
const MODEL_URL: &str = "https://github.com/VOICEVOX/voicevox_vvm/releases/download/0.17.0/n0.vvm";
const DICT_URL: &str = "https://github.com/r9y9/open_jtalk/releases/download/v1.11.1/open_jtalk_dic_utf_8-1.11.tar.gz";
/// VOICEVOX ONNX Runtime release; must match `Onnxruntime::LIB_RECOMMENDED_VERSION` of the `voicevox_core` tag in Cargo.toml.
const ORT_VERSION: &str = "1.23.2";

const MODEL_FILE: &str = "n0.vvm";
const DICT_DIR: &str = "open_jtalk_dic_utf_8-1.11";
const ORT_DIR: &str = "onnxruntime";
/// What earlier versions (piper-plus) downloaded, deleted on install.
const OLD_FILES: [&str; 5] = ["tsukuyomi.onnx", "tsukuyomi.onnx.json", "tsukuyomi-6lang.onnx", "tsukuyomi-6lang.onnx.json", "naist-jdic"];

/// VOICEVOX Nemo styles (`voicevox_vvm` README): 女声1, 男声1, and 女声2 for lines without a speaker mark.
const WOMAN: u32 = 10005;
const MAN: u32 = 10001;
const NARRATOR: u32 = 10007;

/// Pause between the lines of a dialog, and the longer one around the narrator's lines (as in the test room).
const LINE_GAP_SECONDS: f32 = 0.55;
const NARRATOR_GAP_SECONDS: f32 = 1.0;
/// The narrator reads like a test announcer: a little slower and with flatter intonation than the people talking.
const NARRATOR_SPEED: f32 = 0.9;
const NARRATOR_INTONATION: f32 = 0.8;

/// The loaded voice, kept between recordings (loading takes a moment).
#[derive(Default)]
pub struct TtsState(Mutex<Option<Synthesizer<OpenJtalk>>>);

fn tts_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("tts"))
}

// ---------------------------------------------------------------------------
// ONNX Runtime
// ---------------------------------------------------------------------------

/// VOICEVOX's ONNX Runtime archive for this platform. None where VOICEVOX publishes no build.
fn ort_download() -> Option<String> {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("android", "aarch64") => "android-arm64",
        ("android", "x86_64") => "android-x64",
        ("macos", "aarch64") => "osx-arm64",
        ("macos", "x86_64") => "osx-x86_64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "win-x64",
        _ => return None,
    };
    Some(format!(
        "https://github.com/VOICEVOX/onnxruntime-builder/releases/download/voicevox_onnxruntime-{ORT_VERSION}/voicevox_onnxruntime-{platform}-{ORT_VERSION}.tgz"
    ))
}

fn supported() -> bool {
    ort_download().is_some()
}

/// The library file, named as VOICEVOX CORE expects it (`libvoicevox_onnxruntime.so.1.23.2`, `….so` on Android, …).
fn ort_library(dir: &Path) -> PathBuf {
    dir.join(ORT_DIR).join(Onnxruntime::LIB_RECOMMENDED_VERSIONED_FILENAME)
}

/// Loads ONNX Runtime once per process (later calls return the loaded one).
fn load_ort(dir: &Path) -> Result<&'static Onnxruntime, String> {
    Onnxruntime::load_once()
        .filename(ort_library(dir))
        .perform()
        .map_err(|e| app_error("tts_runtime_missing", e.to_string()))
}

// ---------------------------------------------------------------------------
// Status and install
// ---------------------------------------------------------------------------

fn model_ready(dir: &Path) -> bool {
    dir.join(MODEL_FILE).is_file()
}

fn dict_ready(dir: &Path) -> bool {
    dir.join(DICT_DIR).join("sys.dic").is_file()
}

fn runtime_ready(dir: &Path) -> bool {
    ort_library(dir).is_file()
}

fn installed(dir: &Path) -> bool {
    supported() && model_ready(dir) && dict_ready(dir) && runtime_ready(dir)
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
    Ok(TtsStatus { supported: supported(), installed: installed(&dir), bytes: dir_size(&dir) })
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

/// Unpacks the dictionary archive (one `open_jtalk_dic_utf_8-1.11/` folder) into `dir`.
fn extract_dictionary(tgz: &Path, dir: &Path) -> Result<(), String> {
    let _ = fs::remove_dir_all(dir.join(DICT_DIR));
    let file = fs::File::open(tgz).map_err(|e| e.to_string())?;
    tar::Archive::new(flate2::read::GzDecoder::new(file))
        .unpack(dir)
        .map_err(|e| app_error("tts_dictionary_failed", format!("extract: {e}")))?;
    let _ = fs::remove_file(tgz);
    if dict_ready(dir) { Ok(()) } else { Err(app_error("tts_dictionary_failed", "sys.dic not found in the archive")) }
}

/// The library in VOICEVOX's archive: `<root>/lib/<name>` as a real file (not a version symlink).
fn is_runtime_file(path: &Path, lib: &str) -> bool {
    let parent_is_lib = path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "lib");
    parent_is_lib && path.components().count() == 3 && path.file_name().is_some_and(|n| n.to_string_lossy() == lib)
}

/// Takes the ONNX Runtime library out of VOICEVOX's release archive.
fn extract_runtime(tgz: &Path, dir: &Path) -> Result<(), String> {
    let lib = Onnxruntime::LIB_RECOMMENDED_VERSIONED_FILENAME;
    let file = fs::File::open(tgz).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    fs::create_dir_all(dir.join(ORT_DIR)).map_err(|e| e.to_string())?;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        if entry.header().entry_type().is_file() && is_runtime_file(&path, lib) {
            entry.unpack(ort_library(dir)).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(tgz);
            return Ok(());
        }
    }
    Err(app_error("tts_runtime_missing", format!("{lib} not found in the ONNX Runtime archive")))
}

fn load_voice(dir: &Path) -> Result<Synthesizer<OpenJtalk>, String> {
    let ort = load_ort(dir)?;
    let dict = dir.join(DICT_DIR);
    let dict = dict.to_str().ok_or_else(|| app_error("tts_dictionary_failed", "path is not UTF-8"))?;
    let open_jtalk = OpenJtalk::new(dict).map_err(|e| app_error("tts_dictionary_failed", e.to_string()))?;
    let synth = Synthesizer::builder(ort)
        .text_analyzer(open_jtalk)
        .build()
        .map_err(|e| app_error("tts_load_failed", e.to_string()))?;
    let model = VoiceModelFile::open(dir.join(MODEL_FILE)).map_err(|e| app_error("tts_load_failed", e.to_string()))?;
    synth.load_voice_model(&model).perform().map_err(|e| app_error("tts_load_failed", e.to_string()))?;
    Ok(synth)
}

/// Downloads whatever is missing and checks the voice really speaks. Progress: `tts-progress`.
#[tauri::command]
pub async fn tts_install(app: tauri::AppHandle) -> Result<TtsStatus, String> {
    let Some(ort_url) = ort_download() else {
        return Err(app_error("tts_unsupported", std::env::consts::OS));
    };
    let dir = tts_dir(&app)?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for old in OLD_FILES {
            let old = dir.join(old);
            let _ = if old.is_dir() { fs::remove_dir_all(old) } else { fs::remove_file(old) };
        }
        let emit = |stage: &'static str, percent: u8| {
            let _ = handle.emit("tts-progress", Progress { stage, percent });
        };
        if !model_ready(&dir) {
            download(MODEL_URL, &dir.join(MODEL_FILE), &|p| emit("voice", p))?;
        }
        if !dict_ready(&dir) {
            let tgz = dir.join("open_jtalk_dic.tar.gz");
            download(DICT_URL, &tgz, &|p| emit("dictionary", p))?;
            emit("building", 0);
            extract_dictionary(&tgz, &dir)?;
        }
        if !runtime_ready(&dir) {
            let tgz = dir.join("onnxruntime.tgz");
            download(&ort_url, &tgz, &|p| emit("runtime", p))?;
            extract_runtime(&tgz, &dir)?;
        }
        emit("checking", 0);
        let synth = load_voice(&dir)?;
        // Speaking once proves the runtime can open the (encrypted) model and the dictionary reads Japanese.
        speak(&synth, &["はい。".to_string()])?;
        if let Ok(mut slot) = handle.state::<TtsState>().0.lock() {
            *slot = Some(synth);
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

impl Speaker {
    fn style(self) -> StyleId {
        StyleId::new(match self {
            Speaker::Male => MAN,
            Speaker::Female => WOMAN,
            Speaker::Narrator => NARRATOR,
        })
    }
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

/// Takes out the spaces JLPT scripts put between words for beginners (飲みものを 買いましょうか): Open JTalk reads
/// each space as a break, which chops a sentence into separate pieces. Spaces next to Latin text stay.
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

/// The lines as they are spoken: like the real test, the narrator's question (the last sentence of the opening narrator
/// line, when it asks something) is asked again after the dialog.
fn with_repeated_question(lines: &[String]) -> Vec<String> {
    let mut out = lines.to_vec();
    let Some(first) = lines.first().filter(|l| speaker_of(l).0 == Speaker::Narrator) else { return out };
    let question = first
        .split_inclusive('。')
        .map(str::trim)
        .rfind(|s| !s.is_empty())
        .filter(|s| s.trim_end_matches(['。', '？', '?']).ends_with('か'));
    let last_dialog = lines.iter().rposition(|l| speaker_of(l).0 != Speaker::Narrator);
    if let (Some(question), Some(at)) = (question, last_dialog) {
        out.insert(at + 1, question.to_string());
    }
    out
}

/// Sample rate and samples of a mono 16-bit WAV as VOICEVOX writes it (44-byte header).
fn wav_samples(wav: &[u8]) -> Option<(u32, Vec<i16>)> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[36..40] != b"data" {
        return None;
    }
    let rate = u32::from_le_bytes(wav[24..28].try_into().ok()?);
    Some((rate, wav[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()))
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

/// One line in its speaker's voice; the narrator slower and flatter. Returns a WAV file.
fn speak_line(synth: &Synthesizer<OpenJtalk>, who: Speaker, text: &str) -> Result<Vec<u8>, String> {
    let failed = |e: String| app_error("tts_failed", e);
    if who != Speaker::Narrator {
        return synth.tts(text, who.style()).perform().map_err(|e| failed(e.to_string()));
    }
    // AudioQuery's scales are typed floats; set them through its (stable, ENGINE-compatible) JSON form.
    let query = synth.create_audio_query(text, who.style()).map_err(|e| failed(e.to_string()))?;
    let mut json = serde_json::to_value(&query).map_err(|e| failed(e.to_string()))?;
    json["speedScale"] = NARRATOR_SPEED.into();
    json["intonationScale"] = NARRATOR_INTONATION.into();
    let query: voicevox_core::AudioQuery = serde_json::from_value(json).map_err(|e| failed(e.to_string()))?;
    synth.synthesis(&query, who.style()).perform().map_err(|e| failed(e.to_string()))
}

/// Speaks the lines one after another with a pause between them, each in its speaker's voice. Returns a WAV file.
fn speak(synth: &Synthesizer<OpenJtalk>, lines: &[String]) -> Result<Vec<u8>, String> {
    let mut audio: Vec<i16> = Vec::new();
    let mut rate = 24000;
    let mut last: Option<Speaker> = None;
    for line in with_repeated_question(lines) {
        let (who, text) = speaker_of(&line);
        let text = join_words(text);
        if text.is_empty() {
            continue;
        }
        let wav = speak_line(synth, who, &text)?;
        let (line_rate, samples) = wav_samples(&wav).ok_or_else(|| app_error("tts_failed", "unexpected WAV"))?;
        rate = line_rate;
        if let Some(prev) = last {
            let gap = if prev == Speaker::Narrator || who == Speaker::Narrator { NARRATOR_GAP_SECONDS } else { LINE_GAP_SECONDS };
            audio.extend(std::iter::repeat_n(0, (rate as f32 * gap) as usize));
        }
        audio.extend(samples);
        last = Some(who);
    }
    Ok(wav_bytes(rate, &audio))
}

/// Speaks a recording's lines with the offline voice and returns a WAV file.
#[tauri::command]
pub async fn tts_speak(app: tauri::AppHandle, lines: Vec<String>) -> Result<tauri::ipc::Response, String> {
    let dir = tts_dir(&app)?;
    if !installed(&dir) {
        return Err(app_error("tts_not_installed", ""));
    }
    let handle = app.clone();
    let wav = tauri::async_runtime::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let state = handle.state::<TtsState>();
        let mut slot = state.0.lock().map_err(|e| e.to_string())?;
        if slot.is_none() {
            *slot = Some(load_voice(&dir)?);
        }
        speak(slot.as_ref().expect("loaded above"), &lines)
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
    fn narrator_question_is_asked_again_after_the_dialog() {
        let s = |v: &[&str]| v.iter().map(|l| l.to_string()).collect::<Vec<_>>();
        let lines = s(&["男の人と女の人が話しています。男の人はこのあと何をしますか。", "男：ケーキを買おう。", "女：いいね。", "いち。ケーキを買う"]);
        assert_eq!(
            with_repeated_question(&lines),
            s(&["男の人と女の人が話しています。男の人はこのあと何をしますか。", "男：ケーキを買おう。", "女：いいね。", "男の人はこのあと何をしますか。", "いち。ケーキを買う"])
        );
        // No question in the narrator line (概要理解), or no dialog (発話表現): nothing is added.
        let summary = s(&["ラジオで男の人が話しています。", "男：きょうは…"]);
        assert_eq!(with_repeated_question(&summary), summary);
        let no_dialog = s(&["友だちに本を借りたいです。何と言いますか。", "いち。貸して"]);
        assert_eq!(with_repeated_question(&no_dialog), no_dialog);
    }

    #[test]
    fn wav_round_trip() {
        let wav = wav_bytes(24000, &[0, 1, -1]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
        assert_eq!(wav_samples(&wav), Some((24000, vec![0, 1, -1])));
        assert_eq!(wav_samples(b"not a wav"), None);
    }

    #[test]
    fn runtime_download_matches_the_platform() {
        if let ("linux", "x86_64") = (std::env::consts::OS, std::env::consts::ARCH) {
            let url = ort_download().unwrap();
            assert!(url.ends_with(&format!("/voicevox_onnxruntime-{ORT_VERSION}/voicevox_onnxruntime-linux-x64-{ORT_VERSION}.tgz")));
            assert_eq!(Onnxruntime::LIB_RECOMMENDED_VERSIONED_FILENAME, format!("libvoicevox_onnxruntime.so.{ORT_VERSION}"));
        }
        assert_eq!(Onnxruntime::LIB_RECOMMENDED_VERSION, ORT_VERSION, "ORT_VERSION follows voicevox_core");
    }

    #[test]
    fn picks_the_real_runtime_library_from_the_archive() {
        let v = ORT_VERSION;
        let pick = |p: &str, lib: &str| is_runtime_file(Path::new(p), lib);
        let so = format!("libvoicevox_onnxruntime.so.{v}");
        assert!(pick(&format!("voicevox_onnxruntime-linux-x64-{v}/lib/{so}"), &so));
        assert!(!pick(&format!("voicevox_onnxruntime-linux-x64-{v}/lib/libvoicevox_onnxruntime.so"), &so));
        assert!(!pick(&format!("voicevox_onnxruntime-linux-x64-{v}/lib/pkgconfig/{so}"), &so));
        assert!(pick(&format!("voicevox_onnxruntime-android-arm64-{v}/lib/libvoicevox_onnxruntime.so"), "libvoicevox_onnxruntime.so"));
    }

    /// End to end with real files, run by hand:
    /// `TTS_TEST_DIR=<dir with n0.vvm, open_jtalk_dic_utf_8-1.11/ and onnxruntime/<lib>> cargo test --lib tts -- --ignored`
    /// (`onnxruntime.tgz` there is extracted first when the library is missing). Writes `test.wav`.
    #[test]
    #[ignore]
    fn speaks_with_real_files() {
        let dir = PathBuf::from(std::env::var("TTS_TEST_DIR").expect("TTS_TEST_DIR"));
        if !runtime_ready(&dir) {
            extract_runtime(&dir.join("onnxruntime.tgz"), &dir).unwrap();
        }
        let synth = load_voice(&dir).unwrap();
        let lines = vec![
            "男の 人と 女の 人が 話して います。".to_string(),
            "男：あしたの ピクニック、何を 持って いきますか。".to_string(),
            "女：くだものを おねがいします。".to_string(),
        ];
        let wav = speak(&synth, &lines).unwrap();
        let (rate, audio) = wav_samples(&wav).unwrap();
        assert!(rate > 0 && audio.len() > rate as usize * 3, "at least three seconds of audio");
        fs::write(dir.join("test.wav"), wav).unwrap();
    }
}

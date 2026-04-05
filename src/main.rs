//! astra-phonemizer: GPL v3 espeak-ng wrapper for Astra TTS pipeline.
//!
//! Communicates with the Astra daemon via ndjson over stdin/stdout.
//! Long-lived child process: starts, prints version handshake, then
//! reads PhonemeRequest lines from stdin and writes PhonemeResponse lines to stdout.

use std::ffi::{CStr, CString};
use std::io::{self, BufRead, Write};
use std::os::raw::{c_char, c_int, c_void};

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// espeak-ng FFI declarations (minimal subset we need)
// ---------------------------------------------------------------------------

/// Output mode: don't produce audio, just phonemize.
const AUDIO_OUTPUT_RETRIEVAL: c_int = 0x2000;

extern "C" {
    fn espeak_Initialize(
        output: c_int,
        buflength: c_int,
        path: *const c_char,
        options: c_int,
    ) -> c_int;

    fn espeak_SetVoiceByName(name: *const c_char) -> c_int;

    fn espeak_TextToPhonemes(
        textptr: *mut *const c_void,
        textmode: c_int,
        phonememode: c_int,
    ) -> *const c_char;

    fn espeak_Info(path_data: *mut *const c_char) -> *const c_char;
}

// ---------------------------------------------------------------------------
// ndjson protocol messages (must match astra-daemon/src/voice/piper/models.rs)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct VersionHandshake {
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    espeak_version: Option<String>,
}

#[derive(Deserialize)]
struct PhonemeRequest {
    id: String,
    text: String,
    language: String,
}

#[derive(Serialize)]
struct PhonemeResponse {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    phonemes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// ---------------------------------------------------------------------------
// espeak-ng helpers
// ---------------------------------------------------------------------------

fn init_espeak() -> Result<(), String> {
    // On Windows, espeak-ng data is bundled alongside the binary.
    // On Linux/macOS, system-installed espeak-ng-data is used (default path).
    let path_cstring = if cfg!(windows) {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .and_then(|d| CString::new(d.to_string_lossy().as_bytes()).ok())
    } else {
        None
    };

    let path_ptr = path_cstring
        .as_ref()
        .map_or(std::ptr::null(), |p| p.as_ptr());

    let rc = unsafe { espeak_Initialize(AUDIO_OUTPUT_RETRIEVAL, 0, path_ptr, 0) };
    if rc < 0 {
        return Err(format!("espeak_Initialize failed with code {}", rc));
    }
    Ok(())
}

fn get_espeak_version() -> Option<String> {
    unsafe {
        let mut path_data: *const c_char = std::ptr::null();
        let version = espeak_Info(&mut path_data);
        if version.is_null() {
            None
        } else {
            Some(CStr::from_ptr(version).to_string_lossy().into_owned())
        }
    }
}

fn set_voice(language: &str) -> Result<(), String> {
    let c_lang = CString::new(language).map_err(|e| format!("invalid language: {}", e))?;
    let rc = unsafe { espeak_SetVoiceByName(c_lang.as_ptr()) };
    if rc != 0 {
        // EE_NOT_FOUND = 2, EE_INTERNAL_ERROR = -1
        return Err(format!(
            "espeak_SetVoiceByName('{}') failed with code {}",
            language, rc
        ));
    }
    Ok(())
}

fn text_to_phonemes(text: &str) -> Result<String, String> {
    let c_text = CString::new(text).map_err(|e| format!("invalid text: {}", e))?;

    let text_ptr: *const c_void = c_text.as_ptr() as *const c_void;
    let mut ptr = text_ptr;

    let mut result = String::new();

    // espeak_TextToPhonemes advances the pointer through the text,
    // returning phonemes for each word/segment until the pointer reaches the end.
    loop {
        let phonemes = unsafe {
            // textmode=0 (UTF-8), phonememode=2 (IPA)
            espeak_TextToPhonemes(&mut ptr, 0, 2)
        };

        if phonemes.is_null() {
            break;
        }

        let segment = unsafe { CStr::from_ptr(phonemes) }
            .to_string_lossy()
            .into_owned();

        if segment.is_empty() {
            break;
        }

        if !result.is_empty() {
            result.push(' ');
        }
        result.push_str(&segment);

        // If the pointer didn't advance (end of text), stop
        if ptr.is_null() || ptr == text_ptr {
            break;
        }
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    // Initialize espeak-ng
    if let Err(e) = init_espeak() {
        let err = PhonemeResponse {
            id: String::new(),
            phonemes: None,
            error: Some(format!("Failed to initialize espeak-ng: {}", e)),
        };
        let _ = serde_json::to_writer(io::stdout().lock(), &err);
        let _ = io::stdout().lock().write_all(b"\n");
        std::process::exit(1);
    }

    // Print version handshake (first line of stdout)
    let handshake = VersionHandshake {
        version: env!("CARGO_PKG_VERSION").to_string(),
        espeak_version: get_espeak_version(),
    };
    {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        serde_json::to_writer(&mut out, &handshake).expect("failed to write handshake");
        out.write_all(b"\n").expect("failed to write newline");
        out.flush().expect("failed to flush stdout");
    }

    // Read ndjson requests from stdin, write responses to stdout
    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // EOF or read error
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: PhonemeRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                // Write error response with empty id
                let resp = PhonemeResponse {
                    id: String::new(),
                    phonemes: None,
                    error: Some(format!("invalid request JSON: {}", e)),
                };
                let mut out = stdout.lock();
                let _ = serde_json::to_writer(&mut out, &resp);
                let _ = out.write_all(b"\n");
                let _ = out.flush();
                continue;
            }
        };

        // Set the voice/language for this request
        let response = match set_voice(&request.language) {
            Ok(()) => {
                // Phonemize
                match text_to_phonemes(&request.text) {
                    Ok(phonemes) => PhonemeResponse {
                        id: request.id,
                        phonemes: Some(vec![phonemes]),
                        error: None,
                    },
                    Err(e) => PhonemeResponse {
                        id: request.id,
                        phonemes: None,
                        error: Some(e),
                    },
                }
            }
            Err(e) => PhonemeResponse {
                id: request.id,
                phonemes: None,
                error: Some(e),
            },
        };

        let mut out = stdout.lock();
        let _ = serde_json::to_writer(&mut out, &response);
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }
}

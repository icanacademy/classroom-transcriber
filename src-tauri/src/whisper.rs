use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WhisperError {
    #[error("Transcriber not found")]
    TranscriberNotFound,
    #[error("Transcription failed: {0}")]
    TranscriptionError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerStats {
    pub word_count: usize,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub segments: Vec<TranscriptSegment>,
    pub speakers: std::collections::HashMap<String, SpeakerStats>,
    pub full_transcript: String,
}

pub struct Transcriber {
    binary_path: PathBuf,
    hf_token: Option<String>,
}

impl Transcriber {
    pub fn new() -> Result<Self, WhisperError> {
        // Find the bundled whisperx-transcriber binary
        // In development, it's in src-tauri/binaries/
        // In production, it's in the app bundle next to the main executable

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));

        let mut possible_paths = vec![];

        // Production paths - next to the executable
        if let Some(ref dir) = exe_dir {
            // Windows: whisperx-transcriber.exe
            possible_paths.push(dir.join("whisperx-transcriber.exe"));
            // macOS: whisperx-transcriber
            possible_paths.push(dir.join("whisperx-transcriber"));
        }

        // Development paths - macOS
        possible_paths.push(PathBuf::from("binaries/whisperx-transcriber-x86_64-apple-darwin"));
        possible_paths.push(PathBuf::from("src-tauri/binaries/whisperx-transcriber-x86_64-apple-darwin"));

        // Development paths - Windows
        possible_paths.push(PathBuf::from("binaries/whisperx-transcriber-x86_64-pc-windows-msvc.exe"));
        possible_paths.push(PathBuf::from("src-tauri/binaries/whisperx-transcriber-x86_64-pc-windows-msvc.exe"));

        for path in &possible_paths {
            if path.exists() {
                return Ok(Self {
                    binary_path: path.clone(),
                    hf_token: std::env::var("HF_TOKEN").ok(),
                });
            }
        }

        Err(WhisperError::TranscriberNotFound)
    }

    pub fn set_hf_token(&mut self, token: String) {
        self.hf_token = Some(token);
    }

    pub fn transcribe(&self, audio_path: &PathBuf) -> Result<String, WhisperError> {
        let result = self.transcribe_with_diarization(audio_path)?;
        Ok(self.extract_labeled_transcript(&result))
    }

    pub fn transcribe_with_diarization(&self, audio_path: &PathBuf) -> Result<TranscriptionResult, WhisperError> {
        let mut cmd = Command::new(&self.binary_path);
        cmd.arg(audio_path.to_str().unwrap());
        cmd.arg("--model");
        cmd.arg("tiny");

        // Add HF token if available for speaker diarization
        if let Some(ref token) = self.hf_token {
            cmd.arg("--hf-token");
            cmd.arg(token);
        } else {
            cmd.arg("--no-diarize");
        }

        // Set environment
        cmd.env("KMP_DUPLICATE_LIB_OK", "TRUE");

        let output = cmd
            .output()
            .map_err(|e| WhisperError::TranscriptionError(format!("Failed to run transcriber: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WhisperError::TranscriptionError(format!(
                "Transcription failed: {}",
                stderr
            )));
        }

        // Parse JSON output
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Find the JSON line (skip any warning lines)
        for line in stdout.lines() {
            if line.starts_with('{') {
                let result: TranscriptionResult = serde_json::from_str(line)
                    .map_err(|e| WhisperError::TranscriptionError(format!("Failed to parse JSON: {}", e)))?;
                return Ok(result);
            }
        }

        Err(WhisperError::TranscriptionError("No JSON output from transcription".to_string()))
    }

    fn extract_labeled_transcript(&self, result: &TranscriptionResult) -> String {
        // If no diarization or only one speaker, return full transcript
        if result.speakers.len() <= 1 {
            return result.full_transcript.clone();
        }

        // Sort speakers by word count
        let mut speakers_by_words: Vec<_> = result.speakers.iter().collect();
        speakers_by_words.sort_by(|a, b| b.1.word_count.cmp(&a.1.word_count));

        // Create speaker labels: Speaker 1, Speaker 2, etc.
        let mut speaker_labels: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for (i, (speaker, _)) in speakers_by_words.iter().enumerate() {
            let label = format!("Speaker {}", i + 1);
            speaker_labels.insert((*speaker).clone(), label);
        }

        // Format transcript with speaker labels
        let labeled_text: Vec<String> = result.segments
            .iter()
            .map(|s| {
                let label = speaker_labels.get(&s.speaker).unwrap_or(&s.speaker);
                format!("[{}] {}", label, s.text)
            })
            .collect();

        labeled_text.join("\n")
    }
}

pub fn get_whisper_status() -> String {
    match Transcriber::new() {
        Ok(_) => {
            let hf_status = if std::env::var("HF_TOKEN").is_ok() {
                "Speaker diarization enabled"
            } else {
                "Speaker diarization disabled (no HF_TOKEN)"
            };
            format!("WhisperX ready. {}", hf_status)
        }
        Err(_) => "Transcriber not found".to_string(),
    }
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WhisperError {
    #[error("Model not found at {0}")]
    ModelNotFound(String),
    #[error("Python environment not found")]
    PythonNotFound,
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
    python_path: PathBuf,
    script_path: PathBuf,
    hf_token: Option<String>,
}

impl Transcriber {
    pub fn new(app_dir: &PathBuf) -> Result<Self, WhisperError> {
        // Find Python in the whisperx-env
        let python_path = app_dir.join("whisperx-env").join("bin").join("python");

        if !python_path.exists() {
            // Try alternate locations
            let alt_python = PathBuf::from("/Users/edward/classroom-transcriber/whisperx-env/bin/python");
            if alt_python.exists() {
                return Ok(Self {
                    python_path: alt_python,
                    script_path: PathBuf::from("/Users/edward/classroom-transcriber/whisperx_transcribe.py"),
                    hf_token: std::env::var("HF_TOKEN").ok(),
                });
            }
            return Err(WhisperError::PythonNotFound);
        }

        let script_path = app_dir.join("whisperx_transcribe.py");

        Ok(Self {
            python_path,
            script_path,
            hf_token: std::env::var("HF_TOKEN").ok(),
        })
    }

    pub fn set_hf_token(&mut self, token: String) {
        self.hf_token = Some(token);
    }

    pub fn transcribe(&self, audio_path: &PathBuf) -> Result<String, WhisperError> {
        // Call Python script for transcription with diarization
        let result = self.transcribe_with_diarization(audio_path)?;

        // For backwards compatibility, return just the student's transcript
        // Filter out likely teacher segments (usually the one who talks less or asks questions)
        Ok(self.extract_student_transcript(&result))
    }

    pub fn transcribe_with_diarization(&self, audio_path: &PathBuf) -> Result<TranscriptionResult, WhisperError> {
        let mut cmd = Command::new(&self.python_path);
        cmd.arg(&self.script_path);
        cmd.arg(audio_path.to_str().unwrap());
        cmd.arg("--model");
        cmd.arg("tiny"); // Use tiny for speed, can be configurable later

        // Add HF token if available
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
            .map_err(|e| WhisperError::TranscriptionError(format!("Failed to run Python script: {}", e)))?;

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

    fn extract_student_transcript(&self, result: &TranscriptionResult) -> String {
        // If no diarization or only one speaker, return full transcript
        if result.speakers.len() <= 1 {
            return result.full_transcript.clone();
        }

        // Find the speaker who talked the most (likely the student)
        // In a classroom recording, the student is usually the one who speaks more
        let mut speakers_by_words: Vec<_> = result.speakers.iter().collect();
        speakers_by_words.sort_by(|a, b| b.1.word_count.cmp(&a.1.word_count));

        // Create speaker labels: Speaker 1, Speaker 2, etc. (sorted by who talks most)
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

    /// Get a formatted transcript with speaker labels
    pub fn get_labeled_transcript(&self, result: &TranscriptionResult) -> String {
        result.segments
            .iter()
            .map(|s| format!("[{}] {}", s.speaker, s.text))
            .collect::<Vec<String>>()
            .join("\n")
    }
}

pub fn check_whisper_installed() -> bool {
    // Check if Python environment exists
    let python_path = PathBuf::from("/Users/edward/classroom-transcriber/whisperx-env/bin/python");
    python_path.exists()
}

pub fn get_whisper_status() -> String {
    let python_path = PathBuf::from("/Users/edward/classroom-transcriber/whisperx-env/bin/python");
    let script_path = PathBuf::from("/Users/edward/classroom-transcriber/whisperx_transcribe.py");

    if !python_path.exists() {
        return "Python environment not found. Please run setup.".to_string();
    }

    if !script_path.exists() {
        return "Transcription script not found.".to_string();
    }

    // Check if HF_TOKEN is set
    let hf_status = if std::env::var("HF_TOKEN").is_ok() {
        "Speaker diarization enabled"
    } else {
        "Speaker diarization disabled (no HF_TOKEN)"
    };

    format!("WhisperX ready. {}", hf_status)
}

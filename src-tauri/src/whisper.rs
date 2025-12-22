use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum WhisperError {
    #[error("Model not found at {0}")]
    ModelNotFound(String),
    #[error("Whisper CLI not found. Please install: brew install whisper-cpp")]
    CliNotFound,
    #[error("Transcription failed: {0}")]
    TranscriptionError(String),
}

pub struct Transcriber {
    model_path: PathBuf,
    whisper_cli: PathBuf,
}

impl Transcriber {
    pub fn new(model_path: &PathBuf) -> Result<Self, WhisperError> {
        if !model_path.exists() {
            return Err(WhisperError::ModelNotFound(
                model_path.to_string_lossy().to_string(),
            ));
        }

        // Find whisper CLI
        let whisper_cli = find_whisper_cli()?;

        Ok(Self {
            model_path: model_path.clone(),
            whisper_cli,
        })
    }

    pub fn transcribe(&self, audio_path: &PathBuf) -> Result<String, WhisperError> {
        // Run whisper CLI
        let output = Command::new(&self.whisper_cli)
            .args([
                "-m",
                self.model_path.to_str().unwrap(),
                "-f",
                audio_path.to_str().unwrap(),
                "-l",
                "en",
                "--no-timestamps",
                "-otxt",
            ])
            .output()
            .map_err(|e| WhisperError::TranscriptionError(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WhisperError::TranscriptionError(stderr.to_string()));
        }

        // Read the output text file (whisper creates .txt file next to input)
        let txt_path = audio_path.with_extension("wav.txt");
        if txt_path.exists() {
            let transcript = std::fs::read_to_string(&txt_path)
                .map_err(|e| WhisperError::TranscriptionError(e.to_string()))?;
            // Clean up the txt file
            let _ = std::fs::remove_file(&txt_path);
            return Ok(transcript.trim().to_string());
        }

        // Fallback: parse stdout
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().to_string())
    }
}

fn find_whisper_cli() -> Result<PathBuf, WhisperError> {
    // Common locations for whisper CLI
    let mut candidates = vec![
        // macOS Homebrew locations
        "/usr/local/bin/whisper-cli".to_string(),
        "/opt/homebrew/bin/whisper-cli".to_string(),
        "/usr/local/bin/whisper-cpp".to_string(),
        "/opt/homebrew/bin/whisper-cpp".to_string(),
    ];

    // Windows locations
    #[cfg(target_os = "windows")]
    {
        candidates.push("C:\\Program Files\\whisper-cpp\\whisper-cli.exe".to_string());
        candidates.push("C:\\Program Files (x86)\\whisper-cpp\\whisper-cli.exe".to_string());

        // Check in app's own directory
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                candidates.push(exe_dir.join("whisper-cli.exe").to_string_lossy().to_string());
                candidates.push(exe_dir.join("whisper-cli").to_string_lossy().to_string());
            }
        }

        // Check in LOCALAPPDATA
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            candidates.push(format!("{}\\whisper-cpp\\whisper-cli.exe", local_app_data));
        }
    }

    for path in &candidates {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    // Try to find via which (Unix) or where (Windows)
    #[cfg(not(target_os = "windows"))]
    {
        for cmd in &["whisper-cli", "whisper-cpp"] {
            if let Ok(output) = Command::new("which").arg(cmd).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout);
                    let p = PathBuf::from(path.trim());
                    if p.exists() {
                        return Ok(p);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        for cmd in &["whisper-cli.exe", "whisper-cli", "whisper-cpp.exe"] {
            if let Ok(output) = Command::new("where").arg(cmd).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout);
                    let first_line = path.lines().next().unwrap_or("").trim();
                    let p = PathBuf::from(first_line);
                    if p.exists() {
                        return Ok(p);
                    }
                }
            }
        }
    }

    Err(WhisperError::CliNotFound)
}

pub fn check_whisper_installed() -> bool {
    find_whisper_cli().is_ok()
}

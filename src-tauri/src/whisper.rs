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
        // Build the command
        let mut cmd = Command::new(&self.whisper_cli);

        // On Windows, add directories to PATH so whisper-cli can find DLLs
        #[cfg(target_os = "windows")]
        {
            let mut paths_to_add = Vec::new();

            // Add whisper-cli directory
            if let Some(cli_dir) = self.whisper_cli.parent() {
                paths_to_add.push(cli_dir.to_path_buf());
            }

            // Add resources directory (where Tauri bundles additional files)
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    // Check resources subfolder
                    let resources_dir = exe_dir.join("resources");
                    if resources_dir.exists() {
                        paths_to_add.push(resources_dir);
                    }
                    // Also add exe directory itself
                    paths_to_add.push(exe_dir.to_path_buf());
                }
            }

            if !paths_to_add.is_empty() {
                let current_path = std::env::var("PATH").unwrap_or_default();
                let new_paths: Vec<String> = paths_to_add.iter().map(|p| p.display().to_string()).collect();
                let new_path = format!("{};{}", new_paths.join(";"), current_path);
                cmd.env("PATH", new_path);
            }
        }

        // Run whisper CLI
        let output = cmd
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
            .map_err(|e| WhisperError::TranscriptionError(format!("Failed to run whisper-cli: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(WhisperError::TranscriptionError(format!(
                "Whisper failed. stderr: {} stdout: {}",
                stderr, stdout
            )));
        }

        // Try multiple possible output file paths
        // whisper-cli creates .txt file, but the exact naming varies by version
        let possible_txt_paths = vec![
            audio_path.with_extension("txt"),           // recording.txt (replaces .wav)
            audio_path.with_extension("wav.txt"),       // recording.wav.txt (appends .txt)
            {
                // Same directory, same base name + .txt
                let mut p = audio_path.clone();
                let filename = audio_path.file_stem().unwrap_or_default().to_string_lossy();
                p.set_file_name(format!("{}.txt", filename));
                p
            },
        ];

        for txt_path in &possible_txt_paths {
            if txt_path.exists() {
                let transcript = std::fs::read_to_string(txt_path)
                    .map_err(|e| WhisperError::TranscriptionError(format!("Failed to read transcript file: {}", e)))?;
                // Clean up the txt file
                let _ = std::fs::remove_file(txt_path);
                let trimmed = transcript.trim().to_string();
                if !trimmed.is_empty() {
                    return Ok(trimmed);
                }
            }
        }

        // Fallback: parse stdout
        let stdout = String::from_utf8_lossy(&output.stdout);
        let trimmed = stdout.trim().to_string();

        if trimmed.is_empty() {
            return Err(WhisperError::TranscriptionError(
                "Transcription produced empty output. The audio may be too short or contain no speech.".to_string()
            ));
        }

        Ok(trimmed)
    }
}

fn find_whisper_cli() -> Result<PathBuf, WhisperError> {
    let mut candidates: Vec<PathBuf> = vec![];

    // First priority: Check bundled binary next to the app executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            #[cfg(target_os = "windows")]
            {
                // Direct locations
                candidates.push(exe_dir.join("whisper-cli.exe"));
                candidates.push(exe_dir.join("whisper-cli-x86_64-pc-windows-msvc.exe"));
                // Resources folder (Tauri bundles resources here)
                candidates.push(exe_dir.join("resources").join("whisper-cli.exe"));
                candidates.push(exe_dir.join("resources").join("whisper-cli-x86_64-pc-windows-msvc.exe"));
                // Binaries subfolder in resources
                candidates.push(exe_dir.join("resources").join("binaries").join("whisper-cli.exe"));
                candidates.push(exe_dir.join("resources").join("binaries").join("whisper-cli-x86_64-pc-windows-msvc.exe"));
                // _up_ directory (for dev builds)
                candidates.push(exe_dir.join("..").join("whisper-cli.exe"));
            }
            #[cfg(target_os = "macos")]
            {
                candidates.push(exe_dir.join("whisper-cli"));
                candidates.push(exe_dir.join("whisper-cli-aarch64-apple-darwin"));
                candidates.push(exe_dir.join("whisper-cli-x86_64-apple-darwin"));
                // Resources folder
                candidates.push(exe_dir.join("../Resources").join("whisper-cli"));
                candidates.push(exe_dir.join("../Resources").join("whisper-cli-aarch64-apple-darwin"));
                candidates.push(exe_dir.join("../Resources").join("binaries").join("whisper-cli-aarch64-apple-darwin"));
            }
            #[cfg(target_os = "linux")]
            {
                candidates.push(exe_dir.join("whisper-cli"));
                candidates.push(exe_dir.join("whisper-cli-x86_64-unknown-linux-gnu"));
            }
        }
    }

    // macOS Homebrew locations
    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from("/usr/local/bin/whisper-cli"));
        candidates.push(PathBuf::from("/opt/homebrew/bin/whisper-cli"));
        candidates.push(PathBuf::from("/usr/local/bin/whisper-cpp"));
        candidates.push(PathBuf::from("/opt/homebrew/bin/whisper-cpp"));
    }

    // Windows locations
    #[cfg(target_os = "windows")]
    {
        candidates.push(PathBuf::from("C:\\Program Files\\whisper-cpp\\whisper-cli.exe"));
        candidates.push(PathBuf::from("C:\\Program Files (x86)\\whisper-cpp\\whisper-cli.exe"));

        // Check in LOCALAPPDATA
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            candidates.push(PathBuf::from(format!("{}\\whisper-cpp\\whisper-cli.exe", local_app_data)));
        }
    }

    for p in &candidates {
        if p.exists() {
            return Ok(p.clone());
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

pub fn get_whisper_status() -> String {
    match find_whisper_cli() {
        Ok(path) => format!("Found at: {}", path.display()),
        Err(e) => format!("Not found: {}", e),
    }
}

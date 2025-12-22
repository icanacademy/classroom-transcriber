mod audio;
mod db;
mod sync;
mod whisper;

use audio::AudioRecorder;
use db::{Database, Recording};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use sync::SyncClient;
use tauri::{Emitter, State};
use whisper::Transcriber;

struct AppState {
    db: Mutex<Database>,
    recorder: Mutex<AudioRecorder>,
    transcriber: Mutex<Option<Transcriber>>,
    data_dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct RecordingResult {
    id: String,
    duration: f64,
}

#[derive(Serialize)]
struct AppSettings {
    student_id: String,
    student_name: String,
    teacher_name: String,
    server_url: String,
    model_loaded: bool,
    setup_complete: bool,
}

#[derive(Serialize)]
struct SyncResult {
    synced_count: usize,
    failed_count: usize,
    errors: Vec<String>,
}

#[derive(Serialize)]
struct TranscribeResult {
    transcript: String,
    recording_id: String,
}

#[derive(Serialize, Clone)]
struct ProcessingStatus {
    stage: String,  // "saving", "transcribing", "syncing", "done", "error"
    message: String,
    recording_id: Option<String>,
    transcript: Option<String>,
    synced: bool,
}

// ========== Settings Commands ==========

#[tauri::command]
fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let student_id = db
        .get_setting("student_id")
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let student_name = db
        .get_setting("student_name")
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let teacher_name = db
        .get_setting("teacher_name")
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let server_url = db
        .get_setting("server_url")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "http://localhost:3000".to_string());
    let setup_complete = db
        .get_setting("setup_complete")
        .map_err(|e| e.to_string())?
        .map(|v| v == "true")
        .unwrap_or(false);
    let model_loaded = state.transcriber.lock().unwrap().is_some();

    Ok(AppSettings {
        student_id,
        student_name,
        teacher_name,
        server_url,
        model_loaded,
        setup_complete,
    })
}

#[tauri::command]
fn save_settings(
    state: State<AppState>,
    student_id: String,
    student_name: String,
    teacher_name: String,
    server_url: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_setting("student_id", &student_id)
        .map_err(|e| e.to_string())?;
    db.set_setting("student_name", &student_name)
        .map_err(|e| e.to_string())?;
    db.set_setting("teacher_name", &teacher_name)
        .map_err(|e| e.to_string())?;
    db.set_setting("server_url", &server_url)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn complete_setup(
    state: State<AppState>,
    student_name: String,
    teacher_name: String,
    server_url: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Generate student ID from name (lowercase, no spaces)
    let student_id = student_name
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join("-");

    db.set_setting("student_id", &student_id)
        .map_err(|e| e.to_string())?;
    db.set_setting("student_name", &student_name)
        .map_err(|e| e.to_string())?;
    db.set_setting("teacher_name", &teacher_name)
        .map_err(|e| e.to_string())?;
    db.set_setting("server_url", &server_url)
        .map_err(|e| e.to_string())?;
    db.set_setting("setup_complete", "true")
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ========== Recording Commands ==========

#[tauri::command]
fn start_recording(state: State<AppState>) -> Result<(), String> {
    let mut recorder = state.recorder.lock().map_err(|e| e.to_string())?;
    recorder.start_recording().map_err(|e| e.to_string())
}

#[tauri::command]
fn stop_recording(state: State<AppState>) -> Result<RecordingResult, String> {
    let mut recorder = state.recorder.lock().map_err(|e| e.to_string())?;
    let samples = recorder.stop_recording();

    // Generate unique ID
    let id = uuid::Uuid::new_v4().to_string();

    // Save audio file
    let audio_dir = state.data_dir.join("audio");
    std::fs::create_dir_all(&audio_dir).map_err(|e| e.to_string())?;
    let audio_path = audio_dir.join(format!("{}.wav", id));

    let duration = recorder
        .save_wav(&samples, &audio_path)
        .map_err(|e| e.to_string())?;

    // Get student ID
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let student_id = db
        .get_setting("student_id")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "unknown".to_string());

    // Create recording entry
    let recording = Recording {
        id: id.clone(),
        student_id,
        audio_path: audio_path.to_string_lossy().to_string(),
        transcript: None,
        duration_seconds: duration,
        recorded_at: chrono::Utc::now().to_rfc3339(),
        synced: false,
    };

    db.save_recording(&recording).map_err(|e| e.to_string())?;

    Ok(RecordingResult { id, duration })
}

#[tauri::command]
fn is_recording(state: State<AppState>) -> bool {
    state
        .recorder
        .lock()
        .map(|r| r.is_recording())
        .unwrap_or(false)
}

/// Stop recording, transcribe, and sync - all in one command
#[tauri::command]
fn stop_and_process(state: State<AppState>, window: tauri::Window) -> Result<ProcessingStatus, String> {
    // Stage 1: Stop recording and save audio
    let _ = window.emit("processing-status", ProcessingStatus {
        stage: "saving".to_string(),
        message: "Saving audio...".to_string(),
        recording_id: None,
        transcript: None,
        synced: false,
    });

    let mut recorder = state.recorder.lock().map_err(|e| e.to_string())?;
    let samples = recorder.stop_recording();

    let id = uuid::Uuid::new_v4().to_string();
    let audio_dir = state.data_dir.join("audio");
    std::fs::create_dir_all(&audio_dir).map_err(|e| e.to_string())?;
    let audio_path = audio_dir.join(format!("{}.wav", id));

    let duration = recorder
        .save_wav(&samples, &audio_path)
        .map_err(|e| e.to_string())?;
    drop(recorder);

    // Get student ID and save recording
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let student_id = db
        .get_setting("student_id")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "unknown".to_string());
    let server_url = db
        .get_setting("server_url")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    let recording = Recording {
        id: id.clone(),
        student_id,
        audio_path: audio_path.to_string_lossy().to_string(),
        transcript: None,
        duration_seconds: duration,
        recorded_at: chrono::Utc::now().to_rfc3339(),
        synced: false,
    };
    db.save_recording(&recording).map_err(|e| e.to_string())?;
    drop(db);

    // Stage 2: Transcribe
    let _ = window.emit("processing-status", ProcessingStatus {
        stage: "transcribing".to_string(),
        message: "Transcribing audio...".to_string(),
        recording_id: Some(id.clone()),
        transcript: None,
        synced: false,
    });

    let transcriber_guard = state.transcriber.lock().unwrap();
    let transcript = if let Some(transcriber) = transcriber_guard.as_ref() {
        match transcriber.transcribe(&audio_path) {
            Ok(t) => Some(t),
            Err(e) => {
                let _ = window.emit("processing-status", ProcessingStatus {
                    stage: "error".to_string(),
                    message: format!("Transcription failed: {}", e),
                    recording_id: Some(id.clone()),
                    transcript: None,
                    synced: false,
                });
                None
            }
        }
    } else {
        let _ = window.emit("processing-status", ProcessingStatus {
            stage: "error".to_string(),
            message: "Model not loaded. Please load the model in Settings.".to_string(),
            recording_id: Some(id.clone()),
            transcript: None,
            synced: false,
        });
        None
    };
    drop(transcriber_guard);

    // Update recording with transcript
    if let Some(ref t) = transcript {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let mut updated_recording = recording.clone();
        updated_recording.transcript = Some(t.clone());
        db.save_recording(&updated_recording).map_err(|e| e.to_string())?;
        drop(db);
    }

    // Stage 3: Sync to server
    let _ = window.emit("processing-status", ProcessingStatus {
        stage: "syncing".to_string(),
        message: "Syncing to server...".to_string(),
        recording_id: Some(id.clone()),
        transcript: transcript.clone(),
        synced: false,
    });

    let mut synced = false;
    if transcript.is_some() {
        let client = SyncClient::new(&server_url);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let recordings = db.get_all_recordings().map_err(|e| e.to_string())?;
        if let Some(rec) = recordings.iter().find(|r| r.id == id) {
            if client.submit_transcript(rec).is_ok() {
                db.mark_synced(&id).map_err(|e| e.to_string())?;
                synced = true;
            }
        }
    }

    // Stage 4: Done
    let final_status = ProcessingStatus {
        stage: "done".to_string(),
        message: if synced { "Done! Transcript synced to server.".to_string() }
                 else if transcript.is_some() { "Done! Transcript saved locally (sync pending).".to_string() }
                 else { "Recording saved. Transcription failed.".to_string() },
        recording_id: Some(id),
        transcript,
        synced,
    };

    let _ = window.emit("processing-status", final_status.clone());

    Ok(final_status)
}

// ========== Transcription Commands ==========

#[tauri::command]
fn load_model(state: State<AppState>) -> Result<(), String> {
    let model_path = state.data_dir.join("models").join("ggml-base.en.bin");

    if !model_path.exists() {
        return Err(format!(
            "Model not found. Please download ggml-base.en.bin to: {}",
            model_path.display()
        ));
    }

    let transcriber = Transcriber::new(&model_path).map_err(|e| e.to_string())?;
    *state.transcriber.lock().unwrap() = Some(transcriber);

    Ok(())
}

#[tauri::command]
fn transcribe_recording(state: State<AppState>, recording_id: String) -> Result<TranscribeResult, String> {
    // Get the recording
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let recordings = db.get_all_recordings().map_err(|e| e.to_string())?;
    let recording = recordings
        .iter()
        .find(|r| r.id == recording_id)
        .ok_or_else(|| "Recording not found".to_string())?
        .clone();
    drop(db); // Release lock before transcription

    // Get audio file path
    let audio_path = PathBuf::from(&recording.audio_path);

    // Transcribe using CLI
    let transcriber_guard = state.transcriber.lock().unwrap();
    let transcriber = transcriber_guard
        .as_ref()
        .ok_or_else(|| "Model not loaded. Please load the model first.".to_string())?;

    let transcript = transcriber.transcribe(&audio_path).map_err(|e| e.to_string())?;
    drop(transcriber_guard); // Release lock

    // Update recording with transcript
    let mut updated_recording = recording.clone();
    updated_recording.transcript = Some(transcript.clone());
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.save_recording(&updated_recording)
        .map_err(|e| e.to_string())?;

    Ok(TranscribeResult {
        transcript,
        recording_id,
    })
}

#[tauri::command]
fn get_model_path(state: State<AppState>) -> String {
    state
        .data_dir
        .join("models")
        .join("ggml-base.en.bin")
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
fn download_model(state: State<AppState>, window: tauri::Window) -> Result<(), String> {
    let model_path = state.data_dir.join("models").join("ggml-base.en.bin");

    if model_path.exists() {
        return Ok(()); // Already downloaded
    }

    let _ = window.emit("model-download-progress", "Starting download...");

    let url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

    // Download the model
    let response = reqwest::blocking::get(url)
        .map_err(|e| format!("Failed to download: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let _ = window.emit("model-download-progress", "Downloading... (142 MB)");

    let bytes = response.bytes()
        .map_err(|e| format!("Failed to read response: {}", e))?;

    let _ = window.emit("model-download-progress", "Saving model...");

    std::fs::write(&model_path, &bytes)
        .map_err(|e| format!("Failed to save model: {}", e))?;

    let _ = window.emit("model-download-progress", "Done!");

    // Auto-load the model after download
    let transcriber = Transcriber::new(&model_path).map_err(|e| e.to_string())?;
    *state.transcriber.lock().unwrap() = Some(transcriber);

    Ok(())
}

#[tauri::command]
fn check_model_exists(state: State<AppState>) -> bool {
    let model_path = state.data_dir.join("models").join("ggml-base.en.bin");
    model_path.exists()
}

// ========== Recording List Commands ==========

#[tauri::command]
fn get_recordings(state: State<AppState>) -> Result<Vec<Recording>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.get_all_recordings().map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_recording(state: State<AppState>, recording_id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    // Get the recording to delete the audio file
    let recordings = db.get_all_recordings().map_err(|e| e.to_string())?;
    if let Some(recording) = recordings.iter().find(|r| r.id == recording_id) {
        let _ = std::fs::remove_file(&recording.audio_path);
    }

    db.delete_recording(&recording_id)
        .map_err(|e| e.to_string())
}

// ========== Sync Commands ==========

#[tauri::command]
fn check_server_connection(state: State<AppState>) -> Result<bool, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let server_url = db
        .get_setting("server_url")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "http://localhost:3000".to_string());
    drop(db);

    let client = SyncClient::new(&server_url);
    Ok(client.check_connection())
}

#[tauri::command]
fn sync_transcripts(state: State<AppState>) -> Result<SyncResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let server_url = db
        .get_setting("server_url")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    let unsynced = db
        .get_unsynced_recordings()
        .map_err(|e| e.to_string())?;
    drop(db);

    let client = SyncClient::new(&server_url);

    let mut synced_count = 0;
    let mut failed_count = 0;
    let mut errors = Vec::new();

    for recording in &unsynced {
        match client.submit_transcript(recording) {
            Ok(_) => {
                let db = state.db.lock().map_err(|e| e.to_string())?;
                db.mark_synced(&recording.id)
                    .map_err(|e| e.to_string())?;
                synced_count += 1;
            }
            Err(e) => {
                failed_count += 1;
                errors.push(format!("Recording {}: {}", recording.id, e));
            }
        }
    }

    Ok(SyncResult {
        synced_count,
        failed_count,
        errors,
    })
}

#[tauri::command]
fn get_unsynced_count(state: State<AppState>) -> Result<usize, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let unsynced = db
        .get_unsynced_recordings()
        .map_err(|e| e.to_string())?;
    Ok(unsynced.len())
}

// ========== App Entry Point ==========

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Set up data directory
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("classroom-transcriber");

    std::fs::create_dir_all(&data_dir).expect("Failed to create data directory");
    std::fs::create_dir_all(data_dir.join("models")).expect("Failed to create models directory");
    std::fs::create_dir_all(data_dir.join("audio")).expect("Failed to create audio directory");

    // Initialize database
    let db = Database::new(&data_dir).expect("Failed to initialize database");

    // Initialize audio recorder
    let recorder = AudioRecorder::new().expect("Failed to initialize audio recorder");

    // Auto-load model if it exists
    let model_path = data_dir.join("models").join("ggml-base.en.bin");
    let transcriber = if model_path.exists() {
        match Transcriber::new(&model_path) {
            Ok(t) => {
                println!("Model auto-loaded from: {}", model_path.display());
                Some(t)
            }
            Err(e) => {
                eprintln!("Failed to auto-load model: {}", e);
                None
            }
        }
    } else {
        println!("Model not found at: {}", model_path.display());
        None
    };

    let app_state = AppState {
        db: Mutex::new(db),
        recorder: Mutex::new(recorder),
        transcriber: Mutex::new(transcriber),
        data_dir,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            // Settings
            get_settings,
            save_settings,
            complete_setup,
            // Recording
            start_recording,
            stop_recording,
            stop_and_process,
            is_recording,
            // Transcription
            load_model,
            download_model,
            check_model_exists,
            transcribe_recording,
            get_model_path,
            // Recordings list
            get_recordings,
            delete_recording,
            // Sync
            check_server_connection,
            sync_transcripts,
            get_unsynced_count,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use crate::db::Recording;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    #[error("Server returned error: {0}")]
    ServerError(String),
}

#[derive(Serialize)]
struct SubmitTranscript {
    student_id: String,
    device_type: String,
    audio_duration_seconds: f64,
    transcript: String,
    recorded_at: String,
    client_id: String,
}

#[derive(Deserialize)]
struct SubmitResponse {
    success: bool,
    id: Option<i64>,
    error: Option<String>,
}

pub struct SyncClient {
    client: Client,
    server_url: String,
}

impl SyncClient {
    pub fn new(server_url: &str) -> Self {
        Self {
            client: Client::new(),
            server_url: server_url.trim_end_matches('/').to_string(),
        }
    }

    pub fn check_connection(&self) -> bool {
        self.client
            .get(format!("{}/api/health", self.server_url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub fn submit_transcript(&self, recording: &Recording) -> Result<(), SyncError> {
        let payload = SubmitTranscript {
            student_id: recording.student_id.clone(),
            device_type: "desktop".to_string(),
            audio_duration_seconds: recording.duration_seconds,
            transcript: recording.transcript.clone().unwrap_or_default(),
            recorded_at: recording.recorded_at.clone(),
            client_id: recording.id.clone(),
        };

        let response: SubmitResponse = self
            .client
            .post(format!("{}/api/transcripts", self.server_url))
            .json(&payload)
            .send()?
            .json()?;

        if response.success {
            Ok(())
        } else {
            Err(SyncError::ServerError(
                response.error.unwrap_or_else(|| "Unknown error".to_string()),
            ))
        }
    }
}

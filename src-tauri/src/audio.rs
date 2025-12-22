use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat};
use hound::{WavSpec, WavWriter};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AudioError {
    #[error("No input device available")]
    NoInputDevice,
    #[error("Failed to get device config: {0}")]
    ConfigError(String),
    #[error("Stream error: {0}")]
    StreamError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Hound error: {0}")]
    HoundError(#[from] hound::Error),
    #[error("Recording error: {0}")]
    RecordingError(String),
}

pub struct AudioRecorder {
    samples: Arc<Mutex<Vec<f32>>>,
    is_recording: Arc<Mutex<bool>>,
    sample_rate: Arc<Mutex<u32>>,
    channels: Arc<Mutex<u16>>,
    recording_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl AudioRecorder {
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self {
            samples: Arc::new(Mutex::new(Vec::new())),
            is_recording: Arc::new(Mutex::new(false)),
            sample_rate: Arc::new(Mutex::new(16000)),
            channels: Arc::new(Mutex::new(1)),
            recording_thread: Arc::new(Mutex::new(None)),
        })
    }

    pub fn start_recording(&mut self) -> Result<(), AudioError> {
        // Clear previous samples
        self.samples.lock().unwrap().clear();
        *self.is_recording.lock().unwrap() = true;

        let samples = self.samples.clone();
        let is_recording = self.is_recording.clone();
        let sample_rate_out = self.sample_rate.clone();
        let channels_out = self.channels.clone();

        let handle = thread::spawn(move || {
            let host = cpal::default_host();
            let device = match host.default_input_device() {
                Some(d) => d,
                None => {
                    eprintln!("No input device available");
                    return;
                }
            };

            let config = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to get config: {}", e);
                    return;
                }
            };

            *sample_rate_out.lock().unwrap() = config.sample_rate().0;
            *channels_out.lock().unwrap() = config.channels();

            let err_fn = |err| eprintln!("Stream error: {}", err);

            let is_rec = is_recording.clone();
            let samples_clone = samples.clone();

            let stream = match config.sample_format() {
                SampleFormat::F32 => device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _| {
                        if *is_rec.lock().unwrap() {
                            samples_clone.lock().unwrap().extend_from_slice(data);
                        }
                    },
                    err_fn,
                    None,
                ),
                SampleFormat::I16 => {
                    let samples_clone = samples.clone();
                    let is_rec = is_recording.clone();
                    device.build_input_stream(
                        &config.into(),
                        move |data: &[i16], _| {
                            if *is_rec.lock().unwrap() {
                                let floats: Vec<f32> = data.iter().map(|&s| s.to_float_sample()).collect();
                                samples_clone.lock().unwrap().extend(floats);
                            }
                        },
                        err_fn,
                        None,
                    )
                },
                SampleFormat::U16 => {
                    let samples_clone = samples.clone();
                    let is_rec = is_recording.clone();
                    device.build_input_stream(
                        &config.into(),
                        move |data: &[u16], _| {
                            if *is_rec.lock().unwrap() {
                                let floats: Vec<f32> = data.iter().map(|&s| s.to_float_sample()).collect();
                                samples_clone.lock().unwrap().extend(floats);
                            }
                        },
                        err_fn,
                        None,
                    )
                },
                _ => {
                    eprintln!("Unsupported sample format");
                    return;
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to create stream: {}", e);
                    return;
                }
            };

            if let Err(e) = stream.play() {
                eprintln!("Failed to play stream: {}", e);
                return;
            }

            // Keep thread alive while recording
            while *is_recording.lock().unwrap() {
                thread::sleep(std::time::Duration::from_millis(100));
            }

            drop(stream);
        });

        *self.recording_thread.lock().unwrap() = Some(handle);

        // Give the thread time to start
        thread::sleep(std::time::Duration::from_millis(200));

        Ok(())
    }

    pub fn stop_recording(&mut self) -> Vec<f32> {
        *self.is_recording.lock().unwrap() = false;

        // Wait for thread to finish
        if let Some(handle) = self.recording_thread.lock().unwrap().take() {
            let _ = handle.join();
        }

        let samples = self.samples.lock().unwrap().clone();
        let sample_rate = *self.sample_rate.lock().unwrap();
        let channels = *self.channels.lock().unwrap();

        // Resample to 16kHz mono if needed
        if sample_rate != 16000 || channels != 1 {
            resample_to_16khz_mono(&samples, sample_rate, channels)
        } else {
            samples
        }
    }

    pub fn save_wav(&self, samples: &[f32], path: &PathBuf) -> Result<f64, AudioError> {
        let spec = WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = WavWriter::create(path, spec)?;

        for &sample in samples {
            let amplitude = (sample * i16::MAX as f32) as i16;
            writer.write_sample(amplitude)?;
        }

        writer.finalize()?;

        // Calculate duration
        let duration = samples.len() as f64 / 16000.0;
        Ok(duration)
    }

    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }
}

fn resample_to_16khz_mono(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<f32> {
    // First convert to mono by averaging channels
    let mono: Vec<f32> = if channels > 1 {
        samples
            .chunks(channels as usize)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        samples.to_vec()
    };

    // Simple linear interpolation resampling to 16kHz
    let ratio = sample_rate as f64 / 16000.0;
    let new_len = (mono.len() as f64 / ratio) as usize;
    let mut resampled = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = (src_idx - idx as f64) as f32;

        if idx + 1 < mono.len() {
            let interpolated = mono[idx] * (1.0 - frac) + mono[idx + 1] * frac;
            resampled.push(interpolated);
        } else if idx < mono.len() {
            resampled.push(mono[idx]);
        }
    }

    resampled
}

// Make AudioRecorder Send + Sync safe by not storing the stream
unsafe impl Send for AudioRecorder {}
unsafe impl Sync for AudioRecorder {}

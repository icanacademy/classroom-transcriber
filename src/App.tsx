import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

interface Recording {
  id: string;
  student_id: string;
  audio_path: string;
  transcript: string | null;
  duration_seconds: number;
  recorded_at: string;
  synced: boolean;
}

interface Settings {
  student_id: string;
  student_name: string;
  teacher_name: string;
  server_url: string;
  model_loaded: boolean;
  setup_complete: boolean;
}

interface ProcessingStatus {
  stage: string;
  message: string;
  recording_id: string | null;
  transcript: string | null;
  synced: boolean;
}

interface Student {
  id: string;
  name: string;
}

interface Teacher {
  id: string;
  name: string;
  nickname: string;
}

type Tab = "record" | "history" | "settings";

function App() {
  const [isLoading, setIsLoading] = useState(true);
  const [showSetup, setShowSetup] = useState(false);
  const [activeTab, setActiveTab] = useState<Tab>("record");
  const [isRecording, setIsRecording] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [processingStatus, setProcessingStatus] = useState<ProcessingStatus | null>(null);
  const [recordings, setRecordings] = useState<Recording[]>([]);
  const [settings, setSettings] = useState<Settings>({
    student_id: "",
    student_name: "",
    teacher_name: "",
    server_url: "http://localhost:3000",
    model_loaded: false,
    setup_complete: false,
  });

  // Setup form state
  const [setupStudentName, setSetupStudentName] = useState("");
  const [setupTeacherName, setSetupTeacherName] = useState("");
  const [setupServerUrl, setSetupServerUrl] = useState("http://localhost:3000");
  const [setupError, setSetupError] = useState("");
  const [studentsList, setStudentsList] = useState<Student[]>([]);
  const [teachersList, setTeachersList] = useState<Teacher[]>([]);
  const [loadingLists, setLoadingLists] = useState(false);

  // Settings form state
  const [studentId, setStudentId] = useState("");
  const [studentName, setStudentName] = useState("");
  const [teacherName, setTeacherName] = useState("");
  const [serverUrl, setServerUrl] = useState("http://localhost:3000");
  const [modelPath, setModelPath] = useState("");
  const [unsyncedCount, setUnsyncedCount] = useState(0);
  const [serverConnected, setServerConnected] = useState(false);
  const [recordingDuration, setRecordingDuration] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [lastTranscript, setLastTranscript] = useState<string | null>(null);

  const loadSettings = useCallback(async () => {
    try {
      const s = await invoke<Settings>("get_settings");
      setSettings(s);
      setStudentId(s.student_id);
      setStudentName(s.student_name);
      setTeacherName(s.teacher_name);
      setServerUrl(s.server_url);

      // Pre-fill setup form with saved values
      setSetupServerUrl(s.server_url || "http://localhost:3000");
      setSetupTeacherName(s.teacher_name || "");

      // Always show student selection on app start
      setShowSetup(true);
      setIsLoading(false);
    } catch (e) {
      console.error("Failed to load settings:", e);
      setIsLoading(false);
    }
  }, []);

  const loadRecordings = useCallback(async () => {
    try {
      const recs = await invoke<Recording[]>("get_recordings");
      setRecordings(recs);
    } catch (e) {
      console.error("Failed to load recordings:", e);
    }
  }, []);

  const loadUnsyncedCount = useCallback(async () => {
    try {
      const count = await invoke<number>("get_unsynced_count");
      setUnsyncedCount(count);
    } catch (e) {
      console.error("Failed to get unsynced count:", e);
    }
  }, []);

  const checkServerConnection = useCallback(async () => {
    try {
      const connected = await invoke<boolean>("check_server_connection");
      setServerConnected(connected);
    } catch {
      setServerConnected(false);
    }
  }, []);

  const getModelPath = useCallback(async () => {
    try {
      const path = await invoke<string>("get_model_path");
      setModelPath(path);
    } catch (e) {
      console.error("Failed to get model path:", e);
    }
  }, []);

  const fetchStudentsAndTeachers = useCallback(async (serverUrlToUse: string) => {
    setLoadingLists(true);
    try {
      const [studentsRes, teachersRes] = await Promise.all([
        fetch(`${serverUrlToUse}/api/students`),
        fetch(`${serverUrlToUse}/api/teachers`)
      ]);

      if (studentsRes.ok) {
        const students = await studentsRes.json();
        setStudentsList(students);
      }

      if (teachersRes.ok) {
        const teachers = await teachersRes.json();
        setTeachersList(teachers);
      }
    } catch (e) {
      console.error("Failed to fetch students/teachers:", e);
    } finally {
      setLoadingLists(false);
    }
  }, []);

  // Listen for processing status events
  useEffect(() => {
    const unlisten = listen<ProcessingStatus>("processing-status", (event) => {
      setProcessingStatus(event.payload);
      if (event.payload.stage === "done") {
        if (event.payload.transcript) {
          setLastTranscript(event.payload.transcript);
        }
        setTimeout(() => {
          setIsProcessing(false);
          setProcessingStatus(null);
          loadRecordings();
          loadUnsyncedCount();
        }, 2000);
      } else if (event.payload.stage === "error") {
        setTimeout(() => {
          setIsProcessing(false);
          loadRecordings();
          loadUnsyncedCount();
        }, 3000);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [loadRecordings, loadUnsyncedCount]);

  useEffect(() => {
    loadSettings();
    loadRecordings();
    loadUnsyncedCount();
    checkServerConnection();
    getModelPath();
  }, [loadSettings, loadRecordings, loadUnsyncedCount, checkServerConnection, getModelPath]);

  // Fetch students/teachers when setup wizard is shown
  useEffect(() => {
    if (showSetup) {
      fetchStudentsAndTeachers(setupServerUrl);
    }
  }, [showSetup, setupServerUrl, fetchStudentsAndTeachers]);

  useEffect(() => {
    let interval: number | null = null;
    if (isRecording) {
      interval = setInterval(() => {
        setRecordingDuration((d) => d + 1);
      }, 1000);
    } else {
      setRecordingDuration(0);
    }
    return () => {
      if (interval) clearInterval(interval);
    };
  }, [isRecording]);

  // Check server connection periodically
  useEffect(() => {
    const interval = setInterval(checkServerConnection, 30000);
    return () => clearInterval(interval);
  }, [checkServerConnection]);

  const showError = (msg: string) => {
    setError(msg);
    setTimeout(() => setError(null), 5000);
  };

  const showSuccess = (msg: string) => {
    setSuccess(msg);
    setTimeout(() => setSuccess(null), 3000);
  };

  const handleCompleteSetup = async () => {
    if (!setupStudentName.trim()) {
      setSetupError("Please select your name");
      return;
    }
    if (!setupTeacherName.trim()) {
      setSetupError("Please select your teacher");
      return;
    }

    try {
      await invoke("complete_setup", {
        studentName: setupStudentName.trim(),
        teacherName: setupTeacherName.trim(),
        serverUrl: setupServerUrl,
      });

      // Get updated settings without triggering setup screen again
      let s = await invoke<Settings>("get_settings");
      setSettings(s);
      setStudentId(s.student_id);
      setStudentName(s.student_name);
      setTeacherName(s.teacher_name);
      setServerUrl(s.server_url);

      setShowSetup(false);

      // Auto-download model if not exists
      if (!s.model_loaded) {
        const modelExists = await invoke<boolean>("check_model_exists");
        if (!modelExists) {
          showSuccess("Downloading AI model (142 MB)... Please wait.");
          try {
            await invoke("download_model");
            showSuccess("Model downloaded! Loading...");
            await invoke("load_model");
            s = await invoke<Settings>("get_settings");
            setSettings(s);
          } catch (e) {
            showError(`Model download failed: ${e}`);
            return;
          }
        } else {
          // Model exists but not loaded, try to load it
          try {
            await invoke("load_model");
            s = await invoke<Settings>("get_settings");
            setSettings(s);
          } catch (e) {
            showError(`Failed to load model: ${e}`);
            return;
          }
        }
      }

      // Auto-start recording if model is loaded
      if (s.model_loaded) {
        try {
          await invoke("start_recording");
          setIsRecording(true);
          setLastTranscript(null);
          setError(null);
        } catch (e) {
          showError(`Failed to start recording: ${e}`);
        }
      }
    } catch (e) {
      setSetupError(`Setup failed: ${e}`);
    }
  };

  const handleStartRecording = async () => {
    if (!settings.model_loaded) {
      showError("Please load the Whisper model in Settings first");
      setActiveTab("settings");
      return;
    }

    try {
      await invoke("start_recording");
      setIsRecording(true);
      setLastTranscript(null);
      setError(null);
    } catch (e) {
      showError(`Failed to start recording: ${e}`);
    }
  };

  const handleStopRecording = async () => {
    setIsRecording(false);
    setIsProcessing(true);
    setProcessingStatus({
      stage: "saving",
      message: "Saving audio...",
      recording_id: null,
      transcript: null,
      synced: false,
    });

    try {
      await invoke("stop_and_process");
    } catch (e) {
      showError(`Failed to process recording: ${e}`);
      setIsProcessing(false);
      setProcessingStatus(null);
    }
  };

  const handleSaveSettings = async () => {
    try {
      await invoke("save_settings", {
        studentId,
        studentName,
        teacherName,
        serverUrl
      });
      loadSettings();
      showSuccess("Settings saved!");
    } catch (e) {
      showError(`Failed to save settings: ${e}`);
    }
  };

  const handleLoadModel = async () => {
    try {
      await invoke("load_model");
      loadSettings();
      showSuccess("Model loaded successfully!");
    } catch (e) {
      showError(`${e}`);
    }
  };

  const handleDelete = async (recordingId: string) => {
    if (!confirm("Delete this recording?")) return;
    try {
      await invoke("delete_recording", { recordingId });
      loadRecordings();
      loadUnsyncedCount();
    } catch (e) {
      showError(`Failed to delete: ${e}`);
    }
  };

  const handleManualSync = async () => {
    try {
      const result = await invoke<{ synced_count: number; failed_count: number }>("sync_transcripts");
      if (result.synced_count > 0) {
        showSuccess(`Synced ${result.synced_count} transcript(s)`);
      }
      loadRecordings();
      loadUnsyncedCount();
    } catch (e) {
      showError(`Sync failed: ${e}`);
    }
  };

  const formatDuration = (seconds: number) => {
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs.toString().padStart(2, "0")}`;
  };

  const formatDate = (isoString: string) => {
    return new Date(isoString).toLocaleString();
  };

  const getStatusIcon = (stage: string) => {
    switch (stage) {
      case "saving": return "💾";
      case "transcribing": return "🎯";
      case "syncing": return "☁️";
      case "done": return "✓";
      case "error": return "✗";
      default: return "...";
    }
  };

  // Loading screen
  if (isLoading) {
    return (
      <div className="app">
        <div className="loading-screen">
          <h1>Classroom Transcriber</h1>
          <p>Loading...</p>
        </div>
      </div>
    );
  }

  // Setup wizard / Student selection
  if (showSetup) {
    const isFirstTime = !settings.setup_complete;

    return (
      <div className="app">
        <div className="setup-wizard">
          <div className="setup-header">
            <h1>{isFirstTime ? "Welcome!" : "Who's Recording?"}</h1>
            <p>{isFirstTime ? "Let's set up your transcription app" : "Select your name to start"}</p>
          </div>

          <div className="setup-form">
            {setupError && <div className="setup-error">{setupError}</div>}

            {/* Only show server URL on first time setup or if no students loaded */}
            {(isFirstTime || studentsList.length === 0) && (
              <div className="setup-field">
                <label>Server URL</label>
                <input
                  type="text"
                  value={setupServerUrl}
                  onChange={(e) => setSetupServerUrl(e.target.value)}
                  placeholder="http://localhost:3000"
                />
              </div>
            )}

            {loadingLists && (
              <p style={{ textAlign: "center", color: "#666" }}>Loading names...</p>
            )}

            <div className="setup-field">
              <label>What's your name?</label>
              {studentsList.length > 0 ? (
                <select
                  value={setupStudentName}
                  onChange={(e) => setSetupStudentName(e.target.value)}
                  style={{ width: "100%", padding: "14px 16px", border: "2px solid #e5e5e5", borderRadius: "8px", fontSize: "1rem" }}
                  autoFocus
                >
                  <option value="">Select your name...</option>
                  {studentsList.map((student) => (
                    <option key={student.id} value={student.name}>
                      {student.name}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  type="text"
                  value={setupStudentName}
                  onChange={(e) => setSetupStudentName(e.target.value)}
                  placeholder="Enter your full name"
                  autoFocus
                />
              )}
            </div>

            <div className="setup-field">
              <label>Who's your teacher?</label>
              {teachersList.length > 0 ? (
                <select
                  value={setupTeacherName}
                  onChange={(e) => setSetupTeacherName(e.target.value)}
                  style={{ width: "100%", padding: "14px 16px", border: "2px solid #e5e5e5", borderRadius: "8px", fontSize: "1rem" }}
                >
                  <option value="">Select your teacher...</option>
                  {teachersList.map((teacher) => (
                    <option key={teacher.id} value={teacher.name}>
                      {teacher.nickname ? `${teacher.name} (${teacher.nickname})` : teacher.name}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  type="text"
                  value={setupTeacherName}
                  onChange={(e) => setSetupTeacherName(e.target.value)}
                  placeholder="Enter your teacher's name"
                />
              )}
            </div>

            <button className="setup-button" onClick={handleCompleteSetup} disabled={loadingLists}>
              {isFirstTime ? "Get Started" : "Start Recording"}
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      {/* Header */}
      <header className="header">
        <h1>Classroom Transcriber</h1>
        <div className="header-status">
          <span className={`status-dot ${serverConnected ? "connected" : "disconnected"}`}></span>
          <span>{serverConnected ? "Server Connected" : "Server Offline"}</span>
          {unsyncedCount > 0 && (
            <span className="badge" onClick={handleManualSync} style={{ cursor: "pointer" }}>
              {unsyncedCount} unsynced
            </span>
          )}
        </div>
      </header>

      {/* Alerts */}
      {error && <div className="alert alert-error">{error}</div>}
      {success && <div className="alert alert-success">{success}</div>}

      {/* Tabs */}
      <nav className="tabs">
        <button
          className={activeTab === "record" ? "active" : ""}
          onClick={() => setActiveTab("record")}
        >
          Record
        </button>
        <button
          className={activeTab === "history" ? "active" : ""}
          onClick={() => setActiveTab("history")}
        >
          History ({recordings.length})
        </button>
        <button
          className={activeTab === "settings" ? "active" : ""}
          onClick={() => setActiveTab("settings")}
        >
          Settings
        </button>
      </nav>

      {/* Content */}
      <main className="content">
        {/* Record Tab */}
        {activeTab === "record" && (
          <div className="record-tab">
            <div className="student-info">
              <div><strong>Student:</strong> {settings.student_name || settings.student_id}</div>
              <div><strong>Teacher:</strong> {settings.teacher_name}</div>
            </div>

            {/* Recording UI */}
            {!isProcessing ? (
              <div className="recorder">
                <div className={`record-button ${isRecording ? "recording" : ""}`}>
                  <button
                    onClick={isRecording ? handleStopRecording : handleStartRecording}
                    disabled={!settings.model_loaded}
                  >
                    {isRecording ? "Stop" : "Record"}
                  </button>
                </div>

                {isRecording && (
                  <div className="recording-indicator">
                    <span className="pulse"></span>
                    Recording: {formatDuration(recordingDuration)}
                  </div>
                )}

                {!settings.model_loaded && (
                  <p className="hint">Load the Whisper model in Settings to enable recording</p>
                )}
              </div>
            ) : (
              /* Processing Status UI */
              <div className="processing-container">
                <div className="processing-status">
                  <div className="processing-icon">{getStatusIcon(processingStatus?.stage || "")}</div>
                  <div className="processing-message">{processingStatus?.message}</div>
                  <div className="processing-stages">
                    <span className={processingStatus?.stage === "saving" ? "active" : processingStatus?.stage && ["transcribing", "syncing", "done"].includes(processingStatus.stage) ? "complete" : ""}>Save</span>
                    <span className="arrow">→</span>
                    <span className={processingStatus?.stage === "transcribing" ? "active" : processingStatus?.stage && ["syncing", "done"].includes(processingStatus.stage) ? "complete" : ""}>Transcribe</span>
                    <span className="arrow">→</span>
                    <span className={processingStatus?.stage === "syncing" ? "active" : processingStatus?.stage === "done" ? "complete" : ""}>Sync</span>
                  </div>
                </div>
              </div>
            )}

            {/* Show last transcript */}
            {lastTranscript && !isProcessing && !isRecording && (
              <div className="last-transcript">
                <h3>Last Transcript:</h3>
                <p>{lastTranscript}</p>
              </div>
            )}

            {/* Auto-workflow info */}
            <div className="workflow-info">
              <h4>How it works:</h4>
              <ol>
                <li>Press <strong>Record</strong> and speak</li>
                <li>Press <strong>Stop</strong> when finished</li>
                <li>Audio is automatically transcribed and synced to server</li>
              </ol>
            </div>
          </div>
        )}

        {/* History Tab */}
        {activeTab === "history" && (
          <div className="history-tab">
            <div className="history-header">
              <h2>Recording History</h2>
              {unsyncedCount > 0 && (
                <button className="sync-button" onClick={handleManualSync}>
                  Sync {unsyncedCount} Pending
                </button>
              )}
            </div>

            {recordings.length === 0 ? (
              <p className="empty-state">No recordings yet. Start recording to see them here.</p>
            ) : (
              <div className="recordings-list">
                {recordings.map((rec) => (
                  <div key={rec.id} className="recording-card">
                    <div className="recording-header">
                      <span className="recording-date">{formatDate(rec.recorded_at)}</span>
                      <span className="recording-duration">{formatDuration(rec.duration_seconds)}</span>
                      <span className={`sync-status ${rec.synced ? "synced" : "unsynced"}`}>
                        {rec.synced ? "Synced" : "Pending"}
                      </span>
                    </div>

                    {rec.transcript ? (
                      <div className="transcript">
                        <p>{rec.transcript}</p>
                      </div>
                    ) : (
                      <div className="no-transcript">
                        <p>Transcription pending...</p>
                      </div>
                    )}

                    <div className="recording-actions">
                      <button className="delete-btn" onClick={() => handleDelete(rec.id)}>
                        Delete
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Settings Tab */}
        {activeTab === "settings" && (
          <div className="settings-tab">
            <h2>Settings</h2>

            <div className="setting-group">
              <label>Your Name</label>
              <input
                type="text"
                value={studentName}
                onChange={(e) => setStudentName(e.target.value)}
                placeholder="Enter your name"
              />
            </div>

            <div className="setting-group">
              <label>Teacher's Name</label>
              <input
                type="text"
                value={teacherName}
                onChange={(e) => setTeacherName(e.target.value)}
                placeholder="Enter teacher's name"
              />
            </div>

            <div className="setting-group">
              <label>Server URL</label>
              <input
                type="text"
                value={serverUrl}
                onChange={(e) => setServerUrl(e.target.value)}
                placeholder="http://localhost:3000"
              />
              <button className="small-btn" onClick={checkServerConnection}>
                Test Connection
              </button>
            </div>

            <button className="save-btn" onClick={handleSaveSettings}>
              Save Settings
            </button>

            <hr />

            <div className="model-section">
              <h3>Whisper Model</h3>
              <p className="model-status">
                Status: {settings.model_loaded ? (
                  <span className="loaded">Loaded</span>
                ) : (
                  <span className="not-loaded">Not loaded</span>
                )}
              </p>

              {!settings.model_loaded && (
                <>
                  <p className="model-instructions">
                    Download the Whisper model file and place it at:
                  </p>
                  <code className="model-path">{modelPath}</code>
                  <p className="model-download">
                    Download from: <a href="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin" target="_blank" rel="noreferrer">
                      ggml-base.en.bin (142 MB)
                    </a>
                  </p>
                  <button onClick={handleLoadModel}>Load Model</button>
                </>
              )}

              {settings.model_loaded && (
                <p className="model-ready">Model is ready for transcription!</p>
              )}
            </div>
          </div>
        )}
      </main>
    </div>
  );
}

export default App;

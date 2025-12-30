#!/usr/bin/env python3
"""
Transcription with Speaker Diarization using faster-whisper + pyannote
Runs transcription and diarization in separate processes to avoid library conflicts
"""

import sys
import json
import argparse
import os
import subprocess
import tempfile

# Suppress warnings
import warnings
warnings.filterwarnings("ignore")
os.environ["KMP_DUPLICATE_LIB_OK"] = "TRUE"


def run_transcription(audio_path, model_size, language):
    """Run transcription in a subprocess"""
    script = f'''
import os
import sys
os.environ["KMP_DUPLICATE_LIB_OK"] = "TRUE"
os.environ["MKL_THREADING_LAYER"] = "GNU"
import json
import warnings
warnings.filterwarnings("ignore")

# Redirect all stderr to suppress warnings
import io
old_stderr = sys.stderr
sys.stderr = io.StringIO()

from faster_whisper import WhisperModel

model = WhisperModel("{model_size}", device="cpu", compute_type="int8")
segments, info = model.transcribe("{audio_path}", language="{language}", beam_size=5)

result = []
for seg in segments:
    result.append({{"start": seg.start, "end": seg.end, "text": seg.text.strip()}})

sys.stderr = old_stderr
print(json.dumps({{"segments": result, "duration": info.duration}}))
'''

    result = subprocess.run(
        ["./whisperx-env/bin/python", "-c", script],
        capture_output=True,
        text=True,
        cwd=os.path.dirname(os.path.abspath(__file__))
    )

    if result.returncode != 0:
        raise Exception(f"Transcription failed: {result.stderr}")

    # Find the JSON line in output (skip warning lines)
    for line in result.stdout.strip().split('\n'):
        if line.startswith('{'):
            return json.loads(line)

    raise Exception(f"No JSON output found: {result.stdout}")


def run_diarization(audio_path, hf_token):
    """Run diarization in a subprocess"""
    script = f'''
import os
os.environ["KMP_DUPLICATE_LIB_OK"] = "TRUE"
import json
import warnings
warnings.filterwarnings("ignore")
from pyannote.audio import Pipeline

pipeline = Pipeline.from_pretrained("pyannote/speaker-diarization-3.1", use_auth_token="{hf_token}")
diarization = pipeline("{audio_path}")

result = []
for turn, _, speaker in diarization.itertracks(yield_label=True):
    result.append({{"start": turn.start, "end": turn.end, "speaker": speaker}})

print(json.dumps(result))
'''

    result = subprocess.run(
        ["./whisperx-env/bin/python", "-c", script],
        capture_output=True,
        text=True,
        cwd=os.path.dirname(os.path.abspath(__file__))
    )

    if result.returncode != 0:
        raise Exception(f"Diarization failed: {result.stderr}")

    # Find the JSON line in output (skip warning lines)
    for line in result.stdout.strip().split('\n'):
        if line.startswith('['):
            return json.loads(line)

    raise Exception(f"No JSON output found: {result.stdout}")


def assign_speakers(whisper_segments, speaker_segments):
    """Assign speakers to whisper segments based on timing"""
    for w_seg in whisper_segments:
        w_mid = (w_seg["start"] + w_seg["end"]) / 2
        best_speaker = "UNKNOWN"
        best_overlap = 0

        for s_seg in speaker_segments:
            # Check overlap
            overlap_start = max(w_seg["start"], s_seg["start"])
            overlap_end = min(w_seg["end"], s_seg["end"])
            overlap = max(0, overlap_end - overlap_start)

            if overlap > best_overlap:
                best_overlap = overlap
                best_speaker = s_seg["speaker"]

        w_seg["speaker"] = best_speaker
    return whisper_segments


def group_by_speaker(segments):
    """Group consecutive segments by speaker"""
    grouped = []
    current = None
    for seg in segments:
        if current is None:
            current = seg.copy()
        elif seg["speaker"] == current["speaker"]:
            current["end"] = seg["end"]
            current["text"] += " " + seg["text"]
        else:
            grouped.append(current)
            current = seg.copy()
    if current:
        grouped.append(current)
    return grouped


def calculate_stats(segments):
    """Calculate stats per speaker"""
    speakers = {}
    for seg in segments:
        spk = seg["speaker"]
        if spk not in speakers:
            speakers[spk] = {"word_count": 0, "duration": 0}
        speakers[spk]["word_count"] += len(seg["text"].split())
        speakers[spk]["duration"] += seg["end"] - seg["start"]

    # Round values
    for seg in segments:
        seg["start"] = round(seg["start"], 2)
        seg["end"] = round(seg["end"], 2)

    for spk in speakers:
        speakers[spk]["duration"] = round(speakers[spk]["duration"], 2)

    return speakers


def main():
    parser = argparse.ArgumentParser(description="Transcribe audio with speaker diarization")
    parser.add_argument("audio_path", help="Path to audio file")
    parser.add_argument("--model", default="base", help="Whisper model size (tiny, base, small, medium, large-v2)")
    parser.add_argument("--language", default="en", help="Language code")
    parser.add_argument("--hf-token", help="HuggingFace token for diarization")
    parser.add_argument("--no-diarize", action="store_true", help="Skip diarization")

    args = parser.parse_args()

    # Check for HF token in environment if not provided
    hf_token = args.hf_token or os.environ.get("HF_TOKEN")

    try:
        # Step 1: Transcribe
        print("Transcribing...", file=sys.stderr)
        transcription = run_transcription(args.audio_path, args.model, args.language)
        whisper_segments = transcription["segments"]

        # Step 2: Diarize (if token available and not skipped)
        if not args.no_diarize and hf_token:
            print("Running speaker diarization...", file=sys.stderr)
            speaker_segments = run_diarization(args.audio_path, hf_token)
            whisper_segments = assign_speakers(whisper_segments, speaker_segments)
        else:
            if not args.no_diarize and not hf_token:
                print("Warning: No HuggingFace token. Running without diarization.", file=sys.stderr)
            for seg in whisper_segments:
                seg["speaker"] = "SPEAKER_00"

        # Step 3: Group and calculate stats
        grouped = group_by_speaker(whisper_segments)
        speakers = calculate_stats(grouped)

        result = {
            "segments": grouped,
            "speakers": speakers,
            "full_transcript": " ".join([s["text"] for s in grouped])
        }

        # Output compact JSON (single line) for Rust parser
        print(json.dumps(result, ensure_ascii=False))

    except Exception as e:
        import traceback
        print(json.dumps({"error": str(e), "traceback": traceback.format_exc()}), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()

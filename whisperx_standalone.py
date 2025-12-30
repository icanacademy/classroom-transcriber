#!/usr/bin/env python3
"""
Standalone Transcription with Speaker Diarization
Uses faster-whisper + pyannote - designed for PyInstaller bundling
"""

import sys
import json
import argparse
import os
import warnings

# Suppress warnings
warnings.filterwarnings("ignore")
os.environ["KMP_DUPLICATE_LIB_OK"] = "TRUE"
os.environ["TOKENIZERS_PARALLELISM"] = "false"


def transcribe_audio(audio_path, model_size="tiny", language="en"):
    """Transcribe audio using faster-whisper"""
    from faster_whisper import WhisperModel

    print("Loading Whisper model...", file=sys.stderr)
    model = WhisperModel(model_size, device="cpu", compute_type="int8")

    print("Transcribing...", file=sys.stderr)
    segments, info = model.transcribe(audio_path, language=language, beam_size=5)

    result = []
    for seg in segments:
        result.append({
            "start": seg.start,
            "end": seg.end,
            "text": seg.text.strip()
        })

    return result, info.duration


def diarize_audio(audio_path, hf_token):
    """Run speaker diarization using pyannote"""
    from pyannote.audio import Pipeline

    print("Running speaker diarization...", file=sys.stderr)
    pipeline = Pipeline.from_pretrained(
        "pyannote/speaker-diarization-3.1",
        use_auth_token=hf_token
    )
    diarization = pipeline(audio_path)

    result = []
    for turn, _, speaker in diarization.itertracks(yield_label=True):
        result.append({
            "start": turn.start,
            "end": turn.end,
            "speaker": speaker
        })

    return result


def assign_speakers(whisper_segments, speaker_segments):
    """Assign speakers to whisper segments based on timing overlap"""
    for w_seg in whisper_segments:
        best_speaker = "UNKNOWN"
        best_overlap = 0

        for s_seg in speaker_segments:
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

    for seg in segments:
        seg["start"] = round(seg["start"], 2)
        seg["end"] = round(seg["end"], 2)

    for spk in speakers:
        speakers[spk]["duration"] = round(speakers[spk]["duration"], 2)

    return speakers


def main():
    parser = argparse.ArgumentParser(description="Transcribe audio with speaker diarization")
    parser.add_argument("audio_path", help="Path to audio file")
    parser.add_argument("--model", default="tiny", help="Whisper model size")
    parser.add_argument("--language", default="en", help="Language code")
    parser.add_argument("--hf-token", help="HuggingFace token for diarization")
    parser.add_argument("--no-diarize", action="store_true", help="Skip diarization")

    args = parser.parse_args()
    hf_token = args.hf_token or os.environ.get("HF_TOKEN")

    try:
        whisper_segments, duration = transcribe_audio(args.audio_path, args.model, args.language)

        if not args.no_diarize and hf_token:
            speaker_segments = diarize_audio(args.audio_path, hf_token)
            whisper_segments = assign_speakers(whisper_segments, speaker_segments)
        else:
            for seg in whisper_segments:
                seg["speaker"] = "SPEAKER_00"

        grouped = group_by_speaker(whisper_segments)
        speakers = calculate_stats(grouped)

        result = {
            "segments": grouped,
            "speakers": speakers,
            "full_transcript": " ".join([s["text"] for s in grouped])
        }

        print(json.dumps(result, ensure_ascii=False))

    except Exception as e:
        import traceback
        print(json.dumps({"error": str(e), "traceback": traceback.format_exc()}), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()

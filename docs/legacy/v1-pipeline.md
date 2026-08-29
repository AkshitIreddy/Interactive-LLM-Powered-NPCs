# Exact version 1 interaction pipeline

This document records the observed root `main.ipynb` path. It is descriptive evidence, not an instruction to run the prototype.

## End-to-end sequence

```text
OpenCV notebook loop
  │
  ├─ GDI BitBlt desktop region (fixed 1920×1080)
  ├─ draw status text into copied frame
  ├─ wait for focused-window `t` key
  ├─ show “Speak” for 2 seconds
  ├─ blocking SpeechRecognition microphone listen
  ├─ complete Google recognizer request
  ├─ show transcript for 3 seconds
  └─ functions.main(...)
       │
       ├─ write temp/screen.jpg
       ├─ DeepFace/RetinaFace detect and crop → temp/extracted_face.jpg
       ├─ open webcam 0, take one frame → temp/webcam_photo.jpg
       ├─ DeepFace emotion analysis (fallback neutral)
       └─ branch
          ├─ animation off or no detected face
          │    ├─ literal/name-prefix match against known characters
          │    ├─ known → side-character audio generator
          │    └─ unknown → default/background audio generator
          └─ face detected
               ├─ DeepFace Facenet512 search in every character image folder
               ├─ known → side-character video generator
               └─ unknown → default/background video generator
                    │
                    ├─ background only: compare with one saved default face;
                    │  on mismatch infer age/gender/race, overwrite identity,
                    │  ask Cohere to generate a bio, select Edge voice
                    ├─ shuffle up to 500 tokens of example dialogue
                    ├─ append player line or summarize old conversation
                    ├─ Cohere embedding search of public Chroma store
                    ├─ known only: Cohere embedding search of character store
                    ├─ read world, biography, and recent dialogue files
                    ├─ assemble one large Cohere `command` prompt
                    ├─ wait for the complete response
                    ├─ append NPC response to conversation.json
                    ├─ interpolate response into root temp.py
                    ├─ run temp.py to call character voice module
                    └─ visual only: wait for complete SadTalker MP4
                         │
                         └─ notebook playback
                              ├─ audio-only: load entire audio and play
                              └─ visual: freeze saved screen, resize each
                                 generated frame into rectangular face ROI,
                                 play audio on a thread, join at end
```

## Notebook state machine

The loop is implemented as booleans rather than a typed state machine:

| Flag | Transition |
| --- | --- |
| `speak_display` | Set when `cv2.waitKey(1)` sees `interact_key`; cleared after microphone/STT. |
| `speech_recognition_start` | Becomes true two seconds after the “Speak” prompt; immediately enters blocking microphone capture. |
| `transcribed_text_display` | Shows successful transcript for three seconds, then enables generation. |
| `speech_to_text_error_display` | Shows a wrapped error for three seconds; does not create structured retry state. |
| `main_function_start` | Runs the full synchronous AI and animation path and remains true during nested playback until reset. |

There are three separate calls to `cv2.waitKey(1)` per outer iteration. Key input is scoped to the OpenCV window; it is not a system-wide hotkey.

## Context and memory construction

For a known character, the prompt combines:

1. complete `world.txt`;
2. complete `bio.txt`;
3. a random shuffle of `pre_conversation.json` lines up to 500 Cohere-counted tokens;
4. one public Chroma similarity result based on up to five recent lines;
5. one character Chroma similarity result based on the same recent lines;
6. mutable `conversation.json` with the current player message;
7. the webcam-derived player emotion;
8. an `About … / Talking Style / Additional Information` prompt ending in `Character:`.

When a non-default conversation exceeds 500 tokens, a separate Cohere request summarizes the entire dialogue, adds that summary to the character Chroma store, and clears the JSON conversation to the current player line. For the default NPC, the old conversation is simply discarded. There is no schema/version, provenance, transaction, delivered-message flag, save/quest partition, or embedding model namespace.

## Background NPC lifecycle

Audio-only mode treats the same default NPC as current for ten minutes using `timestamp.txt`. After ten minutes it randomly selects a binary gender, generates a name, chooses an Edge voice, asks Cohere for a Cyberpunk biography, and overwrites the default files.

Video mode compares the detected crop to a single saved `default/face.jpg`. When the comparison fails, it overwrites the face, infers age/gender/race, generates a new name/personality, and overwrites the same default state. Multiple background NPCs cannot coexist, a return after replacement cannot recover the earlier identity, and a false negative destroys continuity.

## Files and services touched during one turn

| Stage | Read/write or external dependency |
| --- | --- |
| Capture | Win32 GDI desktop/window DC, OpenCV window, `temp/screen.jpg`, `temp/extracted_face.jpg` |
| Player perception | webcam device 0, `temp/webcam_photo.jpg`, DeepFace/RetinaFace |
| Identity | DeepFace/Facenet512, character JPEGs, `representations_facenet512.pkl`, default face state |
| Retrieval | `apikeys.json`, Cohere embeddings, Chroma parquet/bin/pickle stores |
| Generation | `world.txt`, bios, style examples, conversations, Cohere `command` through LangChain |
| TTS | root `temp.py`, `.venv/Scripts/python.exe`, per-character Edge TTS modules, fixed audio path |
| Animation | `SadTalker/venv/scripts/python.exe`, full SadTalker model stack, `video_temp/`, fixed MP4 path |
| Playback | pydub/ffmpeg expectations, OpenCV nested loop, frozen screenshot and rectangular ROI |

## Failure semantics

Most stages return magic strings (`"NULL"` or `""`) or print exceptions. There are no timeouts, typed error codes, retry classifications, correlation IDs, cancellation generations, or partial-success contracts. A failure after conversation mutation leaves durable false history. A crash can leave temporary code, audio, image, video, or generated folders that a later turn may mistake for fresh output.

## Latency consequences

The first audible byte cannot occur until capture, face detection, webcam capture/emotion, identity, context loading, cloud retrieval, full LLM completion, conversation persistence, temporary program creation, subprocess startup, and full TTS completion have finished. On the visual path, the entire SadTalker render also completes first. The explicit two-second pre-listen delay and three-second transcript display add five seconds before generation begins even when every model is instantaneous.

Version 2 replaces this path with event-driven capture, authoritative PTT endpoints, streaming STT/LLM/TTS, clause-level audio scheduling, independent structured effects, asynchronous post-delivery memory writes, cancellation generations, and capability-based visual degradation.


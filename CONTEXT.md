# Context Glossary

## Raw ASR Segment

A short, timestamped final transcription fragment produced by the ASR pipeline. It is useful for debugging and evidence lookup, but it is not the primary user-facing meeting record. Interim ASR text is not a Raw ASR Segment.

## Streaming ASR Session

A provider-backed live speech-to-text connection for one Recording Run. It owns temporary audio flow and recognition events, but final transcript ownership still belongs to the Meeting.

## Interim ASR Result

Mutable live recognition text produced before the ASR provider confirms a final segment. It may be replaced by later ASR events and is only suitable for live caption display.

## Final ASR Result

Provider-confirmed transcript text that can be converted into a Raw ASR Segment and persisted in a transcript revision.

## Live Caption

The current on-screen text shown from Interim ASR Results while a Recording Run is active. It is not meeting memory and must not be archived as a Raw ASR Segment.

## Meeting Minutes

The user-facing structured meeting note. It is generated from a persisted transcript revision by a text model, normalized to Simplified Chinese, and saved with the transcript revision it belongs to.

## Meeting Record

A local record of one meeting, including its audio, transcript, Meeting Minutes, and lifecycle state. A Meeting Record may exist without any remote Meeting Room.

## Recording Run

A continuous audio-capture interval inside a Meeting Record. Pausing ends the current Recording Run while keeping the Meeting Record open for later continuation.

## Processing

Persistent background work after recording stops. It can re-transcribe the saved recording, save a new transcript revision, and generate minutes. Recording completion does not wait for cloud work; failures remain retryable on the owning Meeting Record.

## Meeting Room

A future remote space that participants join with a matching code. A Meeting Room coordinates participants but is not itself a local recording or transcript.

## Room Participant

A user or device that has joined a Meeting Room.

## Room Gateway

The product boundary through which a client may later create, join, leave, and observe a Meeting Room. The current product does not require a remote Room Gateway implementation.

## Audio Source

One independently controlled input to a Recording Run. Windows V1 supports a microphone source and a whole-system loopback source.

## Mixed Recording

The single user-facing audio asset produced after enabled Audio Sources are normalized and mixed. Internal recovery parts may exist while recording, but a completed Meeting Record exposes one playable recording.

## Transcription Job

A persistent background attempt to turn a live stream or saved Mixed Recording into transcript revisions. Its failure never invalidates the underlying recording.

## AI Gateway

The application-facing boundary for live transcription, file transcription, and meeting-minutes generation. Individual providers implement only the capabilities they actually support.

## Recycle Bin

The soft-deleted state and storage area for Meeting Records. Audio, transcript, and minutes remain recoverable until the user explicitly deletes them permanently or empties the Recycle Bin.

## Release Provider Policy

The release build separates live transcription, saved-file transcription, and minutes generation into independent Provider capabilities. Remote endpoints require HTTPS; loopback HTTP is allowed for local development. API keys are stored by Windows Credential Manager and are resolved only in the Rust backend. Local ASR models are not exposed or used as fallback in the release build.

# Context Glossary

## Raw ASR Segment

A short, timestamped final transcription fragment produced by the ASR pipeline. It is useful for debugging and evidence lookup, but it is not the primary user-facing meeting record. Interim ASR text is not a Raw ASR Segment.

## Streaming ASR Session

A provider-backed live speech-to-text connection for one Recording Run. It owns temporary audio flow and recognition events, but final transcript ownership still belongs to the Meeting.

## Interim ASR Result

Mutable live recognition text produced before the ASR provider confirms a final segment. It may be replaced by later ASR events and is only suitable for live caption display.

## Final ASR Result

Provider-confirmed transcript text that can be converted into a Raw ASR Segment and used as Meeting Digest input.

## Live Caption

The current on-screen text shown from Interim ASR Results while a Recording Run is active. It is not meeting memory and must not be archived as a Raw ASR Segment.

## Meeting Digest

The user-facing, incrementally updated meeting note. It is generated from meeting transcript memory by a text model and may lightly rewrite earlier wording to merge repetition, fix obvious recognition errors, and preserve confirmed facts.

## Meeting Record

A local record of one meeting, including its audio, transcript, Meeting Digest, and lifecycle state. A Meeting Record may exist without any remote Meeting Room.

## Recording Run

A continuous audio-capture interval inside a Meeting Record. Pausing ends the current Recording Run while keeping the Meeting Record open for later continuation.

## Finalization

The background work after microphone capture stops. It lets pending ASR chunks finish, merges their transcript into the owning Meeting, and runs the final Meeting Digest update when a text model is configured.

## Archived Meeting Record

A Meeting Record whose user-facing recording has ended. It may briefly be finalizing before becoming archived; late ASR or digest results belong only to that Meeting Record.

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

## Wake Phrase

A text-level trigger such as `嗨助手`, `嘿助手`, `助手`, or `hey assistant` detected inside recognized transcript text. It starts assistant command capture; it is not audio-level wake-word detection.

## Assistant Command Capture

The temporary state after a wake phrase is detected. Transcript after the wake phrase is treated as an instruction for the Copilot until a silence window ends the capture. Captured commands are not part of the Meeting Digest. Stop and Pause cancel unfinished Assistant Command Capture, but Finalization still preserves the meeting content before the wake phrase.

## Release Provider Policy

The release build requires a configured cloud ASR Provider for transcription and a configured Agent Provider for Meeting Digest and Copilot work. API keys are kept in memory only. Local ASR models are not exposed or used as fallback in the release build.

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

## Meeting

An isolated meeting record with its own Raw ASR Segments, Meeting Digest, Copilot responses, and lifecycle state. Stopping a Meeting archives it; starting again creates a separate Meeting whose Copilot context does not include archived Meetings by default.

## Recording Run

A continuous microphone capture interval inside a Meeting. Pausing ends the current Recording Run but keeps the Meeting open, so later recording continues the same Meeting.

## Finalization

The background work after microphone capture stops. It lets pending ASR chunks finish, merges their transcript into the owning Meeting, and runs the final Meeting Digest update when a text model is configured.

## Archived Meeting

A Meeting whose user-facing recording has ended. It may briefly be finalizing before becoming archived; late ASR or digest results must update that Meeting only, never the next active Meeting.

## Wake Phrase

A text-level trigger such as `嗨助手`, `嘿助手`, `助手`, or `hey assistant` detected inside recognized transcript text. It starts assistant command capture; it is not audio-level wake-word detection.

## Assistant Command Capture

The temporary state after a wake phrase is detected. Transcript after the wake phrase is treated as an instruction for the Copilot until a silence window ends the capture. Captured commands are not part of the Meeting Digest. Stop and Pause cancel unfinished Assistant Command Capture, but Finalization still preserves the meeting content before the wake phrase.

## Release Provider Policy

The release build requires a configured cloud ASR Provider for transcription and a configured Agent Provider for Meeting Digest and Copilot work. API keys are kept in memory only. Local ASR models are not exposed or used as fallback in the release build.

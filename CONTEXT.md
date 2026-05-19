# Context Glossary

## Raw ASR Segment

A short, timestamped transcription fragment produced by the ASR pipeline. It is useful for debugging and evidence lookup, but it is not the primary user-facing meeting record.

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

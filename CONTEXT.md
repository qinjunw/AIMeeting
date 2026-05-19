# Context Glossary

## Raw ASR Segment

A short, timestamped transcription fragment produced by the ASR pipeline. It is useful for debugging and evidence lookup, but it is not the primary user-facing meeting record.

## Meeting Digest

The user-facing, incrementally updated meeting note. It is generated from meeting transcript memory by a text model and may lightly rewrite earlier wording to merge repetition, fix obvious recognition errors, and preserve confirmed facts.

## Wake Phrase

A text-level trigger such as `嗨助手`, `嘿助手`, `助手`, or `hey assistant` detected inside recognized transcript text. It starts assistant command capture; it is not audio-level wake-word detection.

## Assistant Command Capture

The temporary state after a wake phrase is detected. Transcript after the wake phrase is treated as an instruction for the Copilot until a silence window ends the capture. Captured commands are not part of the Meeting Digest.

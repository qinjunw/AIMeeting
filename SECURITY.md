# Security and privacy boundary

- API keys are kept in the current UI process memory and are cleared on restart.
- Meeting transcripts, summaries, and UI configuration are stored as plaintext local WebView data.
- Audio is streamed to the configured cloud ASR provider; text requests are sent to the configured model provider.
- Do not use this prototype for confidential meetings without an independent security and data-processing review.
- Never commit real API keys, captured meeting data, browser profiles, or generated installers.

Security issues should be reported privately to the repository owner rather than opened as a public issue.

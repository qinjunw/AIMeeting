# AIMeeting

Windows-first realtime meeting copilot prototype.

## Current MVP slice

- Start microphone transcription from the left rail with **Start mic**.
- Final speech recognition results are appended as meeting transcript segments.
- Interim speech recognition text appears in the rolling summary band while speaking.
- Wake phrases such as `嗨助手` and `hey assistant` switch the app into Dialogue mode and prefill the Copilot question with the text after the wake phrase.
- The Copilot can answer, summarize, or plan from the current transcript memory. Without a provider key it uses a deterministic local fallback so the workflow remains testable.
- Manual transcript input uses the same wake-phrase path, which makes trigger behavior easy to test without a microphone.

## Run the web prototype

```powershell
npm.cmd install
npm.cmd run dev
```

Open the Vite URL shown in the terminal. Chrome or Edge is recommended for the browser-native SpeechRecognition path.

## Run the desktop shell

```powershell
npm.cmd run desktop:dev
```

## Build

```powershell
npm.cmd run lint
npm.cmd run build
npm.cmd run desktop:build
```

The build generates:

- `src-tauri/target/release/aimeeting.exe`
- `src-tauri/target/release/bundle/msi/AIMeeting_0.1.0_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/AIMeeting_0.1.0_x64-setup.exe`

## Provider configuration

The UI accepts:

- `Base URL`: for example `https://api.openai.com/v1` or an OpenAI-compatible gateway.
- `Model`: for example `gpt-4.1-mini`.
- `Endpoint`: `chat` or `responses`.
- `API key`: kept in memory only by the current prototype; it is not persisted to local storage.

## Known limitations

- Realtime transcription currently uses browser/WebView SpeechRecognition when available. A cloud ASR provider is the next hardening step for environments where WebView2 does not expose SpeechRecognition.
- Speaker labels are still coarse placeholders.
- Meeting memory is still stored in local storage, not SQLite.
- Search logs queries, but no real search endpoint is configured by default.
- Wake phrases are text-level triggers from recognized transcript, not audio-level wake-word detection.

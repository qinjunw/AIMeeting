# AIMeeting Windows Release Checklist

## Current release

- Version: `0.1.2`
- Channel: unsigned release candidate
- Target: Windows x64
- Installer identity: `com.aimeeting.app`
- Primary artifact: NSIS setup executable
- Secondary artifact: MSI package

## Build commands

Run from the repository root:

```powershell
npm.cmd run lint
npm.cmd run build
cargo test --manifest-path src-tauri\Cargo.toml
npm.cmd run desktop:build -- --bundles nsis,msi
```

Expected artifacts:

```text
src-tauri\target\release\bundle\nsis\AIMeeting_0.1.2_x64-setup.exe
src-tauri\target\release\bundle\msi\AIMeeting_0.1.2_x64_en-US.msi
```

## Install test

- Install the NSIS setup executable on a Windows machine.
- Launch AIMeeting from the installed shortcut.
- Configure ASR Provider with `paraformer-realtime-v2`.
- Configure Agent Provider with the user's text model.
- Confirm microphone streaming transcription works.
- Confirm Stop produces a Meeting Digest.
- Confirm Meeting history Details opens the archived digest.

## Uninstall test

- Uninstall from Windows Settings > Apps.
- Confirm the installed app entry is removed.
- Confirm desktop/start menu shortcuts are removed.
- Reinstall the same version and confirm the app launches.

## Signing note

This release candidate is not code signed. It can run on Windows, but public users may see SmartScreen or publisher warnings. Public distribution should use a Windows code signing certificate or Azure Trusted Signing before broad release.

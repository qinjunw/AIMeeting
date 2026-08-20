# AIMeeting

AIMeeting 是一个 Windows 桌面会议工具，支持本地录音、实时转写和简体中文会议纪要。

<img width="2638" height="1574" alt="image" src="https://github.com/user-attachments/assets/00a45e6e-5e21-4413-b49f-9ec4427686ab" />


## 当前功能

- 麦克风和 Windows 系统声音独立开关，默认混音录制。
- 录音持久化为 Ogg Opus，转写失败不影响录音。
- 支持开始、暂停、恢复、结束和异常退出恢复。
- DashScope Paraformer 实时转写。
- 保存后的录音可重新转写并重新生成纪要。
- SQLite 保存会议、转写和处理状态。
- 支持录音播放、会议历史、回收站和永久删除。
- API Key 保存到 Windows Credential Manager。


## 运行

免安装版解压后运行 `AIMeeting.exe`。Windows 10/11 x64 需要 Microsoft Edge WebView2 Runtime。使用说明见 [Windows 快速开始](docs/quickstart-windows.md)。

源码开发需要 Node.js、Rust stable、MSVC C++ Build Tools、Windows SDK 和 WebView2 Runtime：

```powershell
npm.cmd install
.\aimeeting.cmd dev
```

构建并启动生产版：

```powershell
.\aimeeting.cmd build-app
.\aimeeting.cmd
```

完整检查与发布构建：

```powershell
.\aimeeting.cmd check
.\aimeeting.cmd portable
.\aimeeting.cmd release
```

`portable` 生成免安装 ZIP，`release` 生成未签名的 NSIS/MSI 安装包。发布说明见 [Windows 发布](docs/release-windows.md)。

## Provider

| 能力 | 支持方式 |
| --- | --- |
| 实时转写 | DashScope `paraformer-realtime-v2` |
| 录音文件转写 | OpenAI-compatible `/audio/transcriptions` 或兼容的 Qwen ASR |
| 会议纪要 | OpenAI-compatible Chat Completions 或 Responses 文字模型 |

远程 Provider 必须使用 HTTPS；本机回环地址允许使用 HTTP。

## 本地数据

会议、录音、转写和纪要默认保存在：

```text
%LOCALAPPDATA%\com.aimeeting.app
```

删除程序不会自动删除会议数据或 Provider 密钥。云端处理范围和删除规则见 [隐私说明](docs/privacy.md)。

## 代码结构

- `src/`：React 桌面界面、状态和 Tauri bridge。
- `src-tauri/src/audio/`：采集、混音、重采样和 Opus 写盘。
- `src-tauri/src/domain/`：会议、任务和 Provider 状态。
- `src-tauri/src/gateways/`：实时 ASR、文件 ASR、纪要和 Room Gateway 接口。
- `src-tauri/src/persistence/`：SQLite、文件、恢复和凭据。
- `src-tauri/src/runtime/`：录音线程、实时转写和后台任务。

验证状态见 [0.2.0 发布就绪记录](docs/release-readiness-0.2.0.md)。

# AIMeeting Windows 免安装版

## 启动

1. 解压整个 ZIP，不要直接在压缩包预览中运行。
2. 双击 `AIMeeting.exe`。
3. 首次打开后进入设置，分别配置实时转写、录音文件转写和会议纪要 Provider。
4. 保持麦克风与系统声音开启，点击“开始录音”。使用耳机可减少扬声器回声。

Windows 10/11 x64 需要 Microsoft Edge WebView2 Runtime。多数系统已预装；若程序无法打开，请安装 Microsoft 官方 Evergreen Runtime 后重试。

## 数据位置

录音、转写、纪要和会议索引保存在：

```text
%LOCALAPPDATA%\com.aimeeting.app
```

API Key 保存在当前 Windows 用户的 Credential Manager。删除 ZIP 或 `AIMeeting.exe` 不会删除这些数据和凭据。

## Provider

- 实时转写：当前使用 DashScope `paraformer-realtime-v2`。
- 录音文件转写：OpenAI-compatible `/audio/transcriptions`，或兼容的 Qwen ASR 接口。
- 会议纪要：OpenAI-compatible Chat Completions 或 Responses 文字模型。

远程 Provider 必须使用 HTTPS。本机 `localhost`、`127.0.0.1` 和 `::1` 调试服务可以使用 HTTP。

## 删除

会议先进入应用内回收站，只有“永久删除”或“清空回收站”才删除对应本地录音和记录。免安装版没有 Windows 卸载项；如需彻底清除，应先在应用内删除会议，再手动删除上述应用数据目录和 Credential Manager 中的 AIMeeting Provider 凭据。

## 当前限制

本发布候选未数字签名，Windows 可能显示未知发布者或 SmartScreen 提示。它不包含 Android 客户端、说话人分离、远端多人房间或本地 ASR 模型。

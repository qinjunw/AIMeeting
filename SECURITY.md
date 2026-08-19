# Security Policy

## Supported Version

当前只维护最新的 `0.2.x` 发布候选。早期原型不再接收安全修复。

## Reporting

发现安全问题时，请通过获得本项目的私有沟通渠道联系维护者，不要在公开 Issue、日志或截图中提交 API Key、会议录音、完整转写、数据库或 Windows 凭据内容。

报告建议包含：受影响版本、Windows 版本、最小复现步骤、影响范围，以及已脱敏的错误信息。维护者确认前请不要公开利用细节。

## Data Boundary

- 会议数据库与录音位于 `%LOCALAPPDATA%\com.aimeeting.app`。
- Provider API Key 由 Windows Credential Manager 保存，不写入 SQLite 或前端存储。
- 实时 ASR 会把混合音频发送到用户配置的 Provider；会议纪要会把转写文本发送到用户配置的文字模型 Provider。
- 本项目当前没有 AIMeeting 自营中转服务器、遥测或自动上传诊断文件。

用户应只配置可信的 HTTPS Provider，并自行审查其隐私、留存和合规政策。

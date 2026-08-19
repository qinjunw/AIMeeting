# AIMeeting 隐私说明

## 本地数据

AIMeeting 将会议索引、完整转写、会议纪要和录音文件保存在当前 Windows 用户的 `%LOCALAPPDATA%\com.aimeeting.app`。录音不会自动删除；移动到回收站后仍保留，只有用户执行“永久删除”或“清空回收站”才会移除。

## 云端处理

启用云端 ASR 时，会议混合音频会发送给用户配置的语音识别 Provider。生成会议纪要时，转写文本会发送给用户配置的文字模型 Provider。AIMeeting 不提供中转服务，用户应自行确认 Provider 的隐私、留存与合规政策。

## 密钥

Provider API Key 不写入浏览器 localStorage、SQLite、日志或录音目录。Windows 版本通过 Credential Manager 保存密钥，SQLite 只保留不含密钥的 Provider 配置和引用；导出的免安装包、会议数据和诊断信息不应包含明文密钥。

## 删除与恢复

普通删除会把会议和对应本地文件移入应用回收站。恢复会同时恢复数据库记录和文件目录。永久删除不可撤销，执行前应确认不再需要录音、转写或纪要。

删除免安装 ZIP 或 `AIMeeting.exe` 不会删除应用数据或 Credential Manager 中的密钥。需要彻底清除时，应先在应用内永久删除会议，再手动清理应用数据目录和 AIMeeting Provider 凭据。

## 遥测

当前版本没有 AIMeeting 自营服务器、使用分析或自动诊断上传。远端 Room Gateway 只有代码接口，不会建立房间连接。网络请求只发生在用户配置并触发 ASR 或会议纪要 Provider 时。

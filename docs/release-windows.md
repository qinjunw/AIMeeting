# AIMeeting Windows 发布

## 当前发布

- 版本：`0.2.0`
- 渠道：未签名、受控测试发布候选
- 目标：Windows 10/11 x64
- 应用标识：`com.aimeeting.app`
- 首选产物：免安装 ZIP
- 备选产物：NSIS/MSI 安装包

## 免安装版

```powershell
.\aimeeting.cmd portable
```

该命令执行完整 `npm run check`、构建 Tauri EXE、打包文档、生成 SHA-256，并运行不启动应用的结构校验。预期产物：

```text
release-staging\AIMeeting-0.2.0-windows-x64-no-install.zip
release-staging\AIMeeting-0.2.0-windows-x64-no-install.zip.sha256
```

免安装表示无需写入 Windows 安装项，不代表数据与 EXE 同目录。应用仍使用 `%LOCALAPPDATA%\com.aimeeting.app` 和 Windows Credential Manager；删除 EXE 不会清理用户数据。WebView2 Runtime 不打入 ZIP。

校验脚本不会在开发者真实用户环境中启动 EXE，以免启动恢复任务时读取真实会议、凭据或调用云端。启动验证必须在 Windows Sandbox、干净虚拟机或独立测试账号中进行。

## 安装包

```powershell
.\aimeeting.cmd release
```

预期生成 NSIS 和 MSI。安装包会创建卸载项；免安装 ZIP 没有卸载项。两者都不会在卸载时自动删除会议录音和 Provider 凭据，除非未来提供明确的数据清理选项。

## 干净机器测试

1. 核对 ZIP 的 SHA-256。
2. 在 Windows Sandbox 或没有 AIMeeting 数据的测试账号中解压全部文件。
3. 验证 WebView2 已安装与缺失两种场景。
4. 启动应用，分别配置实时转写、文件转写和会议纪要 Provider。
5. 执行麦克风、系统声音、双路、暂停恢复、断网、结束与重试。
6. 重启应用，核对历史、录音、转写、纪要和回收站。
7. 删除 EXE 后确认应用数据仍在，再按文档执行彻底清理。

## 签名

当前候选未签名，Windows 可能显示 SmartScreen 或未知发布者提示。公开分发前应使用 OV/EV 代码签名证书或 Azure Trusted Signing，并分别签名 EXE、NSIS/MSI 与最终发布流程产物。签名后仍需在干净机器重新验证安装、升级和卸载。

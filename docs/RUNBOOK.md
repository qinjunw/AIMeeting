# 运行手册

## Web 演示

```powershell
npm ci
npm run dev
```

Web 模式无需 Key 即可验证手工转写、文本唤醒、停止归档和本地历史。递增纪要与 Copilot 需要 Agent Provider；真实 DashScope 流式转写使用 Tauri 命令，需要桌面模式。

## 桌面模式依赖

- Node.js 20.19+ 或 22.12+
- Rust stable
- Visual Studio Build Tools，勾选“使用 C++ 的桌面开发”
- Microsoft Edge WebView2 Runtime
- Windows 麦克风权限

```powershell
npm ci
npm run desktop:dev
```

在设置中分别填写 DashScope ASR Key 和文本模型的 Base URL、Model、API Key。当前 UI 需要两项 Provider 均配置完整后才启用 Start mic。

## 构建

```powershell
npm run build
npm run desktop:build
```

## 常见问题

- 没有实时文字：确认运行的是 Tauri 桌面模式、麦克风权限已允许且 DashScope Key 有效。
- 手工输入可用但摘要失败：检查文本模型配置和网络代理。
- 历史信息不应保留：在录屏前使用清空功能，并清理 WebView 本地数据。
- 安装包被 Windows 警告：未签名的个人构建可能触发 SmartScreen；正式 Release 应配置代码签名。

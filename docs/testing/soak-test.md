# 录音稳定性测试

## 目的

验证长时间会议中录音链路持续写盘、转写队列拥塞不反压录音，以及进程资源不会出现明显的单调失控。测试输出只写入被 Git 忽略的 `soak-output/`。

## 自动加速测试

以下命令把 30 分钟的双源音频在非实时条件下送入生产 `AudioEngine`。它校验混音输出与 ASR 分支样本总数、录音分支零丢失，以及无意外的 ASR 降级。

```powershell
cargo run --release --manifest-path src-tauri/Cargo.toml --bin recording_soak -- --synthetic-minutes 30
```

成功标准：进程以 0 退出，并输出 `PASS synthetic_minutes=30`。

## 真实硬件测试

真实测试直接使用生产 `RecordingRegistry`、Windows 麦克风/系统声音采集和 Ogg Opus 写入器：

```powershell
cargo run --release --manifest-path src-tauri/Cargo.toml --bin recording_soak -- --realtime-minutes 30 --source mixed --output soak-output
```

每 5 秒输出一次：

- 已运行时间；
- 当前录音文件大小；
- 进程工作集内存；
- 进程累计 CPU 时间。

成功标准：

- 30 分钟内进程不崩溃；
- 录音文件持续增长且结束后可解码；
- `recorded_samples` 大于零；
- 工作集在初始预热后没有持续无界增长；
- ASR Provider 不可用时，录音仍能正常结束并保留文件。

真实测试会录下测试机器当前声音。运行前应关闭含敏感信息的应用，并优先使用耳机，避免扬声器回声进入麦克风。

## 结果记录

发布候选应在 `docs/release-readiness-<version>.md` 中记录测试日期、Windows 版本、音频设备、命令、结果与输出文件解码结论。没有完成真实 30 分钟测试时，不得写成“已通过”。

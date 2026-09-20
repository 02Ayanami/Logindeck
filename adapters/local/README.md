# 本机 QQ 适配

`qq-macos-v2-recording.json` 已通过完整自动退出实机验收：QQ 原生菜单 → 退出账号 → 核验重启后的新进程 → 账密登录页，约 3.5 秒、3 次点击。路径和预期重启由配置描述，录制器仍为独立本地工具。

本机 LoginDeck 的 QQ 记录现已更新为下文的鼠标点击配置；此菜单栏配置保留作为已验收的替代路径。用户名／密码填写仍待新版应用的 macOS 辅助功能授权后验收；不会自动提交登录。

`qq-macos-v2-fill.json` 是此前由独立录制器生成的仅填写配置，没有退出流程，不能单独启用完整账号切换。

实机证据及历史问题见 `docs/testing/2026-09-16-qq-v2-recording.md`。

`qq-guided-pointer-candidate.json` 是 2026-09-17 悬停录制的失败复现配置，未安装、未完整验收。其“更多”按钮能唯一定位，但原生点击后未打开菜单，单步超时且未重试。请勿用它替换已验证的 `qq-macos-v2-recording.json` 菜单栏路径；详细证据见 `docs/testing/2026-09-17-guided-recorder.md`。

`qq-macos-v2-pointer.json` 使用显式鼠标点击定位“更多”和“退出账号”，再进入账密登录页。配置采用 schema v3 状态转换：退出后允许出现 `quick_login` 或直接出现 `password`，执行器自行处理同进程换页、窗口重建和进程替换。登录流程会清空并填写凭据、勾选协议、提交，并在识别到主界面后完成。原先的 `qq-guided-pointer-candidate.json` 仍保留失败复现用途；v2 配置读取时会迁移为单后继状态。

# LoginDeck

macOS 与 Windows 本地账号密码管理工具。共用网站账号和应用账号页面；密码分别保存在 macOS 登录钥匙串或 Windows Credential Manager，账号元数据保存在本地 SQLite。Windows 基础平台已完成本机自动化验证与 debug 构建；实际验收边界见 [Windows 验证记录](docs/testing/2026-09-18-windows-foundation.md)。

## 当前功能

- 网站账号按网站分组，支持新增、编辑、逐个删除、复制账号密码和打开网站。删除最后一个账号后，网站分组自动消失。
- 应用通过自动扫描或手动添加导入，支持账号密码管理、复制和一键唤醒应用。仅允许移除没有账号的应用。
- 账号名称是本地备注，未填写时按所在网站或应用自动生成“账号 N”。每组默认展示三个账号，可展开其余账号。
- Edge 插件在已连接时提示保存网页登录时提交的账号密码。用户确认后，LoginDeck 根据网站与实际登录账号新增账号或更新密码，保留已有备注，并向插件返回操作结果。
- 插件保存提示在 15 秒后过期，成功反馈在 4 秒后消失。未连接时不显示保存提示。
- 网站页插件按钮左侧显示连接状态：绿点“已连接”、红点“未连接”。

当前产品不提供应用自动登录、自动填写、账号切换、短信登录、录制器或批量删除。

Windows 应用扫描合并 Win32 卸载注册信息和当前用户 MSIX/UWP 可启动应用，支持 `.exe` 手动导入、图标降级和启动前身份复核。身份材料是文件或包注册元数据，不是 Authenticode、发布者信任或文件内容认证；复核与进程创建也不是系统级原子事务。

## 安装 Edge 插件

macOS 与 Windows 均由“设置 → Edge 登录识别”自动准备插件文件和本机连接组件。Windows 会为当前用户写入固定的 Edge Native Messaging 注册，无需手工修改注册表或运行终端。

1. 打开 Edge 扩展管理页，开启“开发人员模式”。
2. 点击“加载解压缩的扩展”，选择教程第二步显示的插件文件夹；可复制路径或打开文件夹。
3. 从 Edge 工具栏的“扩展”菜单打开 LoginDeck。
4. 保持 LoginDeck 运行，在插件中点击“检测连接”。

Windows 插件目录位于 `%APPDATA%\com.autologin.desktop\edge-extension`；macOS 目录以界面显示的实际路径为准。首次使用仍需在 Edge 开发人员模式中加载一次解压缩扩展；这不是扩展商店自动安装。更新插件文件后需在 Edge 重新加载插件，并刷新已打开的登录页。

[中文使用说明](docs/help/edge-login-zh-CN.md) · [English guide](docs/help/edge-login-en.md)

## 开发与验证

仓库工具链固定 Rust 1.89.0、pnpm 11.19.0；CI 使用 Node.js 22。macOS 开发还需要 Xcode Command Line Tools。

```sh
cd app
pnpm install --frozen-lockfile
pnpm desktop
```

版本以 `app/package.json` 为唯一来源；`pnpm sync-version` 会同步 Tauri 配置和桌面 Cargo 包。
常用入口为 `pnpm typecheck`、`pnpm build`、`pnpm preflight` 和 `pnpm tauri build`。

macOS 仓库根目录的验证命令：

```sh
./scripts/verify-macos.sh
```

Windows 支持目标为 Windows 10 22H2 / Windows 11 x64；当前本机证据来自 Windows 11，Windows 10 尚未实测。需要 64 位 PowerShell 5.1 或更新版、Rust 的 `x86_64-pc-windows-msvc` 工具链、Visual Studio 2022 Build Tools 的 C++ 桌面开发工具与 Windows SDK、WebView2 Runtime，以及 Node.js 22、Corepack 和可获取 pnpm 11.19.0 的网络环境。首次打包还需要下载 Tauri 的 WiX/NSIS 构建工具。

从包含 `Cargo.toml` 的仓库目录运行：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-windows.ps1
```

脚本也可通过绝对路径从任意目录调用。它执行 Rust 格式检查、固定版本前端安装、生成桌面测试所需资源、整个 Rust workspace/all-targets 串行测试、脚本回归、前端类型检查/测试/构建，以及完整 Tauri debug EXE、MSI 和 NSIS 构建。使用显式 Corepack 版本以避免外层目录的 pnpm 默认版本覆盖应用固定版本；原生命令失败立即停止并保留退出码。运行需要当前用户桌面会话，会使用测试专属凭据、注册表项和受控剪贴板写入。MSI/NSIS 仅构建，不执行安装。

仓库采用共享核心加平台实现的单仓库结构：前端与 `autologin-core` 保持平台中立，
`platform-runtime` 按编译目标选择 `platform-macos` 或 `platform-windows`，不复制核心与前端。
详见[仓库架构说明](docs/repository-architecture.md)。

`app/preview.html` 是使用模拟数据和模拟本机命令的开发预览，不读取真实账号。预览中的连接灯不代表实际插件连接。

桌面开发与构建前会运行 `scripts/prepare-edge-bundle.mjs`，准备 Edge 插件与本机组件资源。对外分发需要完整应用包，不能只分发桌面可执行文件。Windows 支持当前用户 Edge 本机消息注册，但安装包尚未签名或发布，也不代表扩展商店发布；Windows 自动登录、自动填写、账号切换和 UI Automation 均不在本阶段范围。

## 存储与恢复

新密码使用当前平台的系统凭据库保存。编辑时留空密码保留原值。正常复制密码后在 30 秒内按写入标记清理剪贴板；用户随后复制的其他内容会保留。Windows 使用剪贴板序列号与持锁复核。强制退出或第三方剪贴板历史不在该清理保证内。

系统凭据库与 SQLite 是两个存储系统，使用持久化恢复记录处理操作中断。密码删除失败时保留账号元数据；启动及后续修改时会重试恢复。界面不提供维护面板。旧存储引用保留兼容处理，不自动迁移或丢弃。

## 范围与验收记录

- [已确认的产品设计](docs/product-redesign.md)
- [本轮收尾与验收汇总](docs/testing/2026-09-17-release-readiness.md)
- [Edge 实机测试记录](docs/testing/2026-09-17-edge-live-redesign.md)
- [代码审阅修复记录](docs/testing/2026-09-17-review-fixes.md)
- [Windows 基础平台验证与未验证项](docs/testing/2026-09-18-windows-foundation.md)

`adapters/`、`tools/adapter-builder/` 以及底层适配器、填写和切换实验属于历史研发内容，不作为当前产品入口或发布功能。`docs/superpowers/` 中的旧设计保留用于追溯，当前范围以产品设计文档为准。

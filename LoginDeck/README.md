# LoginDeck

macOS 本地账号密码管理工具。提供独立的网站账号和应用账号页面：密码保存在登录钥匙串，账号元数据保存在本地 SQLite。

## 当前功能

- 网站账号按网站分组，支持新增、编辑、逐个删除、复制账号密码和打开网站。删除最后一个账号后，网站分组自动消失。
- 应用通过自动扫描或手动添加导入，支持账号密码管理、复制和一键唤醒应用。仅允许移除没有账号的应用。
- 账号名称是本地备注，未填写时按所在网站或应用自动生成“账号 N”。每组默认展示三个账号，可展开其余账号。
- Edge 插件在已连接时提示保存网页登录时提交的账号密码。用户确认后，LoginDeck 根据网站与实际登录账号新增账号或更新密码，保留已有备注，并向插件返回操作结果。
- 插件保存提示在 15 秒后过期，成功反馈在 4 秒后消失。未连接时不显示保存提示。
- 网站页插件按钮左侧显示连接状态：绿点“已连接”、红点“未连接”。

当前产品不提供应用自动登录、自动填写、账号切换、短信登录、录制器或批量删除。

## 安装 Edge 插件

在“网站账号”页点击“浏览器插件”，直接查看安装教程。LoginDeck 自动准备插件文件和本机连接组件，无需用户使用终端。

1. 打开 Edge 扩展管理页，开启“开发人员模式”。
2. 点击“加载解压缩的扩展”，选择教程第二步显示的插件文件夹；可复制路径或打开文件夹。
3. 从 Edge 工具栏的“扩展”菜单打开 LoginDeck。
4. 保持 LoginDeck 运行，在插件中点击“检测连接”。

插件目录通常为 `~/Library/Application Support/LoginDeck/edge-extension`，以界面显示的实际路径为准。插件只提供连接检测，完整安装教程由 LoginDeck 提供。更新插件文件后需在 Edge 重新加载插件，并刷新已打开的登录页。

[中文使用说明](docs/help/edge-login-zh-CN.md) · [English guide](docs/help/edge-login-en.md)

## 开发与验证

需要 macOS、稳定版 Rust（桌面代码最低要求 1.89）、Node.js、pnpm 和 Xcode Command Line Tools。

```sh
cd app
pnpm install --frozen-lockfile
pnpm desktop
```

版本以 `app/package.json` 为唯一来源；`pnpm sync-version` 会同步 Tauri 配置和桌面 Cargo 包。
常用入口为 `pnpm typecheck`、`pnpm build`、`pnpm preflight` 和 `pnpm tauri build`。

仓库根目录的验证命令：

```sh
./scripts/verify-macos.sh
```

仓库采用共享核心加平台实现的单仓库结构：前端与 `autologin-core` 保持平台中立，
`platform-runtime` 负责选择当前目标实现，现阶段接入 `platform-macos`。未来 Windows 版在
Windows 电脑上完成实现与验证后，以新的平台 crate 接入同一仓库，不复制核心与前端。
详见[仓库架构说明](docs/repository-architecture.md)。

`app/preview.html` 是使用模拟数据和模拟本机命令的开发预览，不读取真实账号。预览中的连接灯不代表实际插件连接。

桌面开发与构建前会运行 `scripts/prepare-edge-bundle.mjs`，准备 Edge 插件与本机组件资源。对外分发需要完整应用包，不能只分发桌面可执行文件。本轮不打包、不签名、不发布。

## 存储与恢复

新密码使用 macOS 登录钥匙串保存。编辑时留空密码保留原值。正常复制密码后在 30 秒内按写入标记清理剪贴板；用户随后复制的其他内容会保留。强制退出或第三方剪贴板历史不在该清理保证内。

系统凭据库与 SQLite 是两个存储系统，使用持久化恢复记录处理操作中断。密码删除失败时保留账号元数据；启动及后续修改时会重试恢复。界面不提供维护面板。旧存储引用保留兼容处理，不自动迁移或丢弃。

## 范围与验收记录

- [已确认的产品设计](docs/product-redesign.md)
- [本轮收尾与验收汇总](docs/testing/2026-09-17-release-readiness.md)
- [Edge 实机测试记录](docs/testing/2026-09-17-edge-live-redesign.md)
- [代码审阅修复记录](docs/testing/2026-09-17-review-fixes.md)

`adapters/`、`tools/adapter-builder/` 以及底层适配器、填写和切换实验属于历史研发内容，不作为当前产品入口或发布功能。`docs/superpowers/` 中的旧设计保留用于追溯，当前范围以产品设计文档为准。

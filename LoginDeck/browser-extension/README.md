## 推荐：从桌面应用安装

在 LoginDeck 的设置中点击“安装 Edge 扩展”。应用自动配置本机组件并准备扩展文件，随后按图文引导在 Edge 加载即可。普通用户无需下面的开发构建命令。

> 当前版本：登录识别默认关闭。安装后请在 LoginDeck 设置中主动开启；关闭后拒绝新保存并清除候选。简明图文帮助见应用内“设置 → 安装与使用帮助”。

# LoginDeck Edge 扩展（macOS）

仅支持 Microsoft Edge。标准 HTTPS 登录表单提交后，扩展角标显示 **1**；打开扩展，核对网站和用户名，勾选“已成功登录”，再选择保存账号或更新密码。失败登录请选择“不保存”。自动填充、自动登录、验证码、注册、密码修改、跨域 iframe、无标准表单及跨页面两步登录不属于首版。

## 构建和安装

必须使用本次更新后的桌面包；它与 Native Messaging 共用跨进程写锁。先关闭旧版桌面进程再更新，不能同时运行旧版和新版。

在仓库根目录：

```sh
pnpm --dir app install --frozen-lockfile
node browser-extension/build.mjs
cargo build -p autologin-native-host --locked
pnpm --dir app exec tauri build --debug --bundles app
python3 installers/native-messaging/macos/install-edge.py \
  --binary target/debug/autologin-native-host
```

1. 在 Edge 打开 `edge://extensions`，启用开发人员模式。
2. 点击“加载解压缩的扩展”，选择本项目 `browser-extension/dist` 目录。
3. 确认扩展 ID 为 `afklnhhancnomdifldifpmdgklflbigi`，固定到工具栏。公钥让开发版 ID 稳定；不是付费签名证书，也不需要 Apple 开发者账号。
4. 安装时允许 Edge 提示的 HTTPS 网站访问权限。默认在所有 HTTPS 网站检测登录，无需逐站开启。更新已有扩展后如 Edge 要求确认新增权限，请确认；刷新已打开网页后生效。
5. 正常登录，角标出现 1 后在 60 秒内打开扩展并确认。关闭确认弹窗、不保存、关闭源标签或超时会丢弃候选。
6. 回到 LoginDeck 查看结果。桌面未运行时也可以保存；下次打开即可看到。

连接不可用时点击“检查连接”。桥接程序安装后 Edge 会自行启动它，不需要常驻服务器。扩展只保存按网站禁用的偏好；密码、用户名和候选不会写入浏览器持久存储。网页内容脚本默认在 HTTPS 网站顶层框架运行，可通过“此网站不再询问”排除网站，只处理真实用户操作触发的明确登录提交；不会监听网页 `postMessage`。

## 安全和一致性

- Native Messaging 的 stdio 是唯一浏览器/本机通道；没有监听端口、socket、URL 协议或秘密临时文件。
- Host 限定一个精确扩展 ID；消息带版本和请求 ID，32 KiB 上限、HTTPS origin、512 字节用户名和 4 KiB 密码限制，严格拒绝多余字段。
- 密码在 Rust `SecretString` 和清零的输入缓冲区中暂存；待确认候选 60 秒过期。JS 字符串无法保证物理清零，但不持久化，发送后立即解除引用。
- 页面不能发送确认命令；仅扩展自己的 popup 能确认。已有账号只查元数据，不读取密码。多条同网站/用户名记录或等待期间记录变化时拒绝覆盖。
- Native host 与桌面在整个 Keychain 写入、SQLite 提交和清理期间共享 OS 文件锁。写入前登记恢复意图，进程异常退出后可恢复。
- 已点击确认后连接中断，保存可能已经完成；先在桌面检查，不自动重放请求。
- 浏览器内置密码管理器可能同时提示保存，是否关闭它由用户自行选择。

## 验证

```sh
node browser-extension/build.mjs
node --test browser-extension/tests/*.test.mjs
cargo test --workspace --locked
pnpm --dir app test --run
```

自动测试覆盖保守表单检测、来源绑定、页面伪造确认、重复确认、协议边界、保存/更新/过期、更新冲突、独立数据库连接及真实子进程锁排他性。实际 Edge 安装和网站端到端验收须在用户允许加载本地扩展及授权测试网站后进行；构建通过不等同于此项通过。

## 卸载

在 Edge 中删除扩展，再运行：

```sh
python3 installers/native-messaging/macos/install-edge.py \
  --binary target/debug/autologin-native-host --uninstall
```

只移除本扩展的 Edge host 注册和桥接可执行文件，不删除已有密码库。

# QQ 原生 AX 探针验证

后续已将本记录中的 `login_probe` / `QqFill` 重构为配置驱动的 `login` / `NativeFill`。最新范围与验证见 [Adapter 引擎记录](2026-09-16-adapter-engine.md)，本文件保留原始探针阶段的实测过程。

## 本轮交付范围

新增 `platform_macos::login_probe` 及 `qq_ax_probe` 示例程序，用 Rust 直接调用 macOS AX 接口验证 QQ 的登录表单。属于开发验证接口，尚未接入桌面填写按钮、真实凭据读取或登录提交。

默认只读取是否存在唯一 QQ 实例、唯一窗口、账号/安全密码框的可写性，以及登录按钮可用性和前台状态。输出不含输入值、控件树、窗口截图或钥匙串引用。

显式测试写入仅接受编译在程序内的虚构内容；不接受外部账号或密码参数，不调用剪贴板，不点击登录、不修改协议或保存密码选项。执行前必须两栏均为空。执行顺序为写账号、写密码、清密码、清账号，每次写入后轮询 AX 确认。

## 保护措施及边界

- 单进程只允许一个探针持有执行权；测试绑定被消费后不能直接重放。
- 保留 NSRunningApplication 实例与 AX 窗口/控件引用。执行前重查进程存活、前台 PID、焦点窗口、完整表单、控件位置与大小。
- 聚焦输入框之后再检查焦点元素和窗口，随后定向设置该控件 AXValue，不发送全局按键。
- QQ 的焦点更新存在异步延迟：请求聚焦后最多等待 1 秒，每轮仍核验窗口、控件及取消状态。等待期间只接受原焦点或目标焦点，出现第三个焦点即停止；不重复发送聚焦请求。报告保留停止前的操作阶段，并区分焦点未确认和布局变化。
- 取消标记在每个操作及验证轮询检查。取消或错误后不执行后续清理，报告可能残留的字段，由用户自行处理。已经发出的 AX 调用无法撤回。
- 限制控件树节点数、深度、扫描时间与 AX 消息超时；不确定、多候选、属性不支持、原有输入内容均停止。
- 安全密码框只确认空/非空，不把其内容转换成日志。非空或掩码仅是 AX 状态确认，不等于密码准确或 QQ 提交逻辑已接受。
- 当前采用操作边界检查，不能保证捕捉检查之间极短的失焦再返回，也不保证与用户同时编辑时无竞争。因此本探针不能作为真实凭据执行器上线。
- 实例发现使用 QQ Bundle ID，未复用正式凭据流程所需的签名验证与启动证明；固定夹具是本接口的硬边界。
- 命令行宿主的辅助功能授权不证明最终 LoginDeck.app 已获得授权。生产接入仍需应用本身的权限引导和实机验收。

## 自动验证

- `cargo test -p platform-macos --locked`：46 项通过，1 项原有钥匙串交互测试跳过。
- 新增 7 项测试覆盖成功清空、窗口/前台变化停止、取消不重放、拒绝覆盖已有内容、超时不重试、单任务互斥，以及允许异步焦点更新但拒绝第三个焦点。
- `cargo clippy -p platform-macos --all-targets --locked -- -D warnings`：通过。
- `cargo check --workspace --all-targets --locked`：通过。
- `cargo build -p platform-macos --example qq_ax_probe --locked`：通过。

## 真实 QQ 结果

通过本机原生探针（非先前的 UI 工具）取得结果：

```text
username_writable: true
password_writable: true
submit_enabled: false
frontmost: false
```

随后显式执行测试写入，返回 `NotFrontmost`，两个可能残留标记均为 false；没有执行写入。UI 工具的 Raise 和控件点击未使原生前台检查通过，已请求用户手动切到 QQ 登录窗口。

用户确认已切到 QQ 后，原生前台检查通过。首次写入账号成功，但聚焦密码框后立即读取焦点仍是旧控件，触发保护停止；细化错误后复现为 `WritingPassword / FocusNotConfirmed`。UI 工具随后观察到密码框已聚焦且为空，账号为本次固定夹具。每次失败后均先重新观察并清空已确认的测试账号，再发起新测试。

增加上述有界焦点确认后，原生测试返回：

```text
phase: Complete
last_operation: ClearingUsername
username_may_remain: false
password_may_remain: false
error: None
```

本轮已完成真实 QQ 的 AX 层虚构账号写入、密码非空确认和两栏清空。结束后独立读取控件树及截图，均确认两栏为空；自动登录、记住密码和协议选项保持未选，登录按钮仍禁用。没有提交登录。

这证明 AX 层的写入/清空流程可执行，不证明密码字符精确一致或 QQ 登录提交逻辑已接受输入。未拍摄中间写入状态，因此此前 UI 工具观察到 AX 值与截图不一致的原因仍未完全确定；不能把本轮结果当作真实账号登录验收通过。

## 复现

```sh
cargo run -p platform-macos --example qq_ax_probe --locked
# QQ 位于前台且两栏为空时，显式执行虚构内容测试：
cargo run -p platform-macos --example qq_ax_probe --locked -- --write-fixture
```

运行终端可能改变前台状态。请由已有的后台执行环境启动测试，勿删除前台校验来绕过此条件。若失败结果提示字段可能残留，先在 QQ 中人工检查并清空，再发起新测试。

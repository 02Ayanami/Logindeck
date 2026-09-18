# 动态控件定位与真实鼠标点击

## 实现

v2 Click 增加 `method`，仅接受 `accessibility` 或 `pointer`；缺省保留原生 AX 行为。录制向导、手动编排、分步试运行、独立回放、通用运行器均传递同一个方式。未实现鼠标能力的驱动会在输入前拒绝，不调用原生点击作为替代。

macOS 鼠标方式重新核验快照、进程、前台、窗口、所选状态控件、目标可用性和矩形。系统级 AX 命中测试要求控件中心点命中同进程目标或其快照中的后代；不允许命中父容器、兄弟节点或其他进程。输入权限预检不弹窗。已有鼠标键或 Shift／Control／Option／Command 按住时拒绝派发。

事件在输入前完成分配，最后重新校验目标与命中，再消耗快照并派发单次左键 down/up。按下后保证配套释放，不在其间因焦点变化跳过释放。无自动补点、固定坐标配置、截图识别或自动提交登录。鼠标触发的退出复用原有显式重启核验。

系统无法将命中校验和事件派发原子化，仍存在最后校验后的极短 UI 变化窗口。多显示器遮挡组合未做完整人工验收，不宣称绝对防止所有外部窗口竞态。

## QQ 实机结果

用户手动登录，使用前一轮悬停录得的相同“更多”外层选择器。

- 首步：`adapter_replay ... qq-macos-v2-pointer.json logout --step 0` 得到 `step_complete=0 detected_state=account_menu attempted_actions=1`。独立界面观察确认业务菜单出现。该选择器的 AXPress 在此前测试超时。
- 关闭菜单并拖动窗口后完整回放，前两步显式使用 `pointer`。131ms 派发打开“更多”；1573ms 已核验重启后进程；4176ms 已识别快捷登录并执行原生账密登录点击；4312ms 检测密码登录页，`LoggedOut attempted_actions=3`。PID `66820 → 67309`。
- 后续只读检查已再次观察到 `home`，因此不将最后一次观察描述为仍停在登录页；没有额外派发退出或登录。
- 在该次命令行回放阶段，新文件尚未替换旧适配器或安装到 LoginDeck，也未读取或保存真实凭据；后续桌面验收见下文。

## 自动验证

- core adapter/switcher 契约与两个 crate 的 lib tests：76 通过，2 个原有专用测试忽略。
- 新增测试覆盖鼠标方式未知值／固定坐标配置拒绝、未支持的驱动拒绝、事件可能已生效时失败不重试、不退回 AXPress、重启能力不能隐式继承，以及矩形与子控件命中边界。
- platform-macos 示例及库 Clippy `-D warnings` 通过，两个本地示例构建通过。
- 桌面端 `cargo check -p autologin-desktop --locked` 通过。最后补充了鼠标重启步骤派发前的剩余时间检查，避免控件核验消耗完期限后仍发送点击；两个本地工具重新构建、Clippy 再次通过。

## 桌面包与实际导入

- 使用 `pnpm --dir app exec tauri build --debug --bundles app` 生成 `target/debug/bundle/macos/LoginDeck.app`，包含本次指针执行方式及 Edge 资源。完成本地 ad-hoc 签名，`codesign --verify --deep --strict` 通过。该包用于本机运行，未做发行公证。
- 旧包保存在 `/var/folders/4l/wts8mrzd02d3w2xdwds6n8g80000gn/T/logindeck-before-pointer-04tpbpb3/LoginDeck.app`。
- QQ 鼠标配置的填写流程现在显式记录 `Clear username`、`Clear password`、`Fill username`、`Fill password`；没有添加 Submit。普通 Fill 仍拒绝覆盖非空字段。桌面端只接受这四步的固定顺序或原有的两步空字段填写，并要求同一登录页状态。
- 退出旧 LoginDeck 实例，从生成包重新启动，网站和 QQ 应用详情正常显示。
- 经应用内“导入配置”文件选择器读取临时本地副本，预览显示 `com.tencent.qq-pointer-click` 和“可执行退出并填写”。点击“确认安装配置”后 QQ 当前配置标签已更新，切换入口仍可用。不是直接写 AdapterStore 的替代验收。
- 更新后的配置经 LoginDeck 文件选择器预览并确认安装。重签名后首次运行因辅助功能授权失效而在修改 QQ 前停止；重新启用现有 LoginDeck 权限后重试成功。
- QQ 当时已位于密码登录页，旧用户名 `<user>29706` 和旧密码均非空。桌面切换编排识别为 `LoginPage` 后直接执行录制的填写流程：清空旧用户名和密码，填入“小号”用户名 `3450048652` 及其密码。LoginDeck 显示“已填写，请在目标应用中检查并完成登录”；QQ 可见用户名正确、密码非空，登录按钮未被点击。
- 真实退出／重启路径由前述完整回放验证；本次没有重复已通过的悬停录制和退出操作，仅验证新增的显式清空与桌面填写路径。

## 显式清空补充验证

- core switcher 与 adapter 契约：28 项通过，其中新增测试覆盖 Clear 仅限登录页对应的普通／安全文本栏、Clear 不构成 Filled 结果、失败后不重试且不继续 Fill、未实现 Clear 的驱动在修改前拒绝。
- platform-macos 库：46 项通过，1 项原有钥匙串专用测试忽略；原有“普通 Fill 不覆盖已有数据”测试继续通过。
- desktop adapter plan：4 项通过，覆盖四步顺序、字段／状态错误拒绝和无 Submit。
- adapter_builder 的 Clear／Fill 动作映射有独立测试；两个本地示例均完成编译。

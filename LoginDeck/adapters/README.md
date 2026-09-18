# 内置登录 Adapter v1

> 历史研发工具：当前 LoginDeck 产品不提供自动登录、账号切换或录制器入口。以下内容仅保留用于研发追溯。

目标是复用状态识别和执行流程，把应用差异放在数据文件中。首个内置适配为 `qq-macos.json`。格式采用严格 JSON，沿用项目现有 serde 工具链；当前支持 v2 JSON 工作流描述，并提供独立本地录制工具；YAML 导入和社区安装入口仍在后续阶段。

## 数据与执行边界

- `AppDefinition`：`schema_version`、适配 ID、修订号、平台、Bundle ID、状态列表。账号和应用的绑定、签名要求、启动路径来自本机已保存记录，不能由 Adapter 替换。
- `LoginState`：状态 ID、状态类型、需要同时存在的控件和允许的动作。无匹配为未知；控件不唯一或多个状态同时匹配时停止。
- `Action`：v1 仅允许 `fill` 和 `request_user`。填写数据源是后端选择的 `username` / `password`，不接受明文值、凭据引用或自定义目标。

v1 的密码页必须包含普通账号文本框、安全密码框和提交按钮三个选择器；动作固定为先账号后密码。提交按钮只用于识别与位置示意，不执行点击。`manual` 状态仅有一个标志控件和 `request_user` 动作，检测后提示用户准备登录页并重新发起任务。

因此 v1 是标准双输入框填写的可配置实现，尚不是任意流程脚本系统。多步登录、扫码全过程、验证码状态细分、退出、提交、成功状态识别及 Session 切换需要后续增加经过验证的能力；配置不能自行开启这些能力。

## 选择器

```json
{
  "role": "secure_text_field",
  "names": ["输入QQ密码"],
  "ancestor": { "role": "web_area" }
}
```

角色是平台无关枚举；macOS 后端将 AXRole/AXSubrole 转换为这些角色。名称严格匹配标题、说明、占位文字中的任一个，区分大小写、不做模糊猜测。可选 `identifier` 必须额外匹配；可选 `ancestor` 要求父链中存在相符祖先。角色、名称、标识和祖先条件组合使用。读取的树中不包含输入框 AXValue。

允许的角色：`text_field`、`secure_text_field`、`button`、`menu_item`、`menu`、`static_text`、`group`、`web_area`、`window`。普通文本框不能冒充安全密码框。定义大小、状态数量、选择器长度和树深度均有限制；未知字段、未知动作和不支持的 schema 版本会拒绝加载。

## 接入路径

1. 在此目录增加经过审阅的 JSON。
2. 在 `crates/autologin-core/src/adapter.rs` 的 `BUILTINS` 中登记文件。
3. 使用该应用的控件快照添加识别和歧义测试。
4. 完成本机权限、签名、位置确认、填写、中断和界面变化的实机验收。

符合 v1 控件及动作模型的应用不需要另写识别器或填写器；注册表仍需重新编译。当前仅 QQ 已登记，不把模拟应用的测试通过当成第二个真实应用受支持。

前端通过后端 `fill_supported` 能力字段决定是否提供填写入口，不能按应用名称或前端传入配置选择执行规则。任务持有已校验定义和窗口引用，每次填写前后重新观察。取消、失焦、窗口变化及不确定结果都停止后续动作；不会自动重试或回滚已输入内容。

密钥读取、应用签名验证、目标进程绑定、前台检查、超时、单任务互斥和人工确认属于引擎责任，不允许由配置关闭。密码不经过前端、剪贴板、配置或日志。

## v2 通用切换引擎（开发中）

`autologin_core::switcher` 提供应用无关的状态和流程运行器；`platform_macos::login::NativeWorkflow` 实现原生 Driver。示例位于 `examples/switcher-v2.fixture.json`，仅供模拟验证，不是 QQ 的真实退出配置。支持菜单点击、填写、提交、状态等待和人工 Challenge 交接；流程描述可以直接解析，无需为每个应用新增执行器。

桌面保留 v1 填写任务，并新增 v2 本地配置导入和“按配置切换”入口。独立本地 Adapter Builder 已提供控件选择、流程编排及文件导出，见 `tools/adapter-builder/README.md`。v2 首版桌面执行范围为退出后填写，不提交登录；导入和任务交互见 `docs/testing/2026-09-16-adapter-import-execution.md`。状态、验证与原生限制见 `docs/testing/2026-09-16-switcher-v2-native.md`。

# LoginDeck Windows 基础平台设计

## 目标

在现有单仓库架构内增加可在 Windows 10 22H2 与 Windows 11 x64 上运行的基础平台实现，复用现有 React 前端、Tauri 命令层和 `autologin-core` 领域逻辑。第一阶段交付 Windows 凭据安全存储、受控剪贴板、接近“设置 → 应用 → 已安装的应用”覆盖范围的可启动应用发现、应用图标和应用启动。

## 范围

本阶段包含：

- 使用 Windows Credential Manager 保存、读取和删除网站及应用密码。
- 使用 Windows 原生剪贴板写入 Unicode 文本，并只在剪贴板内容未改变时按既有 30 秒策略清理。
- 合并 Win32 卸载注册信息与当前用户 MSIX/UWP 包清单。
- 只向用户展示能解析出有效启动入口的应用。
- 支持 Win32 可执行文件与 MSIX/UWP AUMID 两种启动方式。
- 提取 Win32 和 MSIX/UWP 应用图标，并继续向前端返回 PNG data URL。
- 在 `platform-runtime` 中按编译目标选择 Windows 实现。
- 在真实 Windows 机器上提供测试与验证脚本。

本阶段不包含：

- 应用自动登录、自动填写、账号切换或 Windows UI Automation。
- Windows 7、Windows 8、Windows on ARM 或 32 位 Windows。
- 全盘递归扫描可执行文件。
- 卸载、修复或更新应用。
- 复制一套 Windows 专用前端、桌面命令或领域逻辑。

## 总体架构

新增 `crates/platform-windows` crate，实现 `autologin-core` 已有的 `CredentialStore`、`Clipboard` 和 `ApplicationCatalog` 契约。所有 Win32、COM 和 WinRT 依赖都封装在该 crate 内；`platform-runtime` 只负责目标选择和稳定类型导出。

依赖方向保持为：

```text
frontend -> desktop commands -> autologin-core interfaces
                         \-> platform-runtime -> platform-windows
native-host ----------------^
```

`autologin-core` 只增加表达 Windows 平台所必需的中立数据能力，不依赖 Tauri、Win32、COM 或 WinRT。

## 应用发现

### 数据来源

`WindowsApplicationCatalog` 组合两个彼此独立的来源：

1. `Win32Inventory`
   - 读取当前用户和本机的卸载注册信息。
   - 同时读取 64 位和 32 位注册表视图。
   - 使用 `DisplayName`、`DisplayVersion`、`Publisher`、`DisplayIcon`、`InstallLocation`、卸载键身份及相关可用字段构造候选项。
   - 过滤 `SystemComponent=1`、无展示名称、更新/补丁、子组件以及无法解析启动入口的记录。

2. `PackagedAppInventory`
   - 通过 `Windows.Management.Deployment.PackageManager` 查询当前用户包。
   - 过滤框架包、资源包、不可用包和没有 Application 启动项的包。
   - 从包清单得到展示名称、版本、AUMID、可执行声明和图标资源。

开始菜单快捷方式是 Win32 启动目标解析的辅助索引，不是唯一清单来源。解析器优先匹配卸载记录提供的明确可执行路径，其次匹配当前用户与所有用户开始菜单中的 `.lnk`，最后在明确的 `InstallLocation` 根目录内做有边界的候选解析。不得扫描整个磁盘。

### 可启动过滤

候选项只有在满足以下任一条件时才进入 LoginDeck 扫描结果：

- Win32：存在可规范化、可访问的 `.exe` 启动目标。
- MSIX/UWP：存在仍属于当前用户有效包的 AUMID Application 启动项。

运行库、驱动、更新、卸载器、辅助进程和缺少启动入口的软件不会展示。常见卸载器名称和安装元数据只用于排除候选，不作为启动目标。

### 内部模型与持久化

平台 crate 使用内部 `WindowsAppCandidate` 表达来源差异，完成过滤和去重后才转换成 `DiscoveredApplication`。

- Win32 `platform_application_id` 由稳定的卸载键身份与规范化可执行文件身份导出。
- MSIX/UWP `platform_application_id` 使用 AUMID。
- Win32 `launch_target` 保存规范化绝对 `.exe` 路径。
- MSIX/UWP `launch_target` 保存带类型前缀的值：`aumid:<AUMID>`。
- `alternate_launch_targets` 仅保存同一身份下通过验证的其他启动目标。
- Windows 不需要 macOS security-scoped bookmark，因此 `path_access_ref` 为 `None`。
- `signature_identity` 保存足以在启动前重新确认同一应用的有界身份材料，不保存秘密。

共享 `Platform` 枚举增加 `Windows` 值。序列化值必须固定，数据库中的已有 macOS 行不迁移、不改写。

### 去重与排序

先在每种来源内部去重，再跨来源合并：

- 同一规范化可执行文件只保留一个 Win32 项。
- 同一 AUMID 只保留一个打包应用项。
- 若注册信息和打包应用最终指向同一已打包桌面应用，优先使用 AUMID 记录，因为其身份和激活方式更稳定。
- 展示名称只用于排序和辅助匹配，不作为唯一身份。
- 最终结果按不区分大小写的展示名称与稳定 ID 排序，保证重复扫描顺序稳定。

## 应用导入、图标和启动

### 手动导入

Windows 手动导入接受单个 `.exe` 文件或包含明确应用入口的目录。目录解析有深度和候选数上限，不跟随目录联接、符号链接或重解析点。导入结果必须经过与自动发现相同的身份验证和过滤。

MSIX/UWP 应用由自动清单发现，不通过选择 `WindowsApps` 中的受保护文件手动导入。

### 图标

- Win32 图标优先使用经过验证的 `DisplayIcon`，否则从主可执行文件提取。
- MSIX/UWP 图标从包清单声明的视觉资源中按缩放资源规则选择。
- 平台层将图标转换为 PNG 并返回现有 data URL 格式。
- 图标提取失败只返回 `None`，不使应用发现或列表加载失败。

### 原子验证与启动

`ApplicationCatalog::verify_and_launch` 继续是唯一公开启动入口。

- Win32：重新解析并规范化目标，确认文件存在且身份与持久化记录一致，然后通过 Windows 进程创建 API 启动并返回真实 PID。
- MSIX/UWP：重新确认包仍为当前用户注册、AUMID 仍存在且身份一致，再通过 `IApplicationActivationManager::ActivateApplication` 激活并返回 PID。
- 身份不一致、应用已卸载或目标消失时返回 `application.not_found`，不得退回到未经验证的备用命令。

## 凭据存储

`WindowsCredentialStore` 使用 Windows Credential Manager 的 `CRED_TYPE_GENERIC` 项。

- TargetName 使用 `LoginDeck/<service>/<uuid>` 命名空间。
- SQLite 只保存既有 `SecretRef`，不保存密码或 Credential Manager TargetName 之外的秘密材料。
- `put` 在写入前确认 TargetName 不存在，以满足“在预分配引用处创建且绝不覆盖”的核心契约。
- `reveal` 使用 `CredReadW`，并用 `CredFree` 释放系统分配的缓冲区。
- `delete` 使用 `CredDeleteW`；不存在时映射为既有的凭据不存在错误。
- 密码按 UTF-8 字节保存，并在写入前验证不超过 Windows Generic Credential blob 上限。
- 应用拥有的临时明文缓冲区在使用后主动清零。
- 日志、错误、调试输出和测试快照不得包含密码或完整凭据 blob。

状态查询 facade 保持与 macOS 相同的调用形状，Windows 后端名称为 `windows_credential_manager`，最近一次系统错误只暴露数值状态，不暴露秘密。

## 受控剪贴板

`WindowsClipboard` 实现现有的有界剪贴板契约，不提供读取任意剪贴板内容的公开能力。

- `write` 通过 `OpenClipboard`、`EmptyClipboard` 和 `SetClipboardData(CF_UNICODETEXT)` 写入密码。
- 写入成功后读取 `GetClipboardSequenceNumber()`，封装为 `WindowsClipboardChangeToken`。
- `clear_if_unchanged` 仅在当前序列号与 token 相同时再次打开并清空剪贴板。
- 若用户或其他应用在延迟期间改变了剪贴板，函数直接成功返回且不清除新内容。
- 剪贴板暂时被占用时执行次数有上限、总时长很短的重试；超过上限返回稳定错误，禁止无限等待。
- 所有 Win32 资源都通过小型 RAII 封装确保关闭或释放。

桌面层继续沿用现有 30 秒密码清理调度，不在 Windows 平台 crate 复制计时逻辑。

## 平台运行时接入

根 workspace 增加 `crates/platform-windows`。`platform-runtime` 使用目标条件依赖：

- `target_os = "macos"` 选择 `platform-macos`。
- `target_os = "windows"` 选择 `platform-windows`。
- 其他目标继续在编译期明确拒绝。

Windows 分支导出：

- `NativeApplicationCatalog = WindowsApplicationCatalog`
- `NativeClipboard = WindowsClipboard`
- `NativeCredentialStore = WindowsCredentialStore`
- `application_icon`
- `credential_last_status`
- `CREDENTIAL_BACKEND = "windows_credential_manager"`
- `PLATFORM = Platform::Windows`

自动登录相关模块不提供假实现。桌面命令和 native host 在本阶段不得依赖 Windows 自动登录能力。

## 错误处理

平台原生错误在 crate 边界转换为稳定的 `AppError` 代码。前端不接收本地路径之外的敏感系统细节，也不接收密码、注册表原始值或凭据内容。

关键映射：

- 凭据不存在：`credential.not_found`
- 凭据库/登录会话不可用：`credential.unavailable`
- 凭据访问被拒绝：`credential.denied`
- 剪贴板持续被占用或写入失败：`clipboard.unavailable`
- 应用发现整体不可用：`application.discovery_unavailable`
- 扫描超时：`application.scan_timeout`
- 应用已卸载、入口失效或身份变化：`application.not_found`
- 手动导入不受支持：沿用现有 unsupported import 错误

单个损坏的注册表项、包清单或图标不得使整个发现过程失败；它们被跳过并只记录不含秘密的诊断信息。只有数据源整体无法访问或任务超时才使扫描失败。

## 测试策略

所有行为变更按测试驱动方式实施，先观察针对缺失行为的测试失败，再写最小实现。

### 平台无关测试

- `Platform::Windows` 的序列化与数据库往返。
- Windows 启动目标编码的有界验证。
- 旧 macOS 数据与现有前端响应保持兼容。

### Windows 单元测试

- 32/64 位及用户/机器注册表记录归一化。
- 系统组件、更新、子组件、无名称和无启动入口记录过滤。
- `DisplayIcon` 的引号、图标索引与环境变量解析。
- 开始菜单快捷方式匹配与危险目标排除。
- MSIX/UWP 清单 Application 解析、AUMID 构造与框架/资源包过滤。
- Win32、AUMID 各自去重及跨来源优先级。
- 原生错误码到 `AppError` 的映射。
- 图标失败不影响应用条目。

### Windows 原生集成测试

- 使用测试专用命名空间创建、读取和删除 Credential Manager 项，并在测试结束时清理。
- 验证重复 `put` 不覆盖已有秘密。
- 验证超限秘密被拒绝且未产生凭据项。
- 写入剪贴板后序列号不变时可以清除。
- 写入后由测试替换剪贴板内容时不得清除替换内容。
- 从受控测试注册表与真实当前用户包源发现预期候选。
- 启动测试 Win32 程序并核对返回 PID；对可控的打包测试应用验证 AUMID 激活。

涉及用户全局状态的集成测试串行运行，并为注册表、凭据与剪贴板使用唯一测试命名空间。任何测试不得删除非测试创建的数据。

## 构建、验证与交付门槛

新增 `scripts/verify-windows.ps1`，在 Windows 10 22H2 或 Windows 11 x64 上运行：

1. Rust 格式检查。
2. Windows 平台 crate 单元与原生集成测试。
3. workspace Rust 测试。
4. 前端类型检查与构建。
5. Windows Tauri debug 或 release build 的真实编译。

只有在本机完整通过后才添加 Windows GitHub Actions job。CI 使用 Windows runner 执行可自动化的相同检查；需要交互式桌面会话的剪贴板或启动测试可保留为本机验证项，但必须在交付记录中明确列出并保存结果。

第一阶段完成标准：

- Windows 原生应用可启动并正常加载现有共享界面。
- 网站和应用账号密码能通过 Windows Credential Manager 完成新增、读取、更新流程所需的补偿操作与删除。
- 密码复制后按既有规则清理，且不会删除用户后来复制的内容。
- 应用扫描同时覆盖可启动 Win32 和 MSIX/UWP 应用，不展示不可启动组件。
- 已保存应用可在启动前重新验证并打开。
- 图标缺失时界面可降级，扫描不失败。
- `verify-windows.ps1` 在真实 Windows x64 机器上通过，并形成测试记录。

## 参考资料

- [Windows Installer Properties for the Uninstall Registry Key](https://learn.microsoft.com/en-us/windows/win32/msi/uninstall-registry-key)
- [PackageManager.FindPackagesForUser](https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager.findpackagesforuser)
- [IApplicationActivationManager::ActivateApplication](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationactivationmanager-activateapplication)
- [Windows Credentials Management APIs](https://learn.microsoft.com/en-us/windows/win32/api/wincred/)
- [GetClipboardSequenceNumber](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber)
- [Clipboard Operations](https://learn.microsoft.com/en-us/windows/win32/dataxchg/clipboard-operations)

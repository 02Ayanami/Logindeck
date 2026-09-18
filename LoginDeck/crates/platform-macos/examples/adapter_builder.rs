//! Standalone local Adapter Builder; never linked into the LoginDeck UI.
#[cfg(target_os = "macos")]
mod local {
    mod guided {
        include!("adapter_builder/guided.rs");
    }
    use autologin_core::{
        adapter::{CredentialField, Selector},
        switcher::{
            builder::Builder, ClickMethod, Definition, Flow, FlowKind, Outcome, State, StateKind,
            Step,
        },
        ApplicationCatalog, ApplicationRecord,
    };
    use platform_macos::{login::capture_workflow_nodes, MacApplicationCatalog};
    use std::{
        collections::BTreeMap,
        io::{self, Read, Write},
        path::{Path, PathBuf},
        time::Duration,
    };

    fn prompt(message: &str) -> Result<String, String> {
        print!("{message}: ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut input = String::new();
        if io::stdin()
            .read_line(&mut input)
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("输入已结束".into());
        }
        let input = input.trim().to_string();
        if input == "q" {
            return Err("已取消；没有写入文件".into());
        }
        Ok(input)
    }
    fn choose(message: &str, options: &[String]) -> Result<usize, String> {
        if options.is_empty() {
            return Err("还没有可选项，请先记录状态和控件".into());
        }
        println!("{message}");
        for (i, option) in options.iter().enumerate() {
            println!("  {}. {}", i + 1, option);
        }
        loop {
            if let Ok(index) = prompt("输入序号")?.parse::<usize>() {
                if (1..=options.len()).contains(&index) {
                    return Ok(index - 1);
                }
            }
            println!("请输入列表内的序号。");
        }
    }
    fn state(builder: &Builder, message: &str) -> Result<String, String> {
        let keys: Vec<_> = builder.states().keys().cloned().collect();
        Ok(keys[choose(message, &keys)?].clone())
    }
    fn control(builder: &Builder, state: &str) -> Result<String, String> {
        let keys: Vec<_> = builder.states()[state].controls.keys().cloned().collect();
        Ok(keys[choose("选择控件", &keys)?].clone())
    }
    fn next_states(builder: &Builder) -> Result<Vec<String>, String> {
        let first = state(builder, "操作后应出现哪个状态")?;
        let mut selected = vec![first];
        loop {
            let mut options = vec!["完成后继状态选择".to_string()];
            options.extend(
                builder
                    .states()
                    .keys()
                    .filter(|state| !selected.contains(state))
                    .map(|state| format!("也允许：{state}")),
            );
            let choice = choose("是否还允许其他后继状态？", &options)?;
            if choice == 0 {
                return Ok(selected);
            }
            let remaining: Vec<_> = builder
                .states()
                .keys()
                .filter(|state| !selected.contains(state))
                .cloned()
                .collect();
            selected.push(remaining[choice - 1].clone());
        }
    }
    fn credential_step(clear: bool, state: String, target: String, field: CredentialField) -> Step {
        if clear {
            Step::Clear {
                state,
                target,
                field,
            }
        } else {
            Step::Fill {
                state,
                target,
                field,
            }
        }
    }
    fn record_step(builder: &Builder, _kind: FlowKind) -> Result<Option<Step>, String> {
        let action = choose(
            "添加步骤",
            &[
                "点击".into(),
                "清空用户名".into(),
                "清空密码".into(),
                "填写用户名".into(),
                "填写密码".into(),
                "提交登录".into(),
                "等待状态".into(),
                "等待用户完成验证".into(),
                "勾选复选框".into(),
                "结束编排／取消替换".into(),
            ],
        )?;
        if action == 9 {
            return Ok(None);
        }
        let page = state(builder, "在哪个状态执行")?;
        let step = match action {
            0 | 5 => {
                let target = control(builder, &page)?;
                let next_states = next_states(builder)?;
                if action == 0 {
                    Step::Click {
                        state: page,
                        target,
                        next_states,
                        method: guided::choose_click_method()?,
                    }
                } else {
                    Step::Submit {
                        state: page,
                        target,
                        next_states,
                    }
                }
            }
            1..=4 => credential_step(
                action <= 2,
                page.clone(),
                control(builder, &page)?,
                if matches!(action, 1 | 3) {
                    CredentialField::Username
                } else {
                    CredentialField::Password
                },
            ),
            6 => Step::WaitState { state: page },
            7 => Step::Challenge {
                state: page,
                resume_state: state(builder, "用户完成后应出现哪个状态")?,
            },
            _ => Step::Check {
                state: page.clone(),
                target: control(builder, &page)?,
            },
        };
        Ok(Some(step))
    }
    fn record_flow(builder: &mut Builder) -> Result<(), String> {
        let id = prompt("流程名称（英文、数字、下划线，例如 logout；同名替换）")?;
        let kind = choose("流程用途", &["退出当前账号".into(), "填写／登录".into()])?;
        let kind = if kind == 0 {
            FlowKind::Logout
        } else {
            FlowKind::Login
        };
        let outcome = if kind == FlowKind::Logout {
            Outcome::LoggedOut
        } else {
            match choose(
                "完成条件",
                &["已提交".into(), "观察到已登录界面（不验证账号身份）".into()],
            )? {
                0 => Outcome::Submitted,
                _ => Outcome::LoggedInObserved,
            }
        };
        println!(
            "按执行顺序添加步骤。点击后必须指定下一状态；退出和登录成功流程应以等待状态结束。"
        );
        let mut steps = Vec::new();
        while let Some(step) = record_step(builder, kind)? {
            if steps.len() == 64 {
                return Err("步骤不能超过 64 个".into());
            }
            steps.push(step);
        }
        builder
            .set_flow(
                id,
                Flow {
                    kind,
                    timeout_ms: 120_000,
                    steps,
                    outcome,
                },
            )
            .map_err(|e| format!("流程无效：{e:?}"))
    }
    fn replace_flow_step(builder: &mut Builder) -> Result<bool, String> {
        let flow_id = {
            let ids: Vec<_> = builder.flows().keys().cloned().collect();
            ids[choose("选择要修改的流程", &ids)?].clone()
        };
        let mut flow = builder.flows()[&flow_id].clone();
        let choices: Vec<_> = flow
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| format!("步骤 {index}：{step:?}"))
            .collect();
        let index = choose("选择要替换的步骤", &choices)?;
        let Some(step) = record_step(builder, flow.kind)? else {
            return Ok(false);
        };
        flow.steps[index] = step;
        builder
            .set_flow(flow_id, flow)
            .map_err(|error| format!("步骤未替换：{error:?}"))?;
        Ok(true)
    }
    struct Args {
        application: PathBuf,
        output: PathBuf,
        input: Option<PathBuf>,
    }
    fn usage() -> &'static str {
        "用法：adapter_builder /Applications/QQ.app /完整路径/qq-adapter.json [--load /已有配置.json]"
    }
    fn parse_args(args: &[String]) -> Result<Args, String> {
        match args {
            [application, output] => Ok(Args {
                application: application.into(),
                output: output.into(),
                input: None,
            }),
            [application, output, flag, input] if flag == "--load" => Ok(Args {
                application: application.into(),
                output: output.into(),
                input: Some(input.into()),
            }),
            _ => Err(usage().into()),
        }
    }
    fn read_limited(path: &Path) -> Result<String, String> {
        let file = std::fs::File::open(path).map_err(|_| "配置文件不可用".to_string())?;
        if !file
            .metadata()
            .map_err(|_| "配置文件不可用".to_string())?
            .is_file()
        {
            return Err("配置路径不是文件".into());
        }
        let mut source = String::new();
        file.take(65_537)
            .read_to_string(&mut source)
            .map_err(|_| "配置文件不是有效 UTF-8".to_string())?;
        if source.len() > 65_536 {
            return Err("配置文件超过 64 KiB".into());
        }
        Ok(source)
    }
    fn draft_path(output: &Path) -> Result<PathBuf, String> {
        let name = output.file_name().ok_or("输出路径缺少文件名")?;
        let mut draft = name.to_os_string();
        draft.push(".autologin-draft");
        Ok(output.with_file_name(draft))
    }
    fn lock_path(output: &Path) -> Result<PathBuf, String> {
        let name = output.file_name().ok_or("输出路径缺少文件名")?;
        let parent = output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = std::fs::canonicalize(parent).map_err(|_| "输出目录不可用")?;
        let mut lock = name.to_os_string();
        lock.push(".autologin-lock");
        Ok(parent.join(lock))
    }
    struct SessionLock {
        _file: std::fs::File,
    }
    fn acquire_session_lock(output: &Path) -> Result<SessionLock, String> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let path = lock_path(output)?;
        let mut options = std::fs::OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o600);
        let file = options.open(&path).map_err(|_| "无法创建录制会话锁")?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| "无法保护录制会话锁")?;
        match file.try_lock() {
            Ok(()) => Ok(SessionLock { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(format!(
                "另一个录制器正在编辑同一输出：{}",
                output.display()
            )),
            Err(_) => Err("无法锁定录制会话".into()),
        }
    }
    fn same_existing_file(left: &Path, right: &Path) -> bool {
        std::fs::canonicalize(left)
            .ok()
            .zip(std::fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
    }
    fn write_file(path: &Path, source: &str, replace: bool) -> Result<(), String> {
        use std::os::unix::fs::OpenOptionsExt;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = path.file_name().ok_or("输出路径缺少文件名")?;
        let temp = parent.join(format!(
            ".{}.{}.tmp",
            name.to_string_lossy(),
            std::process::id()
        ));
        let result = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true).mode(0o600);
            let mut file = options.open(&temp).map_err(|_| "无法创建临时文件")?;
            file.write_all(source.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| "写入临时文件失败")?;
            if replace {
                std::fs::rename(&temp, path).map_err(|_| "原子替换文件失败")?;
            } else {
                std::fs::hard_link(&temp, path).map_err(|_| "输出文件已存在或无法创建")?;
                std::fs::remove_file(&temp).map_err(|_| "清理临时文件失败")?;
            }
            std::fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| "同步输出目录失败")
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result.map_err(str::to_owned)
    }
    fn autosave(builder: &Builder, path: &Path) -> Result<(), String> {
        let source = builder
            .draft_json()
            .map_err(|error| format!("草稿无法序列化：{error:?}"))?;
        write_file(path, &source, true)?;
        println!("草稿已自动保存：{}", path.display());
        Ok(())
    }
    fn changed<T: PartialEq>(
        before: &BTreeMap<String, T>,
        after: &BTreeMap<String, T>,
    ) -> (Vec<String>, Vec<String>, Vec<String>) {
        let added = after
            .keys()
            .filter(|key| !before.contains_key(*key))
            .cloned()
            .collect();
        let removed = before
            .keys()
            .filter(|key| !after.contains_key(*key))
            .cloned()
            .collect();
        let modified = after
            .iter()
            .filter(|(key, value)| before.get(*key).is_some_and(|previous| previous != *value))
            .map(|(key, _)| key.clone())
            .collect();
        (added, removed, modified)
    }
    fn list(values: &[String]) -> String {
        if values.is_empty() {
            "-".into()
        } else {
            values.join(", ")
        }
    }
    fn change_summary(baseline: Option<&Definition>, builder: &Builder) -> String {
        let empty_states: BTreeMap<String, State> = BTreeMap::new();
        let empty_flows: BTreeMap<String, Flow> = BTreeMap::new();
        let before_states = baseline.map(|value| &value.states).unwrap_or(&empty_states);
        let before_flows = baseline.map(|value| &value.flows).unwrap_or(&empty_flows);
        let (state_added, state_removed, state_modified) = changed(before_states, builder.states());
        let (flow_added, flow_removed, flow_modified) = changed(before_flows, builder.flows());
        format!(
            "状态：新增 [{}]；删除 [{}]；修改 [{}]\n流程：新增 [{}]；删除 [{}]；修改 [{}]",
            list(&state_added),
            list(&state_removed),
            list(&state_modified),
            list(&flow_added),
            list(&flow_removed),
            list(&flow_modified)
        )
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn recorder_keeps_clear_and_fill_actions_explicit() {
            assert!(matches!(
                credential_step(
                    true,
                    "password".into(),
                    "username".into(),
                    CredentialField::Username
                ),
                Step::Clear {
                    field: CredentialField::Username,
                    ..
                }
            ));
            assert!(matches!(
                credential_step(
                    false,
                    "password".into(),
                    "password".into(),
                    CredentialField::Password
                ),
                Step::Fill {
                    field: CredentialField::Password,
                    ..
                }
            ));
        }

        #[test]
        fn load_arguments_diff_and_atomic_writes_are_explicit() {
            let parsed = parse_args(&[
                "/Applications/Demo.app".into(),
                "/tmp/output.json".into(),
                "--load".into(),
                "/tmp/input.json".into(),
            ])
            .unwrap();
            assert_eq!(parsed.input, Some(PathBuf::from("/tmp/input.json")));
            assert!(parse_args(&["only-one".into()]).is_err());
            assert_eq!(
                draft_path(Path::new("/tmp/output.json")).unwrap(),
                PathBuf::from("/tmp/output.json.autologin-draft")
            );

            let baseline = Definition::parse(include_str!(
                "../../../adapters/examples/switcher-v2.fixture.json"
            ))
            .unwrap();
            let mut builder = Builder::from_definition(baseline.clone()).unwrap();
            builder.remove_flow("login");
            assert!(change_summary(Some(&baseline), &builder).contains("删除 [login]"));

            let directory = std::env::temp_dir()
                .join(format!("adapter-builder-write-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let output = directory.join("adapter.json");
            write_file(&output, "first", false).unwrap();
            assert_eq!(std::fs::read_to_string(&output).unwrap(), "first");
            assert!(write_file(&output, "unexpected", false).is_err());
            write_file(&output, "second", true).unwrap();
            assert_eq!(std::fs::read_to_string(&output).unwrap(), "second");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        fn session_lock_rejects_another_process_and_releases_on_drop() {
            let directory =
                std::env::temp_dir().join(format!("adapter-builder-lock-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let output = directory.join("adapter.json");
            let target = directory.join("must-not-be-opened");
            std::fs::write(&target, "untouched").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::{symlink, PermissionsExt};
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
                symlink(&target, lock_path(&output).unwrap()).unwrap();
                assert!(acquire_session_lock(&output).is_err());
                assert_eq!(
                    std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                    0o644
                );
                std::fs::remove_file(lock_path(&output).unwrap()).unwrap();
            }
            let held = acquire_session_lock(&output).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "local::tests::child_session_lock_probe",
                    "--ignored",
                ])
                .env("AUTOLOGIN_RECORDER_LOCK_PROBE", &output)
                .status()
                .unwrap();
            assert!(status.success(), "child process acquired the held lock");
            let path = lock_path(&output).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            drop(held);
            assert!(acquire_session_lock(&output).is_ok());
            std::fs::remove_dir_all(directory).unwrap();
        }

        #[test]
        #[ignore = "invoked by the parent lock test"]
        fn child_session_lock_probe() {
            let output =
                std::env::var_os("AUTOLOGIN_RECORDER_LOCK_PROBE").expect("parent lock probe path");
            assert!(acquire_session_lock(Path::new(&output)).is_err());
        }
    }
    pub fn run() -> Result<(), String> {
        let raw_args: Vec<_> = std::env::args().skip(1).collect();
        if raw_args == ["--help"] {
            println!("{}\n独立本地录制器；使用 --load 续录已有配置。每次完成编辑后自动保存旁路草稿，正式保存前显示变更摘要。", usage());
            return Ok(());
        }
        let args = parse_args(&raw_args)?;
        let _session_lock = acquire_session_lock(&args.output)?;
        let allow_replace = args
            .input
            .as_ref()
            .is_some_and(|input| same_existing_file(input, &args.output));
        if args.output.exists() && !allow_replace {
            return Err("输出文件已存在；只有 --load 的输入和输出为同一文件时才能原子替换".into());
        }
        let draft = draft_path(&args.output)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let catalog = MacApplicationCatalog::new();
        let mut found = runtime
            .block_on(catalog.import_bundle(&args.application))
            .map_err(|e| e.code().to_string())?;
        if found.len() != 1 {
            return Err("无法唯一确定应用，请指定一个 .app".into());
        }
        let found = found.remove(0);
        let mut app = ApplicationRecord::new(
            found.platform,
            found.platform_application_id,
            found.display_name,
        )
        .map_err(|e| e.code().to_string())?;
        app.launch_target = found.launch_target;
        app.path_access_ref = found.path_access_ref;
        app.signature_identity = found.signature_identity;
        app.version = found.version;
        println!(
            "本地录制：{}。仅记录你选择的控件描述，不读取输入值、钥匙串或截图。",
            app.display_name
        );
        println!("录制时你在目标应用中手动操作，工具只读取界面；任何输入处键入 q 可退出。\n请先在 macOS 辅助功能中允许此终端／工具。");
        let mut verified = runtime
            .block_on(catalog.verify_and_launch(&app))
            .map_err(|e| e.code().to_string())?;
        let baseline = args
            .input
            .as_ref()
            .map(|path| {
                Definition::parse(&read_limited(path)?)
                    .map_err(|error| format!("已有配置无效：{error:?}"))
            })
            .transpose()?;
        if baseline.as_ref().is_some_and(|definition| {
            definition.platform != app.platform
                || definition.application_id != app.platform_application_id
        }) {
            return Err("已有配置与指定应用不匹配".into());
        }
        let fresh = || {
            Builder::new(
                format!("{}-local", app.platform_application_id),
                app.platform,
                app.platform_application_id.clone(),
            )
            .map_err(|error| format!("{error:?}"))
        };
        let mut builder = if draft.exists() {
            match choose(
                &format!("发现未完成草稿：{}", draft.display()),
                &["恢复草稿".into(), "忽略并删除草稿".into()],
            )? {
                0 => {
                    let restored = Builder::from_draft(&read_limited(&draft)?)
                        .map_err(|error| format!("草稿无效，未覆盖：{error:?}"))?;
                    if restored.platform() != app.platform
                        || restored.application_id() != app.platform_application_id
                    {
                        return Err("草稿与指定应用不匹配，未删除草稿".into());
                    }
                    println!("已恢复草稿；最终保存前仍会执行完整配置校验。");
                    restored
                }
                _ => {
                    std::fs::remove_file(&draft).map_err(|_| "无法删除旧草稿")?;
                    match baseline.clone() {
                        Some(definition) => Builder::from_definition(definition)
                            .map_err(|error| format!("{error:?}"))?,
                        None => fresh()?,
                    }
                }
            }
        } else {
            match baseline.clone() {
                Some(definition) => {
                    Builder::from_definition(definition).map_err(|error| format!("{error:?}"))?
                }
                None => fresh()?,
            }
        };
        if let Some(input) = &args.input {
            println!(
                "已加载：{}。可以只替换一个状态、流程或流程步骤。",
                input.display()
            );
        }
        loop {
            println!(
                "\n已记录 {} 个状态、{} 个流程。",
                builder.states().len(),
                builder.flows().len()
            );
            match choose("本地 Adapter Builder", &["记录／替换状态".into(), "编排／替换整个流程".into(), "替换流程中的单个步骤".into(), "删除状态".into(), "删除流程".into(), "查看相对原配置的变更".into(), "验证并保存文件".into(), "退出并保留自动草稿".into(), "排除弹出菜单／叠加状态".into(), "引导录制退出流程（鼠标指认）".into(), "重新识别一个已录制页面".into(), "分步试运行退出流程".into(), "放弃自动草稿并退出".into()])? {
                0 => {
                    let id = prompt("状态名称（例如 home、account_menu、login_page）")?;
                    let kind = match choose("状态类型", &["已登录".into(), "登录页".into(), "中间页面／菜单".into(), "验证码／设备确认".into()])? {
                        0 => StateKind::LoggedIn, 1 => StateKind::LoginPage, 2 => StateKind::Intermediate, _ => StateKind::Challenge,
                    };
                    prompt("按回车开始 5 秒准备时间，然后切到目标应用并打开要记录的页面／菜单")?;
                    std::thread::sleep(Duration::from_secs(5));
                    let nodes = match capture_workflow_nodes(&verified) {
                        Ok(nodes) => nodes,
                        Err(e) => { println!("读取失败：{e:?}。请检查辅助功能权限、目标应用前台状态及窗口数量，再重试。"); continue; }
                    };
                    println!("读取完成，可返回终端。以下仅列出能唯一定位的控件（序号不是坐标）：");
                    for (index, node) in nodes.iter().enumerate() {
                        if Selector::from_node(&nodes, index).is_ok() {
                            // Debug escaping prevents control sequences in application metadata.
                            println!("  {index}: {:?} 名称={:?} 标识={:?}", node.role, node.names, node.identifier);
                        }
                    }
                    let mut selected = Vec::new();
                    loop {
                        let index = prompt("要标记的控件序号；留空结束此状态")?;
                        if index.is_empty() { break; }
                        let Ok(index) = index.parse::<usize>() else { println!("请输入数字"); continue; };
                        if Selector::from_node(&nodes, index).is_err() { println!("该控件无法唯一定位，请换一个特征"); continue; }
                        let label = prompt("控件名称（例如 menu、logout、username、password、submit、success）")?;
                        selected.push((label, index));
                    }
                    match builder.record_state(id, kind, nodes, &selected) {
                        Ok(()) => {
                            println!("状态已记录");
                            autosave(&builder, &draft)?;
                        }
                        Err(e) => println!("状态未保存：{e:?}；需要至少一个唯一控件，名称只能使用英文、数字、点、下划线或连字符"),
                    }
                }
                1 => match record_flow(&mut builder) {
                    Ok(()) => autosave(&builder, &draft)?,
                    Err(error) => println!("{error}"),
                },
                2 => match replace_flow_step(&mut builder) {
                    Ok(true) => autosave(&builder, &draft)?,
                    Ok(false) => println!("没有替换步骤"),
                    Err(error) => println!("{error}"),
                },
                3 => {
                    let id = prompt("要删除的状态名称")?;
                    builder.remove_state(&id);
                    autosave(&builder, &draft)?;
                }
                4 => {
                    let id = prompt("要删除的流程名称")?;
                    builder.remove_flow(&id);
                    autosave(&builder, &draft)?;
                }
                5 => println!("{}", change_summary(baseline.as_ref(), &builder)),
                6 => {
                    match builder.export() {
                        Ok(json) => {
                            println!("相对启动时配置的变更：\n{}", change_summary(baseline.as_ref(), &builder));
                            println!("将写入以下已选控件和步骤，请检查是否包含不应保存的界面文字：\n{json}");
                            if prompt("输入 save 保存；其他输入返回修改")? != "save" { continue; }
                            write_file(&args.output, &json, allow_replace)?;
                            if draft.exists() {
                                std::fs::remove_file(&draft).map_err(|_| "配置已保存，但自动草稿清理失败")?;
                            }
                            println!("已保存：{}\n这是本地适配文件，尚未安装到 LoginDeck；真实兼容性请以完整试运行结果为准。", args.output.display());
                            return Ok(());
                        }
                        Err(e) => println!("配置未通过校验：{e:?}。请检查状态是否重叠、步骤的前后状态是否相接、输入框角色及最终完成条件。可替换状态或流程后再保存。"),
                    }
                }
                7 => {
                    autosave(&builder, &draft)?;
                    return Ok(());
                }
                8 => {
                    let target = state(&builder, "需要排除弹层的背景状态")?;
                    let overlay = state(&builder, "弹层状态")?;
                    let selected = control(&builder, &overlay)?;
                    builder.exclude_control(&target, &overlay, &selected).map_err(|e| format!("{e:?}"))?;
                    autosave(&builder, &draft)?;
                }
                9 => {
                    if let Err(error) = guided::record_logout(&mut builder, &mut verified, &runtime, &catalog, &app, &draft) { println!("{error}"); }
                    autosave(&builder, &draft)?;
                }
                10 => if let Err(error) = guided::verify(&builder, &verified) { println!("{error}"); },
                11 => if let Err(error) = guided::replay(&builder, &mut verified) { println!("{error}"); },
                _ => {
                    if draft.exists() {
                        std::fs::remove_file(&draft).map_err(|_| "无法删除自动草稿")?;
                    }
                    return Ok(());
                }
            }
        }
    }
}
#[cfg(target_os = "macos")]
fn main() {
    if let Err(error) = local::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("当前本地录制器仅支持 macOS。");
    std::process::exit(1);
}

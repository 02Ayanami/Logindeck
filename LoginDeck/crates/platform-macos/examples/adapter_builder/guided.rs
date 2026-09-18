use super::*;
use autologin_core::{
    adapter::{Node, Role},
    VerifiedApplication,
};
use platform_macos::login::capture_workflow_pointer;

fn prepare(message: &str) -> Result<(), String> {
    prompt(message)?;
    for remaining in (1..=5).rev() {
        println!("{remaining}…");
        std::thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

fn pick(
    verified: &VerifiedApplication,
    message: &str,
    action: bool,
) -> Result<(Vec<Node>, usize), String> {
    loop {
        prepare(&format!(
            "{message}。回车后切回应用，把鼠标停在目标控件上（不要点击）"
        ))?;
        let mut capture = match capture_workflow_pointer(verified) {
            Ok(capture) => capture,
            Err(error) => {
                println!("未能指认控件：{error:?}。请保持目标应用在前台；自绘控件可能不支持辅助功能。输入 q 可退出。 ");
                continue;
            }
        };
        capture.candidates.retain(|index| {
            if action {
                matches!(
                    capture.nodes[*index].role,
                    Some(Role::Button | Role::MenuItem)
                )
            } else {
                matches!(
                    capture.nodes[*index].role,
                    Some(Role::Button | Role::TextField | Role::SecureTextField | Role::StaticText)
                )
            }
        });
        if capture.candidates.is_empty() {
            println!("鼠标下没有适合此步骤的可定位控件，请换一个目标；输入 q 可退出。");
            continue;
        }
        let options: Vec<_> = capture
            .candidates
            .iter()
            .enumerate()
            .map(|(level, index)| {
                let node = &capture.nodes[*index];
                format!(
                    "由内向外第 {} 个候选：{:?} 名称={:?}；定位规则={:?}",
                    level + 1,
                    node.role,
                    node.names,
                    Selector::from_node(&capture.nodes, *index).ok()
                )
            })
            .collect();
        let index =
            capture.candidates[choose("鼠标下的控件及其可定位祖先，请确认实际目标", &options)?];
        println!(
            "定位规则：{:?}",
            Selector::from_node(&capture.nodes, index).map_err(|e| format!("{e:?}"))?
        );
        if let Some(rect) = capture.rectangles[index] {
            let helper = std::env::current_exe()
                .map_err(|e| e.to_string())?
                .with_file_name("adapter_highlight");
            if helper.is_file() {
                prepare("回车后重新切回原页面，鼠标仍停在目标上，查看红框；请保持窗口位置不变")?;
                // Recheck the target and its geometry before showing an old rectangle.
                let fresh = match capture_workflow_pointer(verified) {
                    Ok(fresh) => fresh,
                    Err(error) => {
                        println!(
                            "预览前重新指认失败：{error:?}。此前步骤仍保留，请重新指认当前控件。"
                        );
                        continue;
                    }
                };
                let selector =
                    Selector::from_node(&capture.nodes, index).map_err(|e| format!("{e:?}"))?;
                let found = selector.locate(&fresh.nodes).ok();
                if !found.is_some_and(|i| {
                    fresh.candidates.contains(&i) && fresh.rectangles[i] == Some(rect)
                }) {
                    println!("页面、鼠标或目标位置已改变，请重新指认。");
                    continue;
                }
                let status = std::process::Command::new(helper)
                    .args(rect.map(|v| v.to_string()))
                    .status()
                    .map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err("高亮预览失败，请重试".into());
                }
            } else {
                println!("高亮组件未构建；可用 tools/adapter-builder/run.sh 启动完整录制器。目标范围：{rect:?}");
            }
        }
        if prompt("确认这是目标控件？输入 yes 接受，其他输入重新指认")? == "yes"
        {
            return Ok((capture.nodes, index));
        }
    }
}

pub fn record_logout(
    builder: &mut Builder,
    verified: &mut VerifiedApplication,
    runtime: &tokio::runtime::Runtime,
    catalog: &MacApplicationCatalog,
    app: &ApplicationRecord,
    draft: &Path,
) -> Result<(), String> {
    let id = prompt("退出流程名称，例如 qq_logout（新名称）")?;
    if builder.flows().contains_key(&id)
        || builder
            .states()
            .keys()
            .any(|key| key.starts_with(&format!("{id}_")))
    {
        return Err("此名称已存在，请换一个名称，避免替换已有录制".into());
    }
    println!("向导会把每次指认和后续页面串成退出流程。你手动点击；工具不会代你退出。每个页面都可重试，完成后仍需主菜单验证保存。");
    let mut states = Vec::new();
    let mut steps = Vec::new();
    let result = (|| {
        for number in 0..31 {
            let current = format!("{id}_{number}");
            let next = format!("{id}_{}", number + 1);
            let (nodes, index) = pick(
                verified,
                if number == 0 {
                    "打开已登录主界面，指认打开账号菜单的按钮"
                } else {
                    "打开当前页面，指认下一步按钮（如退出账号或确认退出）"
                },
                true,
            )?;
            builder
                .record_state(
                    current.clone(),
                    if number == 0 {
                        StateKind::LoggedIn
                    } else {
                        StateKind::Intermediate
                    },
                    nodes,
                    &[("action".into(), index)],
                )
                .map_err(|e| format!("状态录制失败：{e:?}"))?;
            states.push(current.clone());
            autosave(builder, draft)?;
            // Overlay actions must be absent from earlier backgrounds. Add only
            // explicit choices; export still cross-checks every captured page.
            if number > 0
                && choose(
                    "这个按钮是否只在新菜单／弹窗出现，背景页面仍在其后？",
                    &["否／不确定".into(), "是，将它作为前面页面的排除特征".into()],
                )? == 1
            {
                for background in &states[..states.len() - 1] {
                    builder
                        .exclude_control(background, &current, "action")
                        .map_err(|e| format!("{e:?}"))?;
                }
                autosave(builder, draft)?;
            }
            steps.push(Step::Click {
                state: current,
                target: "action".into(),
                next_states: vec![next.clone()],
                method: choose_click_method()?,
            });
            builder
                .set_flow(
                    id.clone(),
                    Flow {
                        kind: FlowKind::Logout,
                        timeout_ms: 120_000,
                        steps: steps.clone(),
                        outcome: Outcome::LoggedOut,
                    },
                )
                .map_err(|e| format!("{e:?}"))?;
            autosave(builder, draft)?;
            println!("现在请手动点击刚才确认的按钮，然后回到终端。");
            let finished = choose(
                "点击后出现了什么？",
                &[
                    "另一个菜单／确认页面，继续记录".into(),
                    "登录页，退出已完成".into(),
                ],
            )? == 1;
            prompt("请等待目标页面稳定，再按回车重新核验应用")?;
            *verified = runtime
                .block_on(catalog.verify_and_launch(app))
                .map_err(|e| e.code().to_string())?;
            if finished {
                let (nodes, index) = pick(
                    verified,
                    "指认登录页的稳定特征，例如登录按钮或账号输入框；不要选择账号昵称",
                    false,
                )?;
                builder
                    .record_state(
                        next.clone(),
                        StateKind::LoginPage,
                        nodes,
                        &[("login_marker".into(), index)],
                    )
                    .map_err(|e| format!("{e:?}"))?;
                states.push(next.clone());
                steps.push(Step::WaitState { state: next });
                builder
                    .set_flow(
                        id.clone(),
                        Flow {
                            kind: FlowKind::Logout,
                            timeout_ms: 120_000,
                            steps,
                            outcome: Outcome::LoggedOut,
                        },
                    )
                    .map_err(|e| format!("{e:?}"))?;
                autosave(builder, draft)?;
                match builder.export() {
                    Ok(_) => println!("退出流程已生成，全部录制页面交叉校验通过。可从主菜单重新识别或分步试运行，最后验证并保存。"),
                    Err(error) => println!("流程已保留在草稿中，但交叉校验未通过：{error:?}。请使用状态编辑／排除弹层功能修正后再试运行。"),
                }
                return Ok(());
            }
        }
        Err("退出流程页面过多，已取消本次向导".into())
    })();
    if result.is_err() {
        for state in states {
            builder.remove_state(&state);
        }
        builder.remove_flow(&id);
        autosave(builder, draft)?;
    }
    result
}

pub fn verify(builder: &Builder, verified: &VerifiedApplication) -> Result<(), String> {
    let expected = state(builder, "选择要重新识别的状态")?;
    prepare("回车后切回应用，重新打开所选页面；工具只检查识别结果")?;
    let nodes = capture_workflow_nodes(verified).map_err(|e| format!("采集失败：{e:?}"))?;
    builder
        .verify_state(&expected, &nodes)
        .map_err(|e| format!("重新识别未通过：{e:?}；请检查控件是否稳定、页面是否重叠"))?;
    println!("重新识别通过：{expected}。这只验证当前页面，尚未执行退出流程。");
    Ok(())
}

pub fn replay(builder: &Builder, verified: &mut VerifiedApplication) -> Result<(), String> {
    use autologin_core::switcher::{Definition, Driver, Error};
    use platform_macos::login::{activate_workflow_target, Cancellation, NativeWorkflow};
    use std::time::Instant;
    let definition = Definition::parse(
        &builder
            .export()
            .map_err(|e| format!("请先修正配置：{e:?}"))?,
    )
    .map_err(|e| format!("{e:?}"))?;
    let names: Vec<_> = definition
        .flows
        .iter()
        .filter(|(_, flow)| flow.kind == FlowKind::Logout)
        .map(|(id, _)| id.clone())
        .collect();
    let name = &names[choose("选择退出流程（会真实操作应用）", &names)?];
    let flow = &definition.flows[name];
    if flow
        .steps
        .iter()
        .any(|step| !matches!(step, Step::Click { .. } | Step::WaitState { .. }))
    {
        return Err("仅支持退出流程的点击和等待步骤".into());
    }
    let options: Vec<_> = flow
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| format!("步骤 {i}：{step:?}"))
        .collect();
    let start = choose("从哪一步开始？当前应用必须处于该步骤要求的状态", &options)?;
    for (index, step) in flow.steps.iter().enumerate().skip(start) {
        println!("将执行步骤 {index}：{step:?}。退出按钮会真实退出当前账号。");
        if prompt("输入 run 执行这一步，其他输入停止试运行")? != "run" {
            return Ok(());
        }
        let cancel = Cancellation::default();
        activate_workflow_target(verified, &cancel).map_err(|e| format!("{e:?}"))?;
        let mut driver = NativeWorkflow::bind(
            verified,
            &definition,
            cancel,
            |_| Err(Error::Driver),
            || Ok(()),
        )
        .map_err(|e| format!("{e:?}"))?;
        let result = (|| {
            let deadline = Instant::now() + Duration::from_millis(flow.timeout_ms);
            let snapshot = driver
                .observe_ready(Duration::from_secs(10))
                .map_err(|e| format!("{e:?}"))?;
            let detected = definition
                .detect(&snapshot.nodes)
                .map_err(|e| format!("{e:?}"))?;
            let expected = match step {
                Step::Click {
                    state,
                    target,
                    next_states,
                    method,
                } => {
                    if &detected.state != state {
                        return Err(format!("当前为 {}，应为 {state}；未点击", detected.state));
                    }
                    let target = *detected.controls.get(target).ok_or("目标控件缺失")?;
                    driver
                        .click_configured(
                            &snapshot,
                            target,
                            *method,
                            deadline.saturating_duration_since(Instant::now()),
                        )
                        .map_err(|e| {
                            format!("点击未确认成功：{e:?}；不会重试，请先检查应用当前状态")
                        })?;
                    next_states.first().ok_or("点击步骤没有后继状态")?
                }
                Step::WaitState { state } => state,
                _ => return Err("不支持的步骤".into()),
            };
            let mut last_state = Some(detected.state.clone());
            loop {
                if Instant::now() >= deadline {
                    return Err(format!("步骤 {index} 等待状态 {expected} 超时；最后识别到 {}。不会重复点击，草稿仍保留。请重录该步骤，或尝试应用菜单栏中的同类操作。", last_state.as_deref().unwrap_or("未知页面")));
                }
                match driver.observe() {
                    Ok(snapshot) => match definition.detect(&snapshot.nodes) {
                        Ok(found) if &found.state == expected => break,
                        Ok(found) => last_state = Some(found.state),
                        Err(Error::UnknownState) => last_state = None,
                        Err(e) => return Err(format!("状态检查失败：{e:?}；停止试运行")),
                    },
                    Err(Error::WindowUnavailable) => {}
                    Err(e) => return Err(format!("页面读取失败：{e:?}；停止试运行")),
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            println!("步骤 {index} 通过，已识别到 {expected}。回到终端确认下一步。");
            Ok(())
        })();
        *verified = driver.verified_application().clone();
        result?;
    }
    if start == 0 {
        println!("本次完整退出流程试运行通过；配置仍需选择验证并保存。");
    } else {
        println!("所选步骤及后续步骤通过；不代表完整退出流程已经通过。");
    }
    Ok(())
}

pub fn choose_click_method() -> Result<ClickMethod, String> {
    Ok(
        if choose(
            "此步骤的执行方式（失败后不会自动切换方式）",
            &[
                "辅助功能原生操作".into(),
                "重新定位控件后真实鼠标点击".into(),
            ],
        )? == 0
        {
            ClickMethod::Accessibility
        } else {
            ClickMethod::Pointer
        },
    )
}

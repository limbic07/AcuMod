//! 仅供维护者手动执行的真实 Agent 验收入口。
//!
//! 它不注册 Tauri command，也不包含在默认构建中。运行时会创建独立的知识包和
//! 空 MOD 数据根，防止题库调用接触用户已安装的知识包或 MOD 库。

use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::ipc::{Channel, InvokeResponseBody};

use crate::{
    operations::OperationReporter,
    services::{
        agent::{self, AgentCoordinator},
        knowledge, mhwdata,
    },
};

/// 在独立进程主线程上执行 30 道真实 DeepSeek 题库回合，并写出原始回答报告。
pub fn run() -> Result<(), String> {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "无法定位项目根目录。".to_string())?;
    let run_root = project_root.join("target").join("live-eval").join(format!(
        "runtime-{}-{}",
        process::id(),
        unix_nanos_now()?
    ));
    let knowledge_root = run_root.join("knowledge");
    let software_data_root = run_root.join("software-data");
    env::set_var("ACUMOD_LIVE_EVAL_KNOWLEDGE_ROOT", &knowledge_root);
    env::set_var("ACUMOD_LIVE_EVAL_SOFTWARE_DATA_ROOT", &software_data_root);
    let cleanup = LiveEvalRootGuard { root: run_root };

    install_fresh_knowledge(&knowledge_root)?;
    let status = knowledge::get_status()?;
    // 与正式应用保持相同的后台任务状态；清理类只读工具会从 App state
    // 取得进度上报器，即使本验收不会确认或执行任何文件操作。
    let app = tauri::Builder::default()
        .manage(crate::operations::OperationCoordinator::default())
        .manage(AgentCoordinator::default())
        .build(tauri::generate_context!())
        .map_err(|error| format!("无法创建本地验收应用：{error}"))?;
    let app_handle = app.handle().clone();
    let settings = agent::get_agent_settings(&app_handle)?;
    if !settings.api_key_configured {
        return Err("未配置 DeepSeek 访问密钥，无法开始真实题库验收。".to_string());
    }

    let mut report = String::from("# AcuAI 30 题真实模型验收原始记录\n\n");
    report.push_str("每题使用新的 Agent 会话，完整经过真实模型、工具路由、知识来源/字段标记校验和可见回复输出。报告不包含访问密钥；测试根目录和空 MOD 库在结束后自动清理。\n\n");
    report.push_str(&format!(
        "- 模型：`{}`（`{}`）\n- 测试知识包：{}\n- 测试 MOD 库：空目录，未读取用户本地 MOD\n\n",
        settings.model.display_name(),
        settings.model_api_name,
        status
            .packs
            .iter()
            .map(|pack| format!("{} {}", pack.pack_id, pack.version))
            .collect::<Vec<_>>()
            .join("；")
    ));
    let report_path = project_root
        .join("target")
        .join("live-eval")
        .join("acumod-question-bank-live.md");
    fs::create_dir_all(report_path.parent().unwrap())
        .map_err(|error| format!("无法创建题库报告目录：{error}"))?;
    let question_limit = env::var("ACUMOD_LIVE_EVAL_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(LIVE_QUESTION_BANK.len())
        .min(LIVE_QUESTION_BANK.len());
    let question_start = env::var("ACUMOD_LIVE_EVAL_START")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
        .min(LIVE_QUESTION_BANK.len());
    let questions = LIVE_QUESTION_BANK
        .iter()
        .skip(question_start)
        .take(question_limit)
        .collect::<Vec<_>>();

    let mut succeeded = 0usize;
    for question in &questions {
        println!("LIVE_EVAL_START {}", question.id);
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let captured_events = Arc::clone(&events);
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(value) = body {
                captured_events.lock().unwrap().push(value);
            }
            Ok(())
        });
        let result = tauri::async_runtime::block_on(agent::start_agent_turn(
            app_handle.clone(),
            AgentCoordinator::default(),
            question.prompt.to_string(),
            channel,
        ));
        let tool_trace = summarize_events(&events.lock().unwrap());

        report.push_str(&format!(
            "## {}\n\n**问题：** {}\n\n",
            question.id, question.prompt
        ));
        if tool_trace.is_empty() {
            report.push_str("**工具轨迹：** 未收到工具事件。\n\n");
        } else {
            report.push_str(&format!("**工具轨迹：** {}\n\n", tool_trace.join(" → ")));
        }
        match result {
            Ok(turn) => {
                succeeded += 1;
                println!("LIVE_EVAL_DONE {} success", question.id);
                report.push_str("**Agent 结果：** 成功\n\n~~~markdown\n");
                report.push_str(&turn.message);
                report.push_str("\n~~~\n\n");
            }
            Err(error) => {
                println!("LIVE_EVAL_DONE {} failure", question.id);
                report.push_str("**Agent 结果：** 失败\n\n~~~text\n");
                report.push_str(&error);
                report.push_str("\n~~~\n\n");
            }
        }
        // 网络或模型层失败也要立即保留原文，便于中途停止时精确复现。
        fs::write(&report_path, &report).map_err(|error| format!("无法写入题库报告：{error}"))?;
    }
    report.insert_str(
        report.find('\n').unwrap_or(report.len()) + 1,
        &format!("\n完成回合：{succeeded}/{}。\n", questions.len()),
    );
    fs::write(&report_path, report).map_err(|error| format!("无法写入题库报告：{error}"))?;
    drop(cleanup);
    env::remove_var("ACUMOD_LIVE_EVAL_KNOWLEDGE_ROOT");
    env::remove_var("ACUMOD_LIVE_EVAL_SOFTWARE_DATA_ROOT");
    println!("真实题库原始报告：{}", report_path.display());
    Ok(())
}

/// 使用当前用户凭据做一次真实联网搜索验收，不安装知识包、不读取 MOD 库也不修改会话。
pub fn run_web_search_probe() -> Result<(), String> {
    let app = tauri::Builder::default()
        .manage(crate::operations::OperationCoordinator::default())
        .manage(AgentCoordinator::default())
        .build(tauri::generate_context!())
        .map_err(|error| format!("无法创建联网搜索验收应用：{error}"))?;
    let result = tauri::async_runtime::block_on(agent::test_agent_web_search(&app.handle()))?;
    let rendered = serde_json::to_string(&result)
        .map_err(|error| format!("无法输出联网搜索验收结果：{error}"))?;
    println!("WEB_SEARCH_PROBE {rendered}");
    if result.page_read_succeeded {
        Ok(())
    } else {
        Err("DeepSeek 服务端搜索已成功，但白名单页面摘录未通过验收。".to_string())
    }
}

struct LiveEvalRootGuard {
    root: PathBuf,
}

impl Drop for LiveEvalRootGuard {
    fn drop(&mut self) {
        // 此目录由本次进程以唯一名称创建，只存放隔离验收资产。
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn install_fresh_knowledge(root: &Path) -> Result<(), String> {
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "无法定位项目根目录。".to_string())?;
    let build_root = project_root
        .join("references")
        .join("knowledge")
        .join("build");
    let reporter = OperationReporter::default();
    mhwdata::install_database_into(
        root,
        &build_root.join("acumod-mhwdata-15.10.acumhwdb"),
        &reporter,
    )?;
    for pack_name in [
        "acumod-dev-modding.acukb",
        "acumod-dev-game-guides.acukb",
        "acumod-dev-acumod-help.acukb",
    ] {
        knowledge::install_pack_into(
            root,
            build_root.join(pack_name).to_string_lossy().into_owned(),
            &reporter,
        )?;
    }
    Ok(())
}

fn summarize_events(events: &[String]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| {
            let value = serde_json::from_str::<serde_json::Value>(event).ok()?;
            let kind = value.get("kind")?.as_str()?;
            if kind == "toolStarted" {
                value
                    .get("toolName")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            } else if kind == "knowledgeEvidenceReady" {
                Some("knowledgeEvidenceReady".to_string())
            } else {
                None
            }
        })
        .collect()
}

fn unix_nanos_now() -> Result<u128, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|error| format!("系统时间不可用：{error}"))
}

struct LiveQuestion {
    id: &'static str,
    prompt: &'static str,
}

const LIVE_QUESTION_BANK: &[LiveQuestion] = &[
    LiveQuestion { id: "G01", prompt: "金狮子弱什么属性？头、普通前脚和硬化前脚的斩击、打击、弹肉质各是多少？" },
    LiveQuestion { id: "G02", prompt: "激昂金狮子发怒时前脚为什么会弹刀？冰属性该打哪里？" },
    LiveQuestion { id: "G03", prompt: "冰呪龙头破后肉质有什么变化？火属性还应该打头还是前脚？" },
    LiveQuestion { id: "G04", prompt: "煌黑龙不同活性时该带什么属性？为什么不能只说它永远弱冰？" },
    LiveQuestion { id: "G05", prompt: "猛爆碎龙红色粘菌前脚和普通前脚肉质一样吗？近战和弩该怎么理解差异？" },
    LiveQuestion { id: "G06", prompt: "想刷金狮子的刚角，应该破哪里？它和普通剥取材料是一回事吗？" },
    LiveQuestion { id: "G07", prompt: "煌黑龙的天鳞和天壳分别主要从什么途径拿？我该优先断尾还是多刷任务奖励？" },
    LiveQuestion { id: "G08", prompt: "本体主线的“收束之地”怎么解锁？我还缺哪些古龙任务？" },
    LiveQuestion { id: "G09", prompt: "我刚打完冰原主线，铁匠铺为什么没有大部分普通防具的幻化？下一步该做什么？" },
    LiveQuestion { id: "G10", prompt: "我想把冰原中后期大剑从“能过”配到“比较舒适”，应该先告诉你哪些信息？" },
    LiveQuestion { id: "G11", prompt: "匠这个技能到底改变什么？它能把所有武器都加出紫斩吗？" },
    LiveQuestion { id: "G12", prompt: "力量解放有什么效果，挨多少伤或打多久才触发？" },
    LiveQuestion { id: "G13", prompt: "黑龙套一共怎么做，每件要哪些材料、各要几个？" },
    LiveQuestion { id: "G14", prompt: "黑龙刃满斩味是什么颜色、白紫各有多少格，配匠后会多多少？" },
    LiveQuestion { id: "G15", prompt: "金狮子大剑和冰咒龙大剑，哪把实际伤害更高？" },
    LiveQuestion { id: "G16", prompt: "黄色伤害数字就一定能触发弱点特效吗？紫斩打硬肉会不会弹？" },
    LiveQuestion { id: "G17", prompt: "我用太刀打金狮子，看到它弱冰就应该无脑带冰太吗？" },
    LiveQuestion { id: "G18", prompt: "冰呪龙怕火还是爆破？如果我只想更容易打过它，先做哪一种准备？" },
    LiveQuestion { id: "G19", prompt: "我还没打到某个特别任务，AcuAI 能直接告诉我完整前置链吗？" },
    LiveQuestion { id: "G20", prompt: "我只有防卫队装备，第一次进冰原后该直接做什么？给我一套唯一标准答案。" },
    LiveQuestion { id: "M01", prompt: "我下载的压缩包最外层没有 nativePC，直接是 pl、wp 和 vfx，Acumod 导入后会放错位置吗？" },
    LiveQuestion { id: "M02", prompt: "两个 MOD 都改了同一张 TEX，游戏里到底用哪个？我把其中一个禁用后会发生什么？" },
    LiveQuestion { id: "M03", prompt: "我把一个 MRL3 和 MOD3 改到新目录，只移动模型和材质，为什么贴图没了？" },
    LiveQuestion { id: "M04", prompt: "这个防具 MOD 的 armor.am_dat 为什么会让冰狼和浴场套装长得一样？直接删掉 DAT 可以吗？" },
    LiveQuestion { id: "M05", prompt: "DAT 里五个部位只改了其中两个，我要把这套衣服改绑到别的防具，剩下三条该怎么处理？" },
    LiveQuestion { id: "M06", prompt: "防具 MOD 没有 EVAM，但带了一个 wp/slg/slg128_0000，它能自动跟着衣服换飞翔爪吗？" },
    LiveQuestion { id: "M07", prompt: "我把武器模型改绑了，EFX、EPV3 和 EVWP 也能一起随便改名迁移吗？" },
    LiveQuestion { id: "M08", prompt: "这个 MOD 导入后游戏黑屏，能不能直接把 nativePC 整个删掉试试？" },
    LiveQuestion { id: "M09", prompt: "清理冗余文件时，MOD 里的 PNG、README、DLL 和 Lua 脚本是不是都能删？" },
    LiveQuestion { id: "M10", prompt: "nativePC/plugins/CSharp/Loader 里的 DLL 和普通 CSharp 插件有什么区别？AcuMOD 能不能直接运行它测试？" },
];

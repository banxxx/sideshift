use super::*;

pub(super) fn build_readme(
    plan: &[PlanMod],
    counts: &PlanCounts,
    review: &[String],
    loader: LoaderKind,
    keep_dirs: &[String],
    keep_files: &[String],
    agree_eula: bool,
    installed: bool,
    skipped: &[String],
) -> Vec<String> {
    // 最后一维是「阶段 2.5 有没有在本机把 loader 装好并进了包」：README 的启动说法按它分叉
    let loader_line = match (loader, installed) {
        // 关着这一档才是原来那句：包里只有 installer，首次运行联网自装
        (LoaderKind::Forge | LoaderKind::NeoForge, false) => {
            "Forge/NeoForge：start 脚本首次运行会自动执行 installServer（需要本机 Java 与网络），届时生成 run.bat/run.sh 与服务器本体，之后以 run 脚本启动".to_string()
        }
        (LoaderKind::Forge | LoaderKind::NeoForge, true) => {
            "Forge/NeoForge：加载器与依赖已在本机装好并打进包，解压后直接运行 start.bat / start.sh，无需联网安装".to_string()
        }
        (LoaderKind::Fabric, _) => {
            // Fabric 没有 installer 可提前跑：包里那枚官方服务端 jar 自己是启动器，首启现拉 loader 与前置库
            // （实测见 `.scratch/installer-probe`）。写"直接运行"会让人把那段联网等待当成卡死
            "Fabric 服务端：start 脚本首次运行会联网装出加载器与前置库（需要本机 Java 与网络），之后同样以该脚本启动".to_string()
        }
    };
    let mut lines = vec![
        "SideShift 转换报告".to_string(),
        format!("剔除 {} · 保留 {} · 新增 {}", counts.remove, counts.keep, counts.add),
        loader_line,
    ];
    // 勾了包内同名文件时 builder 会让位（见 core::builder::emit_root），那句「已生成」就不成立。
    // 比的是落位名而不是整条勾选键：勾深层那档（`Config/eula.txt`）落出来也是包根这枚
    if keep_files
        .iter()
        .any(|f| parser::base_name(f).eq_ignore_ascii_case("eula.txt"))
    {
        lines.push("eula.txt 沿用包内那份（保留内容里勾了它）：本次未按 EULA 开关改写".to_string());
    } else if !agree_eula {
        lines.push("eula.txt 已生成但为 eula=false：首次启动前请改为 eula=true，否则服务端会拒绝启动".to_string());
    }
    let kept: Vec<&str> = keep_dirs.iter().chain(keep_files.iter()).map(|s| s.as_str()).collect();
    if !kept.is_empty() {
        lines.push(format!("已随包保留客户端目录/文件：{}", kept.join("、")));
    }
    if !review.is_empty() {
        lines.push(format!("待人工确认模组：{}", review.join("、")));
    }
    let removed: Vec<String> = plan
        .iter()
        .filter(|m| m.disposition == ModDisposition::Remove)
        .map(|m| m.name.clone())
        .collect();
    if !removed.is_empty() {
        lines.push(format!("已剔除客户端模组 {} 个", removed.len()));
    }
    // 缺件闸门放行才走到这一条：产物里就是没有这些模组，写在包里那张纸上，
    // 免得服主日后按整合包的模组表来数、数出一堆「莫名消失」的模组
    if !skipped.is_empty() {
        lines.push(format!(
            "缺少模组 {} 个（CurseForge 不通过接口发放下载链，两条取链路都拿不到，已按「允许跳过」跳过）：{}",
            skipped.len(),
            skipped.join("、")
        ));
    }
    lines
}

pub(super) fn output_name_of(file_name: &str) -> String {
    let stem = file_name
        .strip_suffix(".mrpack")
        .or_else(|| file_name.strip_suffix(".zip"))
        .or_else(|| file_name.strip_suffix(".7z"))
        .unwrap_or(file_name);
    format!("{stem}-server.zip")
}


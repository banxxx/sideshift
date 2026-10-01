use super::*;

/* ---------------- CurseForge / MCBBS 清单 ---------------- */

/// CF 那份 `manifest.json` 里本轮真正用到的两段。
/// **只声明编号、不内嵌 jar**：`files[]` 是 `{projectID,fileID,required}`，字节得联网取
/// （`overrides` 字段能改写格式壳的名字，但那条腿不需要读它——物理扫描按 `…/mods/` 自己定包根，
/// 名字改成什么都跟着走，写死反而多一处会过期的假设）
#[derive(Deserialize, Default)]
struct CfManifest {
    #[serde(default)]
    minecraft: CfMinecraft,
    #[serde(default)]
    files: Vec<CfFile>,
}

#[derive(Deserialize, Default)]
struct CfMinecraft {
    #[serde(default)]
    version: String,
    #[serde(default, rename = "modLoaders")]
    mod_loaders: Vec<CfModLoader>,
}

#[derive(Deserialize)]
struct CfModLoader {
    #[serde(default)]
    id: String,
}

/// `files[]` 的一条：CF 官方导出的包**只给这一对编号**，jar 字节、文件名、大小、校验值都不在包里
#[derive(Deserialize)]
struct CfFile {
    #[serde(default, rename = "projectID")]
    project_id: serde_json::Value,
    #[serde(default, rename = "fileID")]
    file_id: serde_json::Value,
    /// 缺字段按**必需**处理：把漏写当成可选，等于替用户决定「这枚少了也能跑」
    #[serde(default = "cf_required_default", rename = "required")]
    required: bool,
}

fn cf_required_default() -> bool {
    true
}

/// CF 那对 id 有的是数字、个别老包给字符串，两种都得吃（与 downloader 侧 `ident()` 同一口径）
fn cf_id(v: &serde_json::Value) -> String {
    v.as_u64()
        .map(|i| i.to_string())
        .or_else(|| v.as_str().map(|s| s.trim().to_string()))
        .unwrap_or_default()
}

/// 清单能替启发式说话的那几档（`None` = 这份清单没说，照旧走猜的那条路）
#[derive(Default, Clone)]
pub(super) struct Declared {
    pub(super) mc_version: Option<String>,
    pub(super) loader: Option<LoaderKind>,
    pub(super) loader_version: Option<String>,
    /// 清单声明的模组编号（去重后）。**空 = 这份清单没列模组**，与「列了但字节不在包里」是两件事
    pub(super) files: Vec<CfRef>,
    /// 认下来的那份清单自己在包里的物理条目名。它是格式自带的文件、不是「包里的内容」，
    /// 不带这一档就会把 `manifest.json` 摆进保留清单里让人勾（勾上等于把编号表拷进服务端包）
    pub(super) index_entry: Option<String>,
}

/// `modLoaders[].id` 的形状是 `<家族>-<版本>`：`forge-47.2.20`、`neoforge-21.1.77`、`fabric-0.15.3`。
/// 家族段**全等**比（`neoforge` 不能被子串 `forge` 抢走），认不出的家族（optifine、rift 这些）
/// 直接跳过——`LoaderKind` 只有三档，硬塞一档等于给下载器一个不存在的坐标。
/// 版本段照 Forge 候选列表的口径给**纯构建号**（CF 的 id 里本来就不带 MC 前缀）
pub(super) fn cf_loader(id: &str) -> Option<(LoaderKind, Option<String>)> {
    let (kind, ver) = id.split_once('-')?;
    let ver = Some(ver.trim().to_string()).filter(|s| !s.is_empty());
    match kind.trim().to_lowercase().as_str() {
        "fabric" | "fabric-loader" => Some((LoaderKind::Fabric, ver)),
        "neoforge" => Some((LoaderKind::NeoForge, ver)),
        "forge" => Some((LoaderKind::Forge, ver)),
        _ => None,
    }
}

/// 读出 CF 清单里那份声明；**内容不像 CF 就返回 None**（`manifest.json` 这名字太通用，
/// 一个不相干的 manifest 不该把整包从启发式那条路上带走），读文件或解 JSON 失败也一律 None：
/// 启发式那条腿还活着，没必要因为一份可选声明把整包判死
pub(super) fn cf_declared_facts(path: &Path, entry: &str) -> Result<Option<Declared>, String> {
    let zip = File::open(path).map_err(|e| format!("无法打开文件：{e}"))?;
    let mut archive = zip::ZipArchive::new(zip).map_err(|e| format!("zip 结构损坏：{e}"))?;
    let Ok(mut f) = archive.by_name(entry) else {
        return Ok(None);
    };
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Ok(None);
    }
    let Ok(m) = serde_json::from_slice::<CfManifest>(&buf) else {
        return Ok(None);
    };
    let mc_version = Some(m.minecraft.version.trim().to_string()).filter(|s| !s.is_empty());
    // 多个 modLoaders 条目时取**第一个认得出的家族**（CF 的导出顺序把主加载器放前面）
    let loader = m
        .minecraft
        .mod_loaders
        .iter()
        .find_map(|l| cf_loader(&l.id));
    // `files[]` 去重：同一对编号重复列过（手工改过的清单真有这种），一行编号 = 方案一行，
    // 留重复等于同一个 jar 下两遍
    let mut files: Vec<CfRef> = Vec::new();
    for f in &m.files {
        let mod_id = cf_id(&f.project_id);
        let file_id = cf_id(&f.file_id);
        if mod_id.is_empty() || file_id.is_empty() {
            continue;
        }
        let r = CfRef { mod_id, file_id, required: f.required, link: CfLink::Unknown, env: None };
        // 去重按**坐标那一对 id**，不按整个结构体：`required` 在同一对编号的两条声明里
        // 给得不一样是真有的事（手改过清单），按整结构体比就会留下两行、同一个 jar 下两遍
        if !files.iter().any(|x| x.mod_id == r.mod_id && x.file_id == r.file_id) {
            files.push(r);
        }
    }
    // 三段全空才叫「内容不像 CF」：只列模组、不写 minecraft 的清单仍然是 CF 形状的证据；
    // 同名太通用，三段全空的那份 manifest.json 不该把整包从启发式那条路上带走
    if mc_version.is_none() && loader.is_none() && files.is_empty() {
        return Ok(None);
    }
    Ok(Some(Declared {
        mc_version,
        loader: loader.as_ref().map(|(k, _)| *k),
        loader_version: loader.and_then(|(_, v)| v),
        files,
        index_entry: Some(entry.to_string()),
    }))
}

/// 按声明的编号造方案行。**这一档没有字节**：路径/名字只是锚点（`split_mod_file` 从
/// `{mod_id}-{file_id}` 切出 `id`/`version`，`detector` 的 `src_path` 也按它精确回指），
/// 真名、大小、sha1、直链由 `core::cfpack` 联网补；补不到就停在「未就位」，不假装在包里
pub(super) fn cf_rows(files: &[CfRef]) -> Vec<PackFile> {
    files
        .iter()
        .map(|r| {
            let file_name = format!("{}-{}.jar", r.mod_id, r.file_id);
            PackFile {
                path: format!("mods/{file_name}"),
                file_name,
                url: String::new(),
                sha1: None,
                size_bytes: 0,
                in_pack: false,
                env_server: None,
                env_client: None,
                depends: Vec::new(),
                cf: Some(r.clone()),
            }
        })
        .collect()
}


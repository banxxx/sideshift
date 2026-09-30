use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct ConversionOptions {
    pub mc_version: String,
    pub loader_version: String,
    /// 本次的 **Java 需求线**（由 MC 版本推的那档，"17"）：只当筛子用，不是"要装哪版"。
    /// 跑 installer 用哪一枚由 `java_path` 决定；这一档进报告与回看，语义始终是"包要什么"。
    pub java_version: String,
    /// 手选跑 installer 的那枚 JDK 绝对路径；空 = 自动（本机第一枚够格的）。
    /// 换机/卸掉之后这一枚可能不在本机了：`core::java::probe` 认不到就退回自动，不会把转换钉死。
    pub java_path: String,
    pub memory_mb: u32,
    pub generate_scripts: bool,
    pub nogui: bool,
    /// 自动写入 `eula=true`（**默认开**：关掉做出的包首次一律拒启，那是"默认给用户一个跑不起来的包"）
    pub agree_eula: bool,
    /* ---- 服务端设置（server.properties 高频字段） ---- */
    pub server_port: u16,
    pub motd: String,
    pub max_players: u32,
    /// survival | creative | adventure | spectator
    pub gamemode: String,
    /// peaceful | easy | normal | hard
    pub difficulty: String,
    pub online_mode: bool,
    pub level_seed: String,
    /* ---- 启动参数扩展 ---- */
    /// Aikar's flags：G1GC 调优参数组，拼入 start 脚本 JVM 参数（**默认开**：官方推荐档，
    /// 且只在 start 脚本里出现，用户手改 `-Xmx` 之外不会碰到它）
    pub use_aikar_flags: bool,
    /// 用户附加 JVM 参数（原样拼接）
    pub extra_jvm_args: String,
    /* ---- 单包输出覆写 ---- */
    /// 本次转换输出目录；空 = 用全局设置
    pub output_override: String,
    /* ---- 客户端保留内容 ---- */
    /// 要带入服务端的包内目录：相对路径（任意层级，如 kubejs/client_scripts），按 `{path}/` 前缀匹配。
    ///
    /// 落位是「勾哪一层就剪掉上层」：`kubejs/client_scripts` 落在产物根 `client_scripts/`，
    /// 它内部的层级原样保留（见 `parser::kept_rel`）
    pub keep_dirs: Vec<String>,
    /// 要带入服务端的包内**文件**（任意层级，如 `options.txt`、`kubejs/startup.js`）：
    /// 按逻辑相对路径**精确全等**匹配，落位同样剪到产物根（`startup.js`）。
    ///
    /// 与 `keep_dirs` 分成两个字段而不是混进一个字符串数组：拷贝/预估那两处要按条目形状分叉
    /// （目录走 `{path}/` 前缀、文件走全等），混在一起前端就得猜"这条到底是不是文件"。
    /// 落位名（最后一段）在两档之间唯一，由勾选弹窗当场保证——同名会撞到同一个包根位置。
    pub keep_files: Vec<String>,
    /* ---- 本机安装 Loader ---- */
    /// 本次转换是否在本机跑 loader installer（Forge / NeoForge 产物「上传即跑」的前提）。
    ///
    /// **显式 bool，不引入 `Option`/第三态**：建包时 `default_options()` 从全局取初值，用户改过就存自己那份。
    /// 于是重试与任务快照永远按快照走——不存在"跟随全局"那种会随设置漂移的语义（同一份方案隔几天
    /// 重跑做出不一样的包，比包本身有问题更难查）。老存档缺这个字段走容器级 `serde(default)` = `Default::default()`。
    pub install_loader_locally: bool,
    /// 允许跳过「拿不到字节」的 CF 模组继续构建。**默认关**：关着时必选模组缺件直接把
    /// 「开始转换」拦住——少一枚 jar 的包在服上多半起不来，而这正是用户没打算做的包。
    /// 开了它=用户看过缺件清单并同意，缺的那些会逐条写进报告与 README，不是静默丢
    pub allow_missing_mods: bool,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            mc_version: String::new(),
            loader_version: String::new(),
            java_version: String::new(),
            java_path: String::new(),
            memory_mb: 4096,
            generate_scripts: true,
            nogui: true,
            agree_eula: true,
            server_port: 25565,
            motd: "A Minecraft server".into(),
            max_players: 20,
            gamemode: "survival".into(),
            difficulty: "easy".into(),
            online_mode: true,
            level_seed: String::new(),
            use_aikar_flags: true,
            extra_jvm_args: String::new(),
            output_override: String::new(),
            keep_dirs: Vec::new(),
            keep_files: Vec::new(),
            install_loader_locally: true,
            allow_missing_mods: false,
        }
    }
}

/// 模板收的那 16 档（前端 `src/lib/types/template.ts` 的 `TemplateFieldKey` 同一份名单）。
///
/// **`Option` + 缺席即不写入**：一条字段没进模板时，这个结构里压根没有那个键（`skip_serializing_if`），
/// 套用就是"只写这里有的那些"。不另存一份「勾了哪些」的平行数组——那份数组和值表一旦分叉，
/// 界面显示勾中而套用时不写（或反过来）都查不出来。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct TemplateValues {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_loader_locally: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_scripts: Option<bool>,
    /// 手选那枚 JDK 的绝对路径；`Some("")` = 明确套用「自动选择」，与"这档不在模板里"是两回事
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_path: Option<String>,
    /// 空串 = 明确套用「回落全局设置」，同上
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_override: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nogui: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agree_eula: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_aikar_flags: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_jvm_args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gamemode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_players: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level_seed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_mode: Option<bool>,
}

impl TemplateValues {
    /// 从一份完整选项里抄出模板能收的那 16 档（全部视为「在模板里」）。
    ///
    /// 唯一的消费者是 `template_defaults`：新建模板时界面要显示的初值必须和
    /// `ConversionOptions::default()` 是同一份事实，否则前端抄一份字面量，改默认就得出两处。
    pub fn seeded_from(o: &ConversionOptions) -> Self {
        Self {
            install_loader_locally: Some(o.install_loader_locally),
            generate_scripts: Some(o.generate_scripts),
            java_path: Some(o.java_path.clone()),
            output_override: Some(o.output_override.clone()),
            memory_mb: Some(o.memory_mb),
            nogui: Some(o.nogui),
            agree_eula: Some(o.agree_eula),
            use_aikar_flags: Some(o.use_aikar_flags),
            extra_jvm_args: Some(o.extra_jvm_args.clone()),
            gamemode: Some(o.gamemode.clone()),
            difficulty: Some(o.difficulty.clone()),
            server_port: Some(o.server_port),
            max_players: Some(o.max_players),
            motd: Some(o.motd.clone()),
            level_seed: Some(o.level_seed.clone()),
            online_mode: Some(o.online_mode),
        }
    }
}

/// 一份转换模板（`templates.json` 里的一条；顺序按数组下标，界面可拖）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ConversionTemplate {
    /// 稳定 id（前端生成，形如 `tpl-xxxxxxxxxxxx`）：转换页的「已套用」认的是它，改名不影响
    pub id: String,
    pub name: String,
    /// 备注：只在列表卡与下拉的第二行露个脸，**不参与套用**
    pub note: String,
    pub values: TemplateValues,
    /// 最后保存的时刻（epoch 毫秒）
    pub updated_at: u64,
}

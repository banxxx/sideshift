//! 跨进程数据模型：字段名/取值必须与 src/lib/types.ts 契约逐一对齐。
//! serde 约定：结构体 camelCase，枚举小写；可选字段 Option + skip_serializing_if。

mod java;
mod mods;
mod options;
mod pack;
mod plan;
mod settings;
mod task;

pub use java::*;
pub use mods::*;
pub use options::*;
pub use pack::*;
pub use plan::*;
pub use settings::*;
pub use task::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_native_separators() {
        let home = std::path::Path::new(if cfg!(windows) {
            "C:\\Users\\ban"
        } else {
            "/home/ban"
        });
        let s = AppSettings::defaults_in(&crate::core::data_root::suggested_root(home, home));
        assert!(!s.output_dir.contains('/'), "输出目录残留正斜杠: {}", s.output_dir);
        assert!(!s.cache_dir.contains('/'), "缓存目录残留正斜杠: {}", s.cache_dir);
        // 数据根与 home 回落共用同一套布局（引导页档位与默认值必须对得上）
        let in_home = AppSettings::defaults_in(&home.join("SideShift"));
        assert!(in_home.output_dir.ends_with("output"));
        assert!(in_home.cache_dir.ends_with("cache"));
        assert_eq!(
            AppSettings {
                output_dir: "D:/mc/out".into(),
                ..Default::default()
            }
            .normalized()
            .output_dir,
            if cfg!(windows) { "D:\\mc\\out" } else { "D:/mc/out" }
        );
    }

    /// 老 settings.json 里没有 updateChannel：必须补成 `None`（= 跟随当前构建），
    /// 不能因为多一个字段就把用户整份设置丢掉；档位值写坏了也一样只能归位，不能连带失败。
    #[test]
    fn settings_without_update_channel_still_load() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert_eq!(s.update_channel, None);

        let broken = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"updateChannel":"ntfs"}"#;
        let s: AppSettings = serde_json::from_str(broken).expect("无效渠道值不该拖垮整份设置");
        assert_eq!(s.normalized().update_channel, Some(UpdateChannel::Stable));
    }

    /// 老设置里没有这个字段：必须能加载（缺 Key = CurseForge 侧整块不可用，不是错误）
    #[test]
    fn settings_without_curseforge_key_still_load() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert_eq!(s.curseforge_api_key, None);
    }

    /// 旧 settings.json / 旧任务存档都没有装 Loader 那三颗开关：必须加载成功，且默认值不能反过来——
    /// 本机安装与复用都默认**开**（一次装好的服务端 100–160 MB，不该让老用户从此每次重下重装；
    /// 而「产物上传即开服」这条主路径也不该对老用户隐身）。关掉才等价于旧产物那份行为。
    #[test]
    fn loader_switch_defaults_hold_for_legacy_payloads() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert!(s.install_loader_locally, "本机安装默认必须开");
        assert!(s.reuse_loader_installs, "复用默认必须开");

        // 任务快照里的方案同理：缺字段 = 没表过态，跟默认档一起开，但不会跟着全局设置的当前值漂移
        let opts: ConversionOptions =
            serde_json::from_str(r#"{"mcVersion":"1.20.1","loaderVersion":"47.4.10"}"#)
                .expect("旧方案存档应能加载");
        assert!(opts.install_loader_locally);
        assert_eq!(opts.memory_mb, 4096, "其余字段走 Default，别悄悄改了老任务的档位");
    }

    /// 老设置里完全没有语言这一档（i18n 之前写的 settings.json）：必须加载成功并落 `Auto`，
    /// 也就是"跟随系统"。档位值写错（手改、或未来版本加了新档又被老程序读）只能归位成 Auto，
    /// 不能把整份设置连带丢掉——`persist::load_settings` 的回落粒度是整份文件。
    #[test]
    fn settings_without_locale_still_load_and_bad_value_normalizes() {
        let legacy = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"autoClassifyOnline":true}"#;
        let s: AppSettings = serde_json::from_str(legacy).expect("旧设置应能加载");
        assert_eq!(s.locale, AppLocale::Auto, "缺字段必须落跟随系统");

        let pinned: AppSettings = serde_json::from_str(&format!(
            r#"{{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"locale":"zhTw"}}"#
        ))
        .expect("显式语言档位应能加载");
        assert_eq!(pinned.locale, AppLocale::ZhTw);

        let broken = r#"{"outputDir":"o","cacheDir":"c","stripClientOnly":true,"verifyAfterBuild":false,
            "downloadSource":"official","concurrency":6,"locale":"zh_CN"}"#;
        let s: AppSettings = serde_json::from_str(broken).expect("无效语言值不该拖垮整份设置");
        assert_eq!(s.normalized().locale, AppLocale::Auto);
    }

    /// 粘贴进来的 Key 常带空白：带着空格发出去只会收到一条读不懂的 403，所以读写两端都归位；
    /// 全空串等于「没配」，界面才不会再显示一个空输入框当成已配置
    #[test]
    fn curseforge_key_trims_and_blank_becomes_none() {
        let s = AppSettings {
            curseforge_api_key: Some("  $23-abc:def  ".into()),
            ..Default::default()
        }
        .normalized();
        assert_eq!(s.curseforge_api_key.as_deref(), Some("$23-abc:def"));

        for blank in ["", "   ", "\t\n"] {
            let s = AppSettings {
                curseforge_api_key: Some(blank.into()),
                ..Default::default()
            }
            .normalized();
            assert_eq!(s.curseforge_api_key, None, "{blank:?} 应归位为未配置");
        }
    }

    /// 一次性迁移只能命中「我们自己写进去的旧默认」：用户挑过/手打的路径差一个字符都不能动，
    /// 否则就是把别人的服务器目录搬走了。
    #[test]
    fn unstick_legacy_rewrites_only_the_old_default() {
        assert_eq!(
            unstick_legacy("C:\\Users\\ban\\SideShift\\output", "C:\\Users\\ban\\SideShift\\output", "E:\\SideShift\\output"),
            "E:\\SideShift\\output"
        );
        for kept in [
            "D:\\mc\\out",                                   // 用户自己挑的盘
            "C:\\Users\\ban\\SideShift\\output\\",           // 旧默认多个分隔符
            "C:\\Users\\ban\\.minecraft\\downloads",         // 另一个已有目录
            "",                                              // 空字段由 load_settings 回落，不归这里管
        ] {
            assert_eq!(
                unstick_legacy(kept, "C:\\Users\\ban\\SideShift\\output", "E:\\SideShift\\output"),
                kept
            );
        }
    }

    /// 方案快照三段往返（下发前端 → 回传 start_conversion → 落盘回灌）必须留住端证据字段：
    /// 端标签不落盘、由 `client_side`/`server_side` 在渲染期反推，这里掉一个字段，
    /// 任务详情的「方案」签就会把整批模组错标成「未判定」。
    #[test]
    fn plan_roundtrip_keeps_env_evidence() {
        use std::collections::HashMap;
        let row = PlanMod {
            id: "sodium-fabric".into(),
            name: "Sodium".into(),
            version: "0.5.8".into(),
            loader: Some("Fabric".into()),
            disposition: ModDisposition::Keep,
            client_only: false,
            needs_review: false,
            auto_supplement: false,
            size_bytes: 1234,
            needs_download: false,
            local_path: None,
            pinned: None,
            depends: vec!["api".into()],
            src_path: Some("deps/mods/sodium.jar".into()),
            env_source: EnvSource::JarMetadata,
            env_conflict: true,
            client_side: Some(SideFlag::Required),
            server_side: Some(SideFlag::Optional),
            bytecode_hint: Some(BytecodeHint::ServerCode),
            cf_blocked: true,
            cf_required: false,
        };
        let to_frontend: Vec<PlanMod> =
            serde_json::from_value(serde_json::to_value([&row]).unwrap()).unwrap();
        let disk = serde_json::to_value(HashMap::from([("t1".to_string(), to_frontend)])).unwrap();
        let revived: HashMap<String, Vec<PlanMod>> = serde_json::from_value(disk).unwrap();
        let r = &revived["t1"][0];
        assert_eq!(r.client_side, Some(SideFlag::Required));
        assert_eq!(r.server_side, Some(SideFlag::Optional));
        assert_eq!(r.env_source, EnvSource::JarMetadata);
        assert!(r.env_conflict);
        assert_eq!(r.bytecode_hint, Some(BytecodeHint::ServerCode));
        assert_eq!(r.src_path.as_deref(), Some("deps/mods/sodium.jar"));
        assert_eq!(r.depends, vec!["api".to_string()]);
    }

    /// 报告新增字段必须能吃下旧存档：tasks.json 里的历史报告没有这些键，
    /// 一旦反序列化失败整个存档都会被当作损坏丢掉（用户看到的是「任务全没了」）
    #[test]
    fn legacy_report_json_still_loads_with_defaults() {
        let mut v = serde_json::to_value(ConversionReport {
            task_id: "t1".into(),
            output_file_name: "a-server.zip".into(),
            output_size_bytes: 1024,
            duration_sec: 30,
            removed: 3,
            kept: 9,
            added: 1,
            pending_review: vec!["X".into()],
            options: ConversionOptions::default(),
            file_count: 42,
            generated_files: vec!["start.bat".into()],
            reused_root_files: vec!["eula.txt".into()],
            start_jar: Some("fabric-server-launch.jar".into()),
            installed: true,
            checks: Vec::new(),
            skipped_mods: vec!["X".into()],
        })
        .unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("fileCount");
        obj.remove("generatedFiles");
        obj.remove("reusedRootFiles");
        obj.remove("startJar");
        obj.remove("installed");
        obj.remove("checks");
        obj.remove("skippedMods");
        let old: ConversionReport = serde_json::from_value(v).unwrap();
        assert_eq!((old.file_count, old.start_jar), (0, None));
        assert!(old.generated_files.is_empty());
        // 老存档没有撞车这一说：那半天还没写出来过
        assert!(old.reused_root_files.is_empty());
        // 旧存档没这一键 ⇒ 没本机装过（那条链路当时还不存在）
        assert!(!old.installed);
        assert!(old.checks.is_empty());
        // 旧存档也没「缺件」这一说：那半天闸门还不存在 ⇒ 没跳过过任何模组
        assert!(old.skipped_mods.is_empty());
        assert!(!old.options.allow_missing_mods, "旧档缺键必须按不放行，不能默认跳过缺件");
    }
}

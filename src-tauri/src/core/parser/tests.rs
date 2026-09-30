use super::scan::{detect_mods_prefix, first_version_like, loader_from_jar_name, ModsLayout};
use super::*;

    use std::io::Write;

    /// 构造一个最小 mrpack：MC 版本只在 dependencies 里，
    /// 含 1 个下载条目 + 1 个包内自带（local）模组 + 1 个自带配置文件
    fn write_fake_mrpack() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.mrpack",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file(MRPACK_ENTRY, opts).unwrap();
        write!(
            w,
            r#"{{
  "game": "minecraft",
  "dependencies": {{ "minecraft": "1.21.1", "fabric-loader": "0.16.9" }},
  "files": [
    {{ "path": "mods/dl.jar", "hashes": {{}}, "downloads": ["https://x/dl.jar"] }},
    {{ "path": "mods/local.jar", "hashes": {{}}, "downloads": [], "fileSize": 4 }},
    {{ "path": "config/x.toml", "hashes": {{}}, "downloads": [], "fileSize": 2 }}
  ]
}}"#
        )
        .unwrap();
        w.start_file("mods/local.jar", opts).unwrap();
        w.write_all(b"junk").unwrap();
        w.start_file("config/x.toml", opts).unwrap();
        w.write_all(b"x=1").unwrap();
        // 未声明文件：手动拖进 zip 的 kubejs 脚本（index.files 里没有这个条目）
        w.start_file("kubejs/client_scripts/demo.js", opts).unwrap();
        w.write_all(b"console.info('hi')").unwrap();
        w.finish().unwrap();
        path
    }

    #[test]
    fn mrpack_mc_version_from_dependencies_not_game() {
        let path = write_fake_mrpack();
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.21.1");
        assert_eq!(parsed.loader_version.as_deref(), Some("0.16.9"));
    }

    #[test]
    fn mrpack_without_env_declares_no_side_flag() {
        // 上面的 fixture 全程没写 env 段：真实包里这是常态。
        // 若这里补成 Optional，下游会误当「作者已声明」，证据阶梯后几层全部失效。
        let path = write_fake_mrpack();
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        for f in &parsed.mod_files {
            assert!(
                f.env_client.is_none() && f.env_server.is_none(),
                "{} 无 env 声明时不应有端标志",
                f.path
            );
        }
    }

    #[test]
    fn mrpack_counts_local_and_downloaded_mods() {
        let path = write_fake_mrpack();
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.mod_count, 2); // dl + local 都算
        assert_eq!(parsed.mod_files.len(), 2);
        // 自带文件：url 为空（流水线据此走 Fetch::ZipEntry 从源包抽取）
        assert!(
            parsed
                .mod_files
                .iter()
                .find(|f| f.path == "mods/local.jar")
                .unwrap()
                .url
                .is_empty()
        );
        assert!(!parsed
            .mod_files
            .iter()
            .find(|f| f.path == "mods/dl.jar")
            .unwrap()
            .url
            .is_empty());
        // 非 mods 的自带文件归入 extra_files；另含 1 个未声明的 kubejs 物理文件
        assert_eq!(parsed.extra_files.len(), 2);
        assert_eq!(parsed.extra_files[0].path, "config/x.toml");
        let undeclared = parsed
            .extra_files
            .iter()
            .find(|f| f.path == "kubejs/client_scripts/demo.js")
            .expect("未声明的 zip 文件应被补收");
        assert!(undeclared.url.is_empty()); // 构建时走 ZipEntry 直接从源包抽取
    }

    /// 保留范围硬闸：判据是「路径里任一段」，所以 `config/mods` 也拦——剪层落位之后它就是包根 `mods/`。
    /// 段全等，`mods_x` 不是 mods（裸 zip 的 mods 定位与收录侧两条腿已按同一口径收口，见
    /// `mods_dir_matches_the_whole_segment`）
    #[test]
    fn keep_gate_covers_top_dirs_on_both_sides() {
        assert!(keep_denied("mods"));
        assert!(keep_denied("MODS/Some.jar"));
        assert!(keep_denied("resourcepacks/x.zip"));
        assert!(keep_denied(logical_rel("overrides/ResourcePacks/x.zip")));
        assert!(!keep_denied("config"));
        assert!(!keep_denied("config/jei/jei.ini"));
        assert!(keep_denied("config/mods"));
        assert!(keep_denied("MyPack/mods/fabric/a.jar"));
        assert!(!keep_denied("mods_backup/a.cfg"));
        assert_eq!(base_name("kubejs/client_scripts"), "client_scripts");
        assert_eq!(base_name("options.txt"), "options.txt");
    }

    /// 勾哪一层就落哪一层：内部层级保留，祖先剪掉；顶层勾选恒等（旧勾选值不受影响）
    #[test]
    fn kept_rel_lands_the_picked_level_at_root() {
        assert_eq!(kept_rel("config", "config/jei/jei.ini"), "config/jei/jei.ini");
        assert_eq!(kept_rel("config/jei", "config/jei/jei.ini"), "jei/jei.ini");
        assert_eq!(
            kept_rel("kubejs/client_scripts", "kubejs/client_scripts/demo.js"),
            "client_scripts/demo.js"
        );
        assert_eq!(kept_rel("kubejs/startup.js", "kubejs/startup.js"), "startup.js");
        // 落位名跟条目自己的大小写，不跟小写勾选键
        assert_eq!(kept_rel("config/jei", "Config/JEI/jei.ini"), "JEI/jei.ini");
        // 勾选键比条目还深（匹配逻辑出错才会出现）⇒ 无从落位，返回空串让调用方跳过，不能落到包根
        assert_eq!(kept_rel("config/jei/jei.ini/deep", "config/jei/jei.ini"), "");
    }

    /// 裸 zip 外面套一层自定义文件夹：mods 那条腿早就按 `MyPack/mods/` 探测了，保留内容这条腿
    /// 必须剥同一层——否则勾选值与落位都带着 `MyPack/`，而服务端按实例根读 `config/`
    #[test]
    fn bare_zip_wrapper_folder_leaves_logical_paths() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("MyPack/mods/a.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        // mods 下的非 jar 会落进 extra_files（mods 判据只看 jar），靠段名闸拦住
        w.start_file("MyPack/mods/README.md", opts).unwrap();
        w.write_all(b"r").unwrap();
        w.start_file("MyPack/config/jei/jei.ini", opts).unwrap();
        w.write_all(b"x").unwrap();
        w.start_file("MyPack/options.txt", opts).unwrap();
        w.write_all(b"y").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        assert_eq!(parsed.root_prefix, "MyPack/");
        assert_eq!(parsed.extra_files.len(), 3);
        // 物理路径留着（取件按它从 zip 里抽字节），逻辑路径才是交付包里的位置
        assert_eq!(parsed.extra_files[0].path, "MyPack/config/jei/jei.ini");
        assert_eq!(parsed.logical_rel("MyPack/config/jei/jei.ini"), "config/jei/jei.ini");
        assert_eq!(parsed.logical_rel("MyPack/options.txt"), "options.txt");
        // 前缀是从某一条 jar 探出来的，别的条目大小写不同也要剥
        assert_eq!(parsed.logical_rel("mypack/Config/a.toml"), "Config/a.toml");
        // 不在这层文件夹下的路径不动它
        assert_eq!(parsed.logical_rel("other/x.cfg"), "other/x.cfg");
        // mods 下的非 jar 会落进 extra_files（jar 才归模组那条腿）：剥完前缀正好撞上段名闸，
        // 不剥的话它叫 `mypack/mods/…`——段名闸照样拦得住，两道保险叠着，落位剪层也不给后门
        let mut denied: Vec<String> = Vec::new();
        for f in &parsed.extra_files {
            if keep_denied(parsed.logical_rel(&f.path)) {
                denied.push(f.path.clone());
            }
        }
        assert_eq!(denied, vec!["MyPack/mods/README.md".to_string()]);
    }

    /// 猜不到 MC 版本 ⇒ 空串。旧的 1.20.1 兜底把「没线索」演成「这一档就是 1.20.1」，
    /// 于是 Java 需求线（17）、Loader 候选、模组反查全按一个凭空造的版本跑；空值才是实话，
    /// 界面据此显示「未识别」、开始转换那道闸据此停住。
    #[test]
    fn bare_zip_without_any_version_hint_parses_as_empty_mc_version() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/a.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("config/x.cfg", opts).unwrap();
        w.write_all(b"y").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "");
        // 空版本不该被下游当成某一档：兜底表对认不出的串给 17，那是「不知道要哪档」而不是「这就是 17」
        assert_eq!(crate::core::java::required_for_mc(""), "17");
    }

    /// 有线索时照旧猜：条目名里带 `minecraft`/`version.json`，或包文件名本身带版本号
    #[test]
    fn bare_zip_still_reads_version_from_entry_names_and_file_name() {
        assert_eq!(
            first_version_like("minecraft-version-1.21.1.json"),
            Some("1.21.1".to_string())
        );
        // 裸包名（快照那种没有 `1.` 前缀的年份号）认不出来 ⇒ 空，而不是硬凑
        assert_eq!(first_version_like("24w14a"), None);
    }

    /// mods 定位改段全等：旧写法拿子串找 `mods/`，`somemods/a.jar` 会被当成模组目录，
    /// 整包的模组与保留内容从此按一个不存在的目录分家（jar 全数进模组清单、真 `mods/` 反而没人认）
    #[test]
    fn mods_dir_matches_the_whole_segment() {
        let e = |names: &[&str]| -> Vec<(String, u64)> {
            names.iter().map(|n| (n.to_string(), 1u64)).collect()
        };
        let prefix = |names: &[&str]| -> Option<String> {
            match detect_mods_prefix(&e(names)) {
                Some(ModsLayout::Prefix(p)) => Some(p),
                Some(ModsLayout::Root) => Some("<root>".into()),
                None => None,
            }
        };
        assert_eq!(prefix(&["mods/a.jar"]).as_deref(), Some("mods/"));
        // 大小写不敏感，前缀按原样条目名切（`in_mods` 拿它跟原始路径比）
        assert_eq!(prefix(&["MyPack/MODS/a.jar"]).as_deref(), Some("MyPack/MODS/"));
        assert_eq!(prefix(&["a.jar"]).as_deref(), Some("<root>"));
        // 不是 mods 的那些：子串命中、名字相近、jar 不在 mods 那一层里
        assert_eq!(prefix(&["somemods/a.jar"]), None);
        assert_eq!(prefix(&["mods_backup/a.jar", "config/x.cfg"]), None);
        assert_eq!(prefix(&["mods/nested/a.jar"]), None);
        // 两样都在时，位置由真 `mods/` 那枚定，相近名不抢位
        assert_eq!(prefix(&["somemods/a.jar", "mods/b.jar"]).as_deref(), Some("mods/"));
    }

    /// 段全等同时收口了「收录侧」那条 `starts_with("mods")`：`mods_backup/` 不再被当作模组目录丢掉，
    /// 它是「用户要不要留下的内容」；而 `mods/` 本身照旧不进保留清单（那里有 `keep_denied` 第二道闸）
    #[test]
    fn mods_like_dirs_are_not_the_mods_dir() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/a.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("mods/README.md", opts).unwrap();
        w.write_all(b"r").unwrap();
        w.start_file("mods_backup/x.cfg", opts).unwrap();
        w.write_all(b"c").unwrap();
        w.start_file("somemods/b.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        // 模组清单只认 `mods/` 那一层
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(parsed.mod_files[0].path, "mods/a.jar");
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["mods_backup/x.cfg", "somemods/b.jar"]);
    }

    /// 造一个「内容层级就是 mrpack 那一套、扩展名却是 .zip」的包（民间打包工具与手动改名都这样）。
    /// `index_at` = 清单在包里的条目名；其余内容按清单所在那一层摆（套壳包整棵都在壳里，正经包在根）
    fn write_mrpack_shape_zip(index_at: &str) -> std::path::PathBuf {
        let shell = index_at
            .rsplit_once('/')
            .map(|(d, _)| format!("{d}/"))
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file(index_at, opts).unwrap();
        write!(
            w,
            r#"{{
  "game": "minecraft",
  "dependencies": {{ "minecraft": "1.21.1", "fabric-loader": "0.16.9" }},
  "files": [
    {{ "path": "mods/dl.jar", "hashes": {{}}, "downloads": ["https://x/dl.jar"] }},
    {{ "path": "mods/local.jar", "hashes": {{}}, "downloads": [], "fileSize": 4 }},
    {{ "path": "config/x.toml", "hashes": {{}}, "downloads": [], "fileSize": 2 }}
  ]
}}"#
        )
        .unwrap();
        // 声明过的条目：物理名 = 外层壳 + 声明路径
        w.start_file(format!("{shell}mods/local.jar"), opts).unwrap();
        w.write_all(b"junk").unwrap();
        w.start_file(format!("{shell}config/x.toml"), opts).unwrap();
        w.write_all(b"x=1").unwrap();
        // 没写进清单的物理文件（手动塞的脚本）
        w.start_file(format!("{shell}kubejs/client_scripts/demo.js"), opts)
            .unwrap();
        w.write_all(b"console.info('hi')").unwrap();
        w.finish().unwrap();
        path
    }

    /// 内容优先分派：`.zip` 里有 `modrinth.index.json` 就走 mrpack 那条腿。
    /// 这条是本轮的主案——以前按扩展名分派，改名包掉进启发式，版本/加载器/清单字段全丢
    #[test]
    fn zip_containing_mrpack_index_parses_as_mrpack() {
        let path = write_mrpack_shape_zip(MRPACK_ENTRY);
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.21.1");
        assert_eq!(parsed.loader_version.as_deref(), Some("0.16.9"));
        assert_eq!(parsed.manifest.loader, LoaderKind::Fabric);
        // 声明的两条 jar 都算模组（`dl` 走 URL、`local` 走包内字节），不再靠文件名猜
        assert_eq!(parsed.mod_files.len(), 2);
        assert_eq!(parsed.root_prefix, "");
        // 清单自己不算保留内容；声明的 config + 未声明的 kubejs 各一条
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["config/x.toml", "kubejs/client_scripts/demo.js"]);
    }

    /// 套了一层外层文件夹的改名包：清单照读，物理名带壳（取件按它抽字节），逻辑路径不带壳
    #[test]
    fn wrapped_mrpack_zip_strips_the_outer_folder() {
        let path = write_mrpack_shape_zip("MyPack/modrinth.index.json");
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.21.1");
        assert_eq!(parsed.root_prefix, "MyPack/");
        // 带壳的物理名要能对上号：`in_pack` 判错会让包内字节白躺着、转头去联网下载
        let local = parsed
            .mod_files
            .iter()
            .find(|f| f.file_name == "local.jar")
            .expect("声明的包内模组应在清单里");
        assert_eq!(local.path, "MyPack/mods/local.jar");
        assert!(local.in_pack);
        assert_eq!(parsed.logical_rel("MyPack/mods/local.jar"), "mods/local.jar");
        assert_eq!(parsed.logical_rel("MyPack/config/x.toml"), "config/x.toml");
        // 未声明的那条也认得到（补收腿按剥壳后的路径判重量级目录，与不套壳时同一条口径）
        assert!(parsed
            .extra_files
            .iter()
            .any(|f| f.path == "MyPack/kubejs/client_scripts/demo.js"));
    }

    /// 清单只认包根与「单段外层文件夹」这两层：`overrides/` 里那份是内容不是根（Prism 警告的误判），
    /// 再深一层也不算
    #[test]
    fn mrpack_index_is_only_taken_at_root_or_one_wrapper_level() {
        let f = |names: &[&str]| -> Option<(String, String)> {
            let v: Vec<String> = names.iter().map(|s| s.to_string()).collect();
            find_manifest(&v, MRPACK_ENTRY).map(|(i, p)| (i.to_string(), p))
        };
        assert_eq!(
            f(&["modrinth.index.json"]),
            Some(("modrinth.index.json".into(), String::new()))
        );
        assert_eq!(
            f(&["MyPack/modrinth.index.json"]),
            Some(("MyPack/modrinth.index.json".into(), "MyPack/".into()))
        );
        // 条目名跟着原样大小写走（`by_name` 要按物理名精确命中），判据本身不敏感
        assert_eq!(
            f(&["MyPack/MODRINTH.INDEX.JSON"]),
            Some(("MyPack/MODRINTH.INDEX.JSON".into(), "MyPack/".into()))
        );
        assert_eq!(f(&["overrides/modrinth.index.json"]), None);
        assert_eq!(f(&["client-overrides/modrinth.index.json"]), None);
        assert_eq!(f(&["A/B/modrinth.index.json"]), None);
        assert_eq!(f(&["mods/a.jar", "config/x.toml"]), None);
        // 壳里那份不抢位：根上有了就取根上的
        assert_eq!(
            f(&["overrides/modrinth.index.json", "modrinth.index.json"])
                .map(|(i, _)| i),
            Some("modrinth.index.json".to_string())
        );
    }

    /// 探不到清单的 `.zip` 照旧走启发式（本轮只加分派，没动那条腿的判据）：
    /// `overrides/` 里躺着清单不算数，包根也没有版本线索 ⇒ 版本空、模组按 mods 段名收
    #[test]
    fn zip_without_a_root_index_falls_back_to_heuristics() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("overrides/modrinth.index.json", opts)
            .unwrap();
        w.write_all(b"{}").unwrap();
        w.start_file("mods/fabric-loader-0.16.9.jar", opts)
            .unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        // 启发式那条腿读不了清单，认不出版本 ⇒ 空值（mrpack 那条腿会给 1.21.1，据此分辨走了哪条腿）
        assert_eq!(parsed.manifest.mc_version, "");
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(parsed.mod_files[0].path, "mods/fabric-loader-0.16.9.jar");
    }

    /// mrpack 的 jar 常常物理躺在 `overrides/mods/` 里，而 `files[]` 写的是实例根路径。
    /// 两侧对不上号 ⇒ 包内有字节却判 in_pack=false（转头联网重下），未声明那批更会被当成普通
    /// 保留内容收下、再被 `keep_denied` 在构建层吃掉，整批模组凭空消失
    #[test]
    fn mrpack_reads_jars_under_the_overrides_shell() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.mrpack",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file(MRPACK_ENTRY, opts).unwrap();
        write!(
            w,
            r#"{{
  "game": "minecraft",
  "dependencies": {{ "minecraft": "1.20.1", "forge": "47.2.20" }},
  "files": [
    {{ "path": "mods/shell.jar", "hashes": {{}}, "downloads": [], "fileSize": 8 }},
    {{ "path": "config/x.toml", "hashes": {{}}, "downloads": [], "fileSize": 2 }}
  ]
}}"#
        )
        .unwrap();
        w.start_file("overrides/mods/shell.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        // 没写进清单的本地 jar：启动器原样装进 mods/，方案也必须收
        w.start_file("overrides/mods/handmade.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("overrides/config/x.toml", opts).unwrap();
        w.write_all(b"x=1").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        let mut mods: Vec<&str> = parsed.mod_files.iter().map(|f| f.path.as_str()).collect();
        mods.sort_unstable();
        assert_eq!(mods, vec!["overrides/mods/handmade.jar", "overrides/mods/shell.jar"]);
        // 取件按物理名从包里抽字节，所以 path 留着壳；交付位置问 logical_rel
        assert_eq!(parsed.logical_rel("overrides/mods/shell.jar"), "mods/shell.jar");
        let shell = parsed
            .mod_files
            .iter()
            .find(|f| f.file_name == "shell.jar")
            .unwrap();
        assert!(shell.in_pack, "包内有字节就该判 in_pack，不该再去联网重下");
        // 声明过的 config 只算一条（补收腿按剥壳后的路径认出它已声明）
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["overrides/config/x.toml"]);
    }

    /* ---------------- CF / MCBBS 清单这一腿 ---------------- */

    /// 官方导出的 CF 包形状：版本与加载器家族只在 `minecraft` 段里，`files[]` 只有编号
    const CF_FORGE: &str = r#"{
  "manifestType": "minecraftModpack",
  "manifestVersion": 1,
  "name": "Demo",
  "version": "1.0",
  "minecraft": { "version": "1.20.1", "modLoaders": [ { "id": "forge-47.2.20", "required": true } ] },
  "files": [ { "projectID": 1, "fileID": 2, "required": true } ],
  "overrides": "overrides"
}"#;

    /// 造一份 CF 形状的 zip：`manifest` 原样写进 `manifest_at`，其余条目按 `files` 摆
    fn write_cf_zip(manifest_at: &str, manifest: &str, files: &[&str]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file(manifest_at, opts).unwrap();
        w.write_all(manifest.as_bytes()).unwrap();
        for f in files {
            w.start_file(f, opts).unwrap();
            w.write_all(b"PK\x03\x04").unwrap();
        }
        w.finish().unwrap();
        path
    }

    /// 清单认下来了就用它说话：版本压过条目名里的那一串数字，家族压过「什么线索都没有 ⇒ Fabric」
    #[test]
    fn cf_manifest_wins_over_guessing() {
        let path = write_cf_zip(
            CF_ENTRY,
            CF_FORGE,
            &[
                "overrides/mods/minecraft-1.16.5-modkit.jar",
                "overrides/config/x.toml",
            ],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.20.1");
        assert_eq!(parsed.manifest.loader, LoaderKind::Forge);
        assert_eq!(parsed.loader_version.as_deref(), Some("47.2.20"));
        // jar 在 `overrides/mods/` 里：物理名留壳（取件按它抽字节），交付位置不带壳
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(
            parsed.mod_files[0].path,
            "overrides/mods/minecraft-1.16.5-modkit.jar"
        );
        assert_eq!(
            parsed.logical_rel("overrides/mods/minecraft-1.16.5-modkit.jar"),
            "mods/minecraft-1.16.5-modkit.jar"
        );
        assert!(parsed.mod_files[0].in_pack);
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["overrides/config/x.toml"]);
    }

    /// 清单只给家族、没给版本号时，mods 里那枚**同家族**的加载器 jar 配补上版本号
    #[test]
    fn cf_loader_jar_fills_the_version_the_manifest_left_out() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "minecraft": { "version": "", "modLoaders": [ { "id": "neoforge-" } ] } }"#,
            &["overrides/mods/neoforge-21.1.77-universal.jar"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.loader, LoaderKind::NeoForge);
        assert_eq!(parsed.loader_version.as_deref(), Some("21.1.77"));
        // 清单没写版本、条目名里也没有 `minecraft`/包文件名线索 ⇒ 照旧留空，不硬猜一档
        assert_eq!(parsed.manifest.mc_version, "");
    }

    /// 家族以清单为准：jar 文件名猜出的是另一家，它的版本号就不能配替补上去
    /// （`fabric-loader-0.16.9.jar` 的版本拿去当 forge 的版本用，等于给下载器一个不存在的坐标）
    #[test]
    fn cf_manifest_and_loader_jar_disagree_keeps_declared_version() {
        let path = write_cf_zip(
            CF_ENTRY,
            CF_FORGE,
            &["overrides/mods/fabric-loader-0.16.9.jar"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.loader, LoaderKind::Forge);
        assert_eq!(parsed.loader_version.as_deref(), Some("47.2.20"));
    }

    /// `manifest.json` 这名字太通用：内容不像 CF 那份形状就当它不存在，启发式那条腿照旧活着
    #[test]
    fn unrelated_manifest_json_does_not_capture_the_pack() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "name": "something-else", "version": "2" }"#,
            &["mods/plain.jar"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "");
        assert_eq!(parsed.manifest.loader, LoaderKind::Fabric);
        assert_eq!(parsed.mod_files.len(), 1);
    }

    /// 官方导出的 CF 包**不带 jar 字节**（`files[]` 只有 projectID/fileID）：按编号出方案行，
    /// 名字/大小/sha1/直链由 `core::cfpack` 联网补 ⇒ 解析这一层必须让它过，而不是报「没内容可转」
    #[test]
    fn cf_declared_ids_become_rows_when_the_pack_has_no_bytes() {
        let path = write_cf_zip(CF_ENTRY, CF_FORGE, &["overrides/config/x.toml"]);
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.mod_files.len(), 1);
        let row = &parsed.mod_files[0];
        // 路径只是锚点：detector 按 src_path 精确回指，split_mod_file 从它切出 id=1 / version=2
        assert_eq!(row.path, "mods/1-2.jar");
        assert_eq!(row.cf.as_ref().map(|c| (c.mod_id.as_str(), c.file_id.as_str())), Some(("1", "2")));
        assert!(!row.in_pack, "字节不在包里：这一行必须判成要联网取");
        assert_eq!(row.size_bytes, 0, "编号表里没有大小，别拿 0 之外的数当真");
        assert_eq!(parsed.manifest.mod_count, 1);
        // overrides 的内容照旧进保留清单
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(extras, vec!["overrides/config/x.toml"]);
    }

    /// 同一对编号重复列过（手改过的清单真有这种）⇒ 只出一行：一行编号就是方案一行，
    /// 留着重复等于同一个 jar 下两遍
    #[test]
    fn duplicate_cf_ids_collapse_to_one_row() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "minecraft": { "version": "1.20.1", "modLoaders": [ { "id": "forge-47.2.20" } ] },
                 "files": [ { "projectID": 1, "fileID": 2 }, { "projectID": 1, "fileID": 2 },
                            { "projectID": "1", "fileID": "3" } ] }"#,
            &["overrides/config/x.toml"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.mod_files.len(), 2, "{:?}", parsed.mod_files);
        // 字符串形式的 id 也吃（个别老包给字符串）
        assert_eq!(parsed.mod_files[1].path, "mods/1-3.jar");
    }

    /// 包里躺着 jar 时 `files[]` 不参与：编号与文件名离线对不上号，硬并等于把同一个模组列两遍
    /// （民间 CF 包十有九成是「清单 + overrides/mods 里的字节」，那一档按 P2-a 的口径走）
    #[test]
    fn cf_ids_are_ignored_when_the_pack_carries_jars() {
        let path = write_cf_zip(CF_ENTRY, CF_FORGE, &["overrides/mods/somekit.jar"]);
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.mod_files.len(), 1);
        assert_eq!(parsed.mod_files[0].file_name, "somekit.jar");
        assert!(parsed.mod_files[0].cf.is_none());
        assert!(parsed.mod_files[0].in_pack);
    }

    /// CF 清单既没声明条目、包里也没有 jar ⇒ 才是真没内容可转
    #[test]
    fn cf_pack_with_neither_ids_nor_bytes_says_so() {
        let path = write_cf_zip(
            CF_ENTRY,
            r#"{ "minecraft": { "version": "1.20.1", "modLoaders": [ { "id": "forge-47.2.20" } ] },
                 "files": [] }"#,
            &["overrides/config/x.toml"],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(!parsed.manifest.parsed);
        assert_eq!(parsed.manifest.error.as_deref(), Some(CF_NOTHING));
    }

    /// 套壳 CF 包：清单躺在外层文件夹里（`MyPack/manifest.json`），jar 在 `MyPack/overrides/mods/`。
    /// 包根按 mods 定位算出来是 `MyPack/overrides/`，清单自己落在那一层外面 ⇒ 得按物理名剔掉，
    /// 不然保留清单里会冒出一枚勾上就把编号表拷进服务端的 `manifest.json`
    #[test]
    fn wrapped_cf_pack_does_not_offer_its_own_manifest() {
        let path = write_cf_zip(
            "MyPack/manifest.json",
            CF_FORGE,
            &[
                "MyPack/overrides/mods/somekit.jar",
                "MyPack/overrides/config/x.toml",
                "MyPack/modlist.html",
            ],
        );
        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert!(parsed.manifest.parsed, "{:?}", parsed.manifest.error);
        assert_eq!(parsed.manifest.mc_version, "1.20.1");
        assert_eq!(parsed.root_prefix, "MyPack/overrides/");
        assert_eq!(
            parsed.logical_rel("MyPack/overrides/mods/somekit.jar"),
            "mods/somekit.jar"
        );
        let extras: Vec<&str> = parsed.extra_files.iter().map(|f| f.path.as_str()).collect();
        assert!(!extras.iter().any(|p| p.ends_with("manifest.json")), "{extras:?}");
        // modlist.html 是人读的模组列表、我们不解析它，所以它仍算「包里的内容」，不该被顺手剔掉
        assert!(extras.contains(&"MyPack/modlist.html"), "{extras:?}");
    }

    /// 加载器 jar 的文件名直取：三家各自的官方构建产物形状
    #[test]
    fn loader_jar_names_give_kind_and_version() {
        let v = |k: LoaderKind, s: &str| Some((k, Some(s.to_string())));
        assert_eq!(
            loader_from_jar_name("fabric-loader-0.16.9.jar"),
            v(LoaderKind::Fabric, "0.16.9")
        );
        assert_eq!(
            loader_from_jar_name("Fabric-Loader-0.14.21-1.20.1.JAR"),
            v(LoaderKind::Fabric, "0.14.21")
        );
        assert_eq!(
            loader_from_jar_name("neoforge-21.1.77-universal.jar"),
            v(LoaderKind::NeoForge, "21.1.77")
        );
        assert_eq!(
            loader_from_jar_name("forge-1.20.1-47.2.20-universal.jar"),
            v(LoaderKind::Forge, "47.2.20")
        );
        assert_eq!(
            loader_from_jar_name("forge-1.7.10-10.13.4.1614.jar"),
            v(LoaderKind::Forge, "10.13.4.1614")
        );
        // 省掉 MC 那一段的写法：单看一段就当构建号
        assert_eq!(
            loader_from_jar_name("forge-47.2.20-universal.jar"),
            v(LoaderKind::Forge, "47.2.20")
        );
        // 只有 MC 那一段（installer 那种）⇒ 宁可不给版本，也不把 1.20.1 当构建号塞进下拉
        assert_eq!(
            loader_from_jar_name("forge-1.20.1-installer.jar"),
            Some((LoaderKind::Forge, None))
        );
        // 名字里带这些字样但不是加载器本体：fabric-api 是模组库，jei 那枚是模组的 Forge 版
        assert_eq!(loader_from_jar_name("fabric-api-0.92.2.jar"), None);
        assert_eq!(loader_from_jar_name("jei-1.20.1-forge-15.2.0.jar"), None);
        assert_eq!(loader_from_jar_name("sodium.jar"), None);
    }

    /// 强证据压过弱证据：mods 里躺着 `fabric-loader-*.jar` 时，整包别处的 `forge` 字样不改判，
    /// 而且这一档的版本号不再恒为空（旧写法两个毛病都在：那条配置能把整包定成 Forge，版本只能落到列表首项）
    #[test]
    fn loader_jar_in_mods_outranks_the_substring_scan() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/fabric-loader-0.16.9.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("mods/sodium.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("config/forgeconfigmodifier.cfg", opts).unwrap();
        w.write_all(b"c").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.loader, LoaderKind::Fabric);
        assert_eq!(parsed.loader_version.as_deref(), Some("0.16.9"));
    }

    /// 没有加载器 jar 时退回旧的那档弱证据，且**不给版本号**：老 Forge（1.16 及以下）的包
    /// mods 里本来就没有那一枚，删了这档会把整批包统一误标成 Fabric
    #[test]
    fn substring_scan_still_decides_when_no_loader_jar_is_present() {
        let path = std::env::temp_dir().join(format!(
            "sideshift-parser-{}.zip",
            uuid::Uuid::new_v4()
        ));
        let mut w = zip::ZipWriter::new(File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("mods/jei.jar", opts).unwrap();
        w.write_all(b"PK\x03\x04").unwrap();
        w.start_file("config/neoforge.toml", opts).unwrap();
        w.write_all(b"t").unwrap();
        w.finish().unwrap();

        let parsed = parse(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(parsed.manifest.loader, LoaderKind::NeoForge);
        assert_eq!(parsed.loader_version, None);
    }

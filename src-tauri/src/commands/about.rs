use super::*;

/* ---------------- 关于页：鸣谢名单 ---------------- */

/// 进关于页第一下读的那份：本机快照，零网络。没有快照就回 null，界面出「不可见 + 重新获取」。
#[tauri::command]
pub fn ack_snapshot(app: AppHandle) -> Option<AckList> {
    ack::read_snapshot(&app)
}

/// 拉一次远端名单（进页后台对账，以及空态上那枚「重新获取」）。
/// 失败只递 `net:` 码或 `ack:not-configured`，界面上两者同一个处理——这一块不抢全局提示区，
/// 也不把后端的英文句子摊在鸣谢名单里。
#[tauri::command]
pub async fn ack_refresh(app: AppHandle, state: S<'_>) -> Result<AckList, String> {
    let dl = downloader_of(&state);
    let list = ack::fetch_list(&dl.client).await?;
    ack::store(&app, &list);
    Ok(list)
}

/// 名单里 Minecraft 玩家的皮肤地址（`{显示名: https 贴图地址}`）。
///
/// **永远 `Ok`**，这是定的口径不是偷懒：这一条链上任何一档失败（没网、Mojang 限流、这个人已改名）
/// 在界面上都是同一件事——那张卡回落自带头像或名字首字，所以「失败」的形态是表里少一个人，
/// 不是一句关于页里没人该看见的错误。`Result` 这层壳是 Tauri 对带引用的异步命令的硬要求。
/// 缓存与 Mojang 的两道细节见 `core::ack` 的「正版皮肤」一节。
#[tauri::command]
pub async fn ack_skins(
    app: AppHandle,
    state: S<'_>,
    names: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let dl = downloader_of(&state);
    Ok(ack::fetch_skins(&app, &dl.client, &names).await)
}

/// 读本机存的一份皮肤贴图字节（base64）。`None`＝没有副本，前端自己去打 CDN。
/// 形状上的门槛（主机、内容哈希、大小）全在 `core::ack` 里，这里不加第二套
#[tauri::command]
pub fn ack_skin_texture_get(app: AppHandle, url: String) -> Option<String> {
    ack::read_texture(&app, &url)
}

/// 把刚取到的一份贴图字节存下来。**返回值只表示"这次落没落上"，调用方不必处理**：
/// 本次皮肤已经解出来了，缓存缺失的代价是下次再打一趟 CDN
#[tauri::command]
pub fn ack_skin_texture_put(app: AppHandle, url: String, data: String) -> bool {
    ack::write_texture(&app, &url, &data)
}

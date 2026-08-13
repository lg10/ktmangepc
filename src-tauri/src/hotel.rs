//! 酒店同步 / 批量基础信息下发 / 批量授权
//!
//! 对应原 SyncHotel.axaml.cs / ChooseRoomFunction.axaml.cs / AuthBatch.axaml.cs：
//! - 同步：GET KtHotel/getDataByHotelId?hotelId=（携带登录 token），缓存 equipId → 房间 JSON
//! - 批量基础信息：按云端房间数据向已发现设备下发 0xFE12（roomNum>255 时取 %100）
//! - 批量授权：GET KtGustControlAuth/getLastKey?hotelId= 获取授权方式/截止时间，
//!   仅向已同步设备下发 0xF102（XOR + CRC16 校验）

use crate::events;
use crate::protocol;
use crate::state::AppState;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

const HOTEL_API: &str = "https://mange.kingint.com/kingint/KtHotel/getDataByHotelId";
const AUTH_API: &str = "https://mange.kingint.com/kingint/KtGustControlAuth/getLastKey";
const HOTEL_SEARCH_API: &str = "https://mange.kingint.com/kingint/KtHotel/listAllByUser";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotelRoomView {
    pub equip_id: String,
    pub room_num: i64,
    pub room_name: String,
    pub build_uid: i64,
    pub floor_uid: i64,
    pub hotel_id: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotelStateView {
    pub synced: bool,
    pub hotel_id: i64,
    pub room_count: usize,
    pub rooms: Vec<HotelRoomView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthInfoView {
    pub permanent: bool,
    /// 永久授权为远未来时间
    pub deadline: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotelSearchRecord {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotelSearchView {
    pub total: i64,
    pub current: i64,
    pub size: i64,
    pub records: Vec<HotelSearchRecord>,
}

#[derive(Default)]
struct HotelInner {
    hotel_id: i64,
    rooms: HashMap<String, Value>,
}

#[derive(Default)]
pub struct HotelService {
    inner: Mutex<HotelInner>,
    http: reqwest::Client,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn token_of(user: &Value) -> Option<String> {
    user.get("token").and_then(|t| t.as_str()).map(|s| s.to_string())
}

fn room_view(equip: &str, v: &Value) -> HotelRoomView {
    HotelRoomView {
        equip_id: equip.to_string(),
        room_num: v.get("localId").and_then(|x| x.as_i64()).unwrap_or(0),
        room_name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        build_uid: v.get("buildUid").and_then(|x| x.as_i64()).unwrap_or(0),
        floor_uid: v.get("floorUid").and_then(|x| x.as_i64()).unwrap_or(0),
        hotel_id: v.get("hotleId").and_then(|x| x.as_i64()).unwrap_or(0),
    }
}

impl HotelService {
    /// 启动时从本地库恢复同步缓存
    pub fn load(&self, db: &crate::db::DbService) {
        let rows = db
            .with_conn(|conn| {
                let mut stmt = conn.prepare("SELECT equip_id, hotel_id, data FROM hotel_room")?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap_or_default();
        if rows.is_empty() {
            return;
        }
        let mut inner = self.inner.try_lock().expect("启动期无并发");
        for (equip, hotel_id, data) in rows {
            if let Ok(v) = serde_json::from_str::<Value>(&data) {
                inner.hotel_id = hotel_id;
                inner.rooms.insert(equip, v);
            }
        }
    }

    /// 搜索酒店（原 HotelListDialog：KtHotel/listAllByUser，name 可为空查全量）
    pub async fn search(
        &self,
        state: &AppState,
        name: &str,
        page_num: i64,
        page_size: i64,
    ) -> Result<HotelSearchView, String> {
        let user = state
            .auth
            .get_login_state()
            .await
            .ok_or("未登录，无法搜索酒店")?;
        let token = token_of(&user).ok_or("登录信息缺少 token")?;

        let mut url = reqwest::Url::parse(HOTEL_SEARCH_API).map_err(|e| e.to_string())?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("pageNum", &page_num.to_string());
            q.append_pair("pageSize", &page_size.to_string());
            if !name.is_empty() {
                q.append_pair("name", name);
            }
        }

        let resp: Value = self
            .http
            .get(url)
            .header("Authorization", token)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("搜索请求失败: {e}"))?
            .json()
            .await
            .map_err(|_| "搜索响应解析失败".to_string())?;

        let ok_code = resp.get("code").and_then(|c| c.as_i64()) == Some(200000)
            || resp.get("success").and_then(|s| s.as_bool()) == Some(true);
        if !ok_code {
            let msg = resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("酒店搜索失败");
            return Err(msg.to_string());
        }
        let data = resp.get("data").filter(|d| d.is_object()).ok_or("搜索响应缺少 data")?;
        let records = data
            .get("records")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|r| HotelSearchRecord {
                        id: r.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                        name: r
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(HotelSearchView {
            total: data.get("total").and_then(|v| v.as_i64()).unwrap_or(0),
            current: data.get("current").and_then(|v| v.as_i64()).unwrap_or(page_num),
            size: data.get("size").and_then(|v| v.as_i64()).unwrap_or(page_size),
            records,
        })
    }

    /// 同步酒店房间数据
    pub async fn sync(&self, app: &AppHandle, state: &AppState, hotel_id: i64) -> Result<usize, String> {
        if hotel_id <= 0 {
            return Err("请填写需要同步的酒店 ID".into());
        }
        let user = state
            .auth
            .get_login_state()
            .await
            .ok_or("未登录，无法同步酒店数据")?;
        let token = token_of(&user).ok_or("登录信息缺少 token")?;

        let resp: Value = self
            .http
            .get(format!("{HOTEL_API}?hotelId={hotel_id}"))
            .header("Authorization", token)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("同步请求失败: {e}"))?
            .json()
            .await
            .map_err(|_| "同步响应解析失败".to_string())?;

        if resp.get("code").and_then(|c| c.as_i64()) != Some(200000) {
            let msg = resp
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("同步失败");
            return Err(msg.to_string());
        }
        let arr = resp
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or("云端未返回房间数据")?;

        let mut rooms: HashMap<String, Value> = HashMap::new();
        for item in arr {
            let equip = item
                .get("rcuUuid")
                .and_then(|x| x.as_str())
                .map(|s| s.to_uppercase())
                .unwrap_or_default();
            if equip.is_empty() {
                continue;
            }
            rooms.insert(equip, item.clone());
        }
        if rooms.is_empty() {
            return Err("该酒店下没有绑定 RCU 的房间".into());
        }

        // 落库
        let rows: Vec<(String, i64, String)> = rooms
            .iter()
            .map(|(k, v)| (k.clone(), hotel_id, v.to_string()))
            .collect();
        state.db.with_conn(move |conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch("DELETE FROM hotel_room")?;
            let mut stmt =
                tx.prepare("INSERT INTO hotel_room (equip_id, hotel_id, data) VALUES (?1,?2,?3)")?;
            for (equip, hid, data) in rows {
                stmt.execute(rusqlite::params![equip, hid, data])?;
            }
            drop(stmt);
            tx.commit()?;
            Ok(())
        })?;

        let count = rooms.len();
        {
            let mut inner = self.inner.lock().await;
            inner.hotel_id = hotel_id;
            inner.rooms = rooms;
        }
        let _ = app.emit(events::UDP_LOG, format!("酒店同步成功：{count} 个房间"));
        Ok(count)
    }

    pub async fn state_view(&self) -> HotelStateView {
        let inner = self.inner.lock().await;
        HotelStateView {
            synced: !inner.rooms.is_empty(),
            hotel_id: inner.hotel_id,
            room_count: inner.rooms.len(),
            rooms: inner.rooms.iter().map(|(k, v)| room_view(k, v)).collect(),
        }
    }

    /// 批量下发房间基础信息（原 ChooseRoomFunction.SaveButton_Click1）
    pub async fn send_base_info_batch(&self, app: &AppHandle, state: &AppState) -> Result<u32, String> {
        let rooms: HashMap<String, HotelRoomView> = {
            let inner = self.inner.lock().await;
            if inner.rooms.is_empty() {
                return Err("尚未同步酒店数据".into());
            }
            inner
                .rooms
                .iter()
                .map(|(k, v)| (k.clone(), room_view(k, v)))
                .collect()
        };
        let devices = state.udp.list_devices().await;
        let mut sent = 0u32;
        for dev in devices {
            let key = dev.equip_id.to_uppercase();
            let Some(room) = rooms.get(&key) else { continue };
            let mut room_num = room.room_num;
            if room_num > 255 {
                room_num %= 100;
            }
            match state
                .udp
                .send_base_info(
                    &dev.equip_id,
                    room.hotel_id as u32,
                    room.build_uid as u8,
                    room.floor_uid as u8,
                    room_num as u8,
                    0,
                )
                .await
            {
                Ok(()) => sent += 1,
                Err(e) => {
                    let _ = app.emit(
                        events::UDP_LOG,
                        format!("基础信息下发失败 {}: {e}", dev.equip_id),
                    );
                }
            }
        }
        let _ = app.emit(events::UDP_LOG, format!("批量基础信息下发完成：{sent} 台"));
        Ok(sent)
    }

    /// 查询酒店授权信息
    pub async fn get_auth_info(&self, hotel_id: i64) -> Result<AuthInfoView, String> {
        let resp: Value = self
            .http
            .get(format!("{AUTH_API}?hotelId={hotel_id}"))
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("授权查询失败: {e}"))?
            .json()
            .await
            .map_err(|_| "授权响应解析失败".to_string())?;
        let data = resp
            .get("data")
            .filter(|d| d.is_object())
            .ok_or("无法获取酒店授权时间")?;
        let auth = data.get("auth").and_then(|a| a.as_i64()).unwrap_or(0);
        let todate = data.get("todate").and_then(|t| t.as_i64());
        let permanent = auth == 1 || todate.is_none();
        let (deadline, text) = if permanent {
            // 2074 年，原实现 3289824000
            (3289824000u64, "永久授权".to_string())
        } else {
            let secs = (todate.unwrap_or(0) / 1000) as u64;
            let text = format!("有效期到：{}", fmt_time(secs));
            (secs, text)
        };
        Ok(AuthInfoView {
            permanent,
            deadline,
            text,
        })
    }

    /// 批量授权（原 AuthBatch.getCode + sendAuthTime），仅已同步设备
    pub async fn send_auth_batch(&self, app: &AppHandle, state: &AppState) -> Result<(u32, String), String> {
        let (hotel_id, equips) = {
            let inner = self.inner.lock().await;
            if inner.rooms.is_empty() {
                return Err("尚未同步酒店数据，无法授权".into());
            }
            (inner.hotel_id, inner.rooms.keys().cloned().collect::<Vec<_>>())
        };
        let info = self.get_auth_info(hotel_id).await?;

        let deadline = decompose(info.deadline);
        let devices = state.udp.list_devices().await;
        let mut sent = 0u32;
        for dev in devices {
            if !equips.contains(&dev.equip_id.to_uppercase()) {
                continue;
            }
            let now = decompose(unix_now());
            let random = (now.5 as u8).wrapping_add(0x17).max(1); // 简单随机 1..255
            let payload = protocol::build_auth_payload(now, info.permanent, deadline, random);
            match build_and_send(state, &dev.equip_id, &payload).await {
                Ok(()) => sent += 1,
                Err(e) => {
                    let _ = app.emit(events::UDP_LOG, format!("授权下发失败 {}: {e}", dev.equip_id));
                }
            }
        }
        let _ = app.emit(events::UDP_LOG, format!("批量授权完成：{sent} 台（{}）", info.text));
        Ok((sent, info.text))
    }
}

async fn build_and_send(state: &AppState, equip_id: &str, payload: &str) -> Result<(), String> {
    // 原实现 SendAuth 走 GetSendHeader default 分支：线上寄存器 0xFE12、regNum=100
    let packet = protocol::build_auth_packet(payload)?;
    state.udp.send_raw(equip_id, packet).await
}

/// Unix 秒 → 本地时间 (年,月,日,时,分,秒)，简化公历换算（东八区偏移）
fn decompose(secs: u64) -> (u16, u8, u8, u8, u8, u8) {
    let secs = secs + 8 * 3600; // UTC+8
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = ((rem / 3600) as u8, ((rem % 3600) / 60) as u8, (rem % 60) as u8);
    // 1970-01-01 起算
    let mut year = 1970i64;
    let mut left = days;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if left < dy {
            break;
        }
        left -= dy;
        year += 1;
    }
    let mdays: [i64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u8;
    for d in mdays {
        if left < d {
            break;
        }
        left -= d;
        month += 1;
    }
    (year as u16, month, (left + 1) as u8, h, m, s)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn fmt_time(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = decompose(secs);
    format!("{y}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

#[tauri::command]
pub async fn sync_hotel(
    app: AppHandle,
    state: State<'_, AppState>,
    hotel_id: i64,
) -> Result<usize, String> {
    state.hotel.sync(&app, &state, hotel_id).await
}

#[tauri::command]
pub async fn search_hotel(
    state: State<'_, AppState>,
    name: String,
    page_num: i64,
    page_size: i64,
) -> Result<HotelSearchView, String> {
    state.hotel.search(&state, &name, page_num, page_size).await
}

#[tauri::command]
pub async fn get_hotel_state(state: State<'_, AppState>) -> Result<HotelStateView, String> {
    Ok(state.hotel.state_view().await)
}

#[tauri::command]
pub async fn send_base_info_batch(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    state.hotel.send_base_info_batch(&app, &state).await
}

#[tauri::command]
pub async fn get_auth_info(state: State<'_, AppState>, hotel_id: i64) -> Result<AuthInfoView, String> {
    state.hotel.get_auth_info(hotel_id).await
}

#[tauri::command]
pub async fn send_auth_batch(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(u32, String), String> {
    state.hotel.send_auth_batch(&app, &state).await
}

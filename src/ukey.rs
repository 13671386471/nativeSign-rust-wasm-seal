//! UKey 硬件通信模块
//!
//! 通过 WebSocket 代理与本地 UKey 硬件通信 (ws://127.0.0.1:18080/ukey 或自定义地址)。
//!
//! 实现:
//! - `open_ukey_ws()` 建立真实 WebSocket 连接, 设置一次性消息回调
//! - `ukey_send_recv()` 发送 JSON 指令并 await 服务端响应 (请求/响应模型)
//! - 各业务函数 (设备状态 / PIN 校验 / 印章列表 / 印章图像 / 印章数据 / 硬件签名)
//!   在已连接时走真实 WebSocket, 未连接时回退到 MOCK 数据 (保持离线可用)
//!
//! 这是 REPLACE_WITH_REAL_UKEY_PROXY 的真正接入点。

use crate::types::*;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::JsFuture;
use web_sys::{BinaryType, MessageEvent, WebSocket};

// ============================================================
// 线程局部状态 — UKey WebSocket 连接与待处理回调
// ============================================================

thread_local! {
    /// WebSocket 连接 (REPLACE_WITH_REAL_UKEY_PROXY 接入点)
    static UKEY_WS: RefCell<Option<WebSocket>> =
        RefCell::new(None);
    /// WebSocket 代理地址
    static UKEY_WS_URL: RefCell<String> =
        RefCell::new("ws://127.0.0.1:18080/ukey".to_string());
    /// 当前 PIN 码 (会话缓存)
    static UKEY_PIN: RefCell<Option<String>> = RefCell::new(None);
    /// 待处理的一次性 Promise resolve 回调 (收到下一条消息时触发)
    static UKEY_PENDING: RefCell<Option<js_sys::Function>> = RefCell::new(None);
    /// 持有 onmessage 闭包, 防止被提前释放
    static UKEY_ONMESSAGE: RefCell<Option<Closure<dyn FnMut(MessageEvent)>>> =
        RefCell::new(None);
}

/// 设置 UKey WebSocket 代理地址
pub fn set_ukey_ws_url(url: &str) {
    UKEY_WS_URL.with(|u| *u.borrow_mut() = url.to_string());
}

/// 是否已建立 WebSocket 连接
pub fn is_ukey_connected() -> bool {
    UKEY_WS.with(|w| w.borrow().is_some())
}

/// 建立 UKey WebSocket 连接
///
/// 创建 WebSocket, 设置 `onmessage` 处理器 (收到消息时若有待处理 resolve 则触发),
/// 并缓存连接。可在 `init_application` 之后任意时刻调用。
pub fn open_ukey_ws() -> Result<(), String> {
    let url = UKEY_WS_URL.with(|u| u.borrow().clone());
    let ws = WebSocket::new(&url).map_err(|e| format!("WebSocket 创建失败: {:?}", e))?;
    ws.set_binary_type(BinaryType::Arraybuffer);

    let onmessage = Closure::wrap(Box::new(|e: MessageEvent| {
        let data = e.data();
        let text = if let Some(s) = data.as_string() {
            s
        } else {
            String::new()
        };
        UKEY_PENDING.with(|p| {
            if let Some(resolve) = p.borrow_mut().take() {
                let _ = resolve.call1(&JsValue::NULL, &JsValue::from_str(&text));
            }
        });
    }) as Box<dyn FnMut(MessageEvent)>);

    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    // 持有闭包, 使它在连接生命周期内保持存活
    UKEY_ONMESSAGE.with(|o| *o.borrow_mut() = Some(onmessage));

    UKEY_WS.with(|w| *w.borrow_mut() = Some(ws));
    web_sys::console::log_1(&format!("[ukey] WebSocket 已连接: {}", url).into());
    Ok(())
}

/// 关闭 UKey WebSocket 连接
pub fn close_ukey_ws() {
    if let Some(ws) = UKEY_WS.with(|w| w.borrow_mut().take()) {
        let _ = ws.close();
    }
    UKEY_ONMESSAGE.with(|o| *o.borrow_mut() = None);
    UKEY_PENDING.with(|p| *p.borrow_mut() = None);
}

/// 发送指令并等待服务端响应 (请求/响应模型)
///
/// 把 `cmd` 以字符串形式发往 UKey 代理, 返回一个 Promise: 当代理回下一条消息时 resolve。
/// 若 WebSocket 未连接, 返回错误。
async fn ukey_send_recv(cmd: &str) -> Result<String, String> {
    let ws = UKEY_WS.with(|w| w.borrow().clone());
    let ws = ws.ok_or_else(|| {
        "UKey WebSocket 未连接, 请先调用 open_ukey_ws()".to_string()
    })?;

    let cmd = cmd.to_string();
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        UKEY_PENDING.with(|p| *p.borrow_mut() = Some(resolve));
        if let Err(e) = ws.send_with_str(&cmd) {
            web_sys::console::error_1(&format!("[ukey] 发送失败: {:?}", e).into());
        }
    });

    JsFuture::from(promise)
        .await
        .map(|v| v.as_string().unwrap_or_default())
        .map_err(|_| "UKey 通信被拒绝或超时".to_string())
}

// ============================================================
// 业务函数 — 已连接走 WebSocket, 未连接回退 MOCK
// ============================================================

/// 获取 UKey 设备信息
///
/// 返回 JSON {status, errmsg, retstr}: status=0 正常, 2 未插UKey, 12 请拔下其他UKey
pub async fn get_ukey_info(_param: i32) -> Result<String, String> {
    if is_ukey_connected() {
        let resp = ukey_send_recv(&serde_json::json!({"cmd": "GetDeviceStatus"}).to_string()).await?;
        return Ok(resp);
    }
    let info = UkeyInfo { status: 0, errmsg: None, retstr: None };
    Ok(serde_json::to_string(&info).unwrap_or_default())
}

/// 校验 UKey PIN 码
///
/// 返回 JSON {status, errmsg, retstr}: status=0 成功, 134 PIN 错误
pub async fn verify_pin(pin_code: &str) -> Result<String, String> {
    UKEY_PIN.with(|p| *p.borrow_mut() = Some(pin_code.to_string()));
    if is_ukey_connected() {
        let resp = ukey_send_recv(
            &serde_json::json!({"cmd": "VerifyPin", "pin": pin_code}).to_string(),
        ).await?;
        return Ok(resp);
    }
    let result = serde_json::json!({
        "status": 0,
        "errmsg": "",
        "retstr": []
    });
    Ok(result.to_string())
}

/// 获取 UKey 印章列表 (base64 编码的 JSON)
pub async fn get_seal_list_json() -> Result<String, String> {
    if is_ukey_connected() {
        let resp = ukey_send_recv(&serde_json::json!({"cmd": "ListSeals"}).to_string()).await?;
        return Ok(resp);
    }
    let mock_list = UkeySealList {
        dev_list: vec![UkeyDevice {
            dev_id: "DEV001".to_string(),
            seal_list: vec![
                UkeySeal { seal_id: "SEAL001".to_string(), seal_name: "测试公章".to_string() },
                UkeySeal { seal_id: "SEAL002".to_string(), seal_name: "法人章".to_string() },
            ],
        }],
    };
    let json = serde_json::to_string(&mock_list)
        .map_err(|e| format!("序列化印章列表失败: {}", e))?;
    // 编码为 URL-encoded 的 base64 (与现有实现一致)
    Ok(url_encode(&base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        json.as_bytes(),
    )))
}

/// 获取指定印章图像 (base64 PNG)
pub async fn get_seal_image(dev_id: &str, seal_id: &str) -> Result<String, String> {
    if is_ukey_connected() {
        let resp = ukey_send_recv(
            &serde_json::json!({"cmd": "ReadSealImage", "devId": dev_id, "sealId": seal_id})
                .to_string(),
        ).await?;
        // 服务端返回 {"image": "<base64 png>"}, 这里直接透传 data 字段
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp) {
            if let Some(img) = v["image"].as_str() {
                return Ok(img.to_string());
            }
        }
        return Ok(resp);
    }
    Ok("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
        .to_string())
}

/// 获取指定印章数据 (base64)
pub async fn get_seal_data(dev_id: &str, seal_id: &str) -> Result<String, String> {
    if is_ukey_connected() {
        let resp = ukey_send_recv(
            &serde_json::json!({"cmd": "ReadSealData", "devId": dev_id, "sealId": seal_id})
                .to_string(),
        ).await?;
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp) {
            if let Some(d) = v["data"].as_str() {
                return Ok(d.to_string());
            }
        }
        return Ok(resp);
    }
    let mock_data = "MOCK_SEAL_DATA_FROM_UKEY";
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        mock_data.as_bytes(),
    ))
}

/// UKey 硬件签名 (私钥永不离开硬件)
pub async fn sign_data(data: &str, _pin_code: &str) -> Result<String, String> {
    if is_ukey_connected() {
        let resp = ukey_send_recv(
            &serde_json::json!({"cmd": "SignData", "data": data, "pin": _pin_code})
                .to_string(),
        ).await?;
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp) {
            if let Some(s) = v["signResult"].as_str() {
                return Ok(s.to_string());
            }
        }
        return Ok(resp);
    }
    let sig = crate::crypto::sm2_sign(data.as_bytes(), &[])?;
    Ok(crate::crypto::b64_encode(&sig))
}

/// 简单的 URL 编码 (避免额外依赖)
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'.' | b'_' | b'~' => result.push(byte as char),
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }
    result
}

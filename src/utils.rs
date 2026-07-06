//! 工具函数模块

use wasm_bindgen::prelude::*;
use js_sys::Array;
use web_sys::{Blob, BlobPropertyBag, Url, window};
use serde::Serialize;

/// 触发文件下载
pub fn download_file(data: &[u8], file_name: &str, mime_type: &str) -> Result<(), JsValue> {
    let window = window().ok_or(JsValue::from_str("无法获取 window 对象"))?;
    let document = window.document().ok_or(JsValue::from_str("无法获取 document 对象"))?;

    // 创建 Blob
    let blob_props = BlobPropertyBag::new();
    blob_props.set_type(mime_type);
    let blob = Blob::new_with_u8_array_sequence_and_options(
        &Array::from_iter(std::iter::once(JsValue::from(js_sys::Uint8Array::from(data)))),
        &blob_props,
    )?;

    // 创建下载 URL
    let url = Url::create_object_url_with_blob(&blob)
        .map_err(|_| JsValue::from_str("创建 Blob URL 失败"))?;

    // 创建临时 <a> 标签触发下载
    let a = document.create_element("a")
        .map_err(|_| JsValue::from_str("创建元素失败"))?;
    let a = a.dyn_into::<web_sys::HtmlElement>()
        .map_err(|_| JsValue::from_str("类型转换失败"))?;

    a.set_attribute("href", &url)?;
    a.set_attribute("download", file_name)?;
    a.set_attribute("style", "display: none")?;

    document.body()
        .ok_or(JsValue::from_str("无法获取 body"))?
        .append_child(&a)?;

    a.click();

    // 清理
    document.body()
        .ok_or(JsValue::from_str("无法获取 body"))?
        .remove_child(&a)?;
    Url::revoke_object_url(&url)
        .map_err(|_| JsValue::from_str("释放 URL 失败"))?;

    Ok(())
}

/// 将字节数据转为 base64 字符串
pub fn bytes_to_base64(data: &[u8]) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data)
}

/// 将 base64 字符串转为字节数据
pub fn base64_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
        .map_err(|e| format!("base64 解码失败: {}", e))
}

/// WebSocket 工具 — 发送消息并等待响应
pub async fn ws_send_recv(
    ws: &web_sys::WebSocket,
    message: &str,
    _timeout_ms: i32,
) -> Result<String, String> {
    ws.send_with_str(message)
        .map_err(|e| format!("WebSocket 发送失败: {:?}", e))?;

    // FIXME: 生产环境需要实现 Promise 封装等待 WebSocket 响应
    // 当前简化实现

    Ok(String::new())
}

/// 简单的 JSON 序列化
pub fn to_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value)
        .map_err(|e| format!("JSON序列化失败: {}", e))
}

/// 文件扩展名提取
pub fn file_extension(file_name: &str) -> &str {
    if let Some(pos) = file_name.rfind('.') {
        &file_name[pos + 1..]
    } else {
        ""
    }
}

/// 文件名去除扩展名
pub fn file_stem(file_name: &str) -> &str {
    if let Some(pos) = file_name.rfind('.') {
        &file_name[..pos]
    } else {
        file_name
    }
}

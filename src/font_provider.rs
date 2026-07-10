//! 自定义字体提供器 — 为 PDFium WASM 提供中文字体数据
//!
//! PDFium WASM 不内置字体提供器，CID 中文字体（如 STSong-Light）无法渲染。
//! 此模块通过 JS → Rust 注册机制，让浏览器端 fetch 字体文件后注入 WASM，
//! 然后实现 PdfiumCustomFontProvider trait 将字体数据提供给 PDFium。

use pdfium_render::prelude::*;
use wasm_bindgen::prelude::*;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

// ============================================================
// 全局字体注册表 — JS 端通过 register_font() 注入字体数据
// ============================================================

/// 已注册的字体数据，key 为字体名（小写），value 为 TTF/OTF 字体的原始字节
static FONT_REGISTRY: LazyLock<Mutex<HashMap<String, Vec<u8>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 从 JS 端注册一个字体文件的数据
/// name: 字体名称（如 "NotoSansSC"，也可以用别名如 "song"）
/// data: 字体文件的原始字节（TTF 或 OTF 格式）
#[wasm_bindgen]
pub fn register_font(name: &str, data: Vec<u8>) {
    let key = name.to_lowercase();
    web_sys::console::log_1(&format!(
        "[font_provider] 注册字体: name={} key={} size={}KB",
        name, key, data.len() / 1024
    ).into());

    if let Ok(mut registry) = FONT_REGISTRY.lock() {
        registry.insert(key, data.clone());

        // 同时注册为常见中文字体的别名，方便 PDF 请求时自动匹配
        let aliases = [
            "notosanssc", "notosanssc-regular",
            "stsong-light", "stsong", "simsun", "nsimsun",
            "fangsong", "stfangsong", "song",
            "simhei", "stheiti", "hei",
            "stkaiti", "kaiti", "kai",
            "microsoftyahei", "msyh",
            "default",
        ];
        for alias in aliases {
            if !registry.contains_key(alias) {
                registry.insert(alias.to_string(), data.clone());
            }
        }
    }
}

/// 检查是否已有系统字体（供 render.rs 判断是否注册字体提供器）
pub fn has_system_font() -> bool {
    if let Ok(registry) = FONT_REGISTRY.lock() {
        !registry.is_empty()
    } else {
        false
    }
}

/// 清空字体注册表
#[wasm_bindgen]
pub fn clear_fonts() {
    if let Ok(mut registry) = FONT_REGISTRY.lock() {
        registry.clear();
    }
}

// ============================================================
// 字体名映射表 — PDF 内部字体名 → 注册的替代字体名
// ============================================================

/// PDF 中常见 CID 中文字体名到替代字体名的映射
fn map_font_name(font_face: &str) -> Option<String> {
    let lower = font_face.to_lowercase();

    // 精确匹配
    let mapped = match lower.as_str() {
        // 宋体家族
        "stsong-light" | "stsong-light-regular" | "stsong"
        | "simsun" | "simsun-extb" | "nsimsun"
        | "songti-sc" | "songti-sc-regular" | "fangsong"
        | "fangsong-regular" | "stfangsong" => "song",
        // 黑体家族
        "stheiti" | "stheiti-regular" | "stheiti-light"
        | "simhei" | "heiiti-sc" | "heiiti-sc-medium"
        | "microsoftyahei" | "microsoftyaheiui" => "hei",
        // 楷体家族
        "stkaiti" | "stkaiti-regular" | "kaiti-sc"
        | "kaiti-sc-regular" | "kaiti" => "kai",
        // 通用中文字体名
        "notosanssc" | "notosanssc-regular" | "notoserifsc"
        | "sourcehansanssc" | "sourcehansanssc-regular"
        | "wenquanyimicrohei" | "wenquanyizenhei" => "song",
        _ => "",
    };

    if !mapped.is_empty() {
        return Some(mapped.to_string());
    }

    // 模糊匹配: 包含中文关键词
    if lower.contains("song") { return Some("song".to_string()); }
    if lower.contains("hei") { return Some("hei".to_string()); }
    if lower.contains("kai") { return Some("kai".to_string()); }

    // 模糊匹配: 包含 cjk/gb 等关键词的 CID 字体
    if lower.contains("cjk") || lower.contains("gb") || lower.contains("big5")
        || lower.contains("cn") || lower.contains("chinese") {
        return Some("song".to_string());
    }

    None
}

/// 从注册表中取出字体字节 (供 font_embed 模块在运行时嵌入 PDF 使用)
pub fn get_registered_font(name: &str) -> Option<Vec<u8>> {
    if let Ok(registry) = FONT_REGISTRY.lock() {
        if let Some(data) = registry.get(&name.to_lowercase()) {
            return Some(data.clone());
        }
    }
    None
}

/// 将 PdfFontCharacterSet 转为可读字符串（该枚举不实现 Debug）
fn charset_to_str(cs: &PdfFontCharacterSet) -> &'static str {
    match cs {
        PdfFontCharacterSet::Ansi => "Ansi",
        PdfFontCharacterSet::Default => "Default",
        PdfFontCharacterSet::Symbol => "Symbol",
        PdfFontCharacterSet::ChineseGb2312 => "ChineseGB2312",
        PdfFontCharacterSet::ChineseBig5 => "ChineseBig5",
        PdfFontCharacterSet::JapaneseShiftJis => "JapaneseShiftJIS",
        PdfFontCharacterSet::KoreanHangul => "KoreanHangul",
        PdfFontCharacterSet::Greek => "Greek",
        PdfFontCharacterSet::Vietnamese => "Vietnamese",
        PdfFontCharacterSet::Hebrew => "Hebrew",
        PdfFontCharacterSet::Arabic => "Arabic",
        PdfFontCharacterSet::Cyrillic => "Cyrillic",
        PdfFontCharacterSet::Thai => "Thai",
        PdfFontCharacterSet::EasternEuropean => "EasternEuropean",
    }
}

/// 将 PdfFontWeight 转为可读字符串
fn weight_to_str(w: &PdfFontWeight) -> String {
    match w {
        PdfFontWeight::Weight100 => "100".to_string(),
        PdfFontWeight::Weight200 => "200".to_string(),
        PdfFontWeight::Weight300 => "300".to_string(),
        PdfFontWeight::Weight400Normal => "400(Normal)".to_string(),
        PdfFontWeight::Weight500 => "500".to_string(),
        PdfFontWeight::Weight600 => "600".to_string(),
        PdfFontWeight::Weight700Bold => "700(Bold)".to_string(),
        PdfFontWeight::Weight800 => "800".to_string(),
        PdfFontWeight::Weight900 => "900".to_string(),
        PdfFontWeight::Custom(v) => format!("Custom({})", v),
    }
}

// ============================================================
// ChineseFontProvider — PdfiumCustomFontProvider 实现
// ============================================================

pub struct ChineseFontProvider {
    /// 自增 ID 用于给每个字体分配唯一 handle
    next_id: PdfiumCustomFontHandle,
}

impl ChineseFontProvider {
    pub fn new() -> Self {
        Self { next_id: 1 }
    }
}

impl PdfiumCustomFontProvider for ChineseFontProvider {
    fn provide(
        &mut self,
        request: PdfiumCustomFontProviderRequest,
    ) -> Option<PdfiumCustomFontProviderResponse> {
        web_sys::console::log_1(&format!(
            "[font_provider] PDFium 请求字体: face={} charset={} weight={} italic={} fixed={} serif={} cursive={}",
            request.font_face,
            charset_to_str(&request.character_set),
            weight_to_str(&request.weight),
            request.is_italic,
            request.is_fixed_pitch,
            request.is_serif,
            request.is_cursive,
        ).into());

        let registry = FONT_REGISTRY.lock().ok()?;

        // 1. 先尝试精确匹配注册的字体
        let lower_face = request.font_face.to_lowercase();

        if let Some(data) = registry.get(&lower_face) {
            let id = self.next_id;
            self.next_id += 1;
            web_sys::console::log_1(&format!(
                "[font_provider] 精确匹配: face={} → '{}' ({}KB)",
                request.font_face, lower_face, data.len() / 1024
            ).into());
            return Some(PdfiumCustomFontProviderResponse {
                id,
                font_face: request.font_face.clone(),
                character_set: request.character_set,
                data: data.clone(),
            });
        }

        // 2. 尝试映射到替代字体名
        let mapped_name = map_font_name(&request.font_face);
        if let Some(ref alias) = mapped_name {
            if let Some(data) = registry.get(alias) {
                let id = self.next_id;
                self.next_id += 1;
                web_sys::console::log_1(&format!(
                    "[font_provider] 映射匹配: face={} → '{}' ({}KB)",
                    request.font_face, alias, data.len() / 1024
                ).into());
                return Some(PdfiumCustomFontProviderResponse {
                    id,
                    font_face: request.font_face.clone(),
                    character_set: request.character_set,
                    data: data.clone(),
                });
            }
        }

        // 3. 如果是 CJK 字符集，尝试任意已注册字体
        let is_cjk = matches!(
            request.character_set,
            PdfFontCharacterSet::ChineseGb2312
            | PdfFontCharacterSet::ChineseBig5
            | PdfFontCharacterSet::JapaneseShiftJis
            | PdfFontCharacterSet::KoreanHangul
        );

        if is_cjk {
            for (key, data) in registry.iter() {
                let id = self.next_id;
                self.next_id += 1;
                web_sys::console::log_1(&format!(
                    "[font_provider] CJK 兜底: face={} → '{}' ({}KB)",
                    request.font_face, key, data.len() / 1024
                ).into());
                return Some(PdfiumCustomFontProviderResponse {
                    id,
                    font_face: request.font_face.clone(),
                    character_set: request.character_set,
                    data: data.clone(),
                });
            }
        }

        // 4. 没有匹配的字体
        web_sys::console::warn_1(&format!(
            "[font_provider] 无匹配字体: face={} charset={}",
            request.font_face,
            charset_to_str(&request.character_set),
        ).into());

        None
    }
}

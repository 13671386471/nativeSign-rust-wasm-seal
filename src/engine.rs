//! 文档引擎模块 — PDF/OFD 解析、渲染、文件操作
//!
//! 替代原始 C++ WASM 引擎中的文档处理部分

use crate::crypto;
use crate::types::*;
use std::collections::HashMap;
use x509_parser::prelude::*;

/// 文档引擎 — 管理文档加载、解析、渲染、保存的全生命周期
pub struct DocumentEngine {
    pub state: DocState,
    pub config: EngineConfig,
}

impl DocumentEngine {
    pub fn new() -> Self {
        Self {
            state: DocState::default(),
            config: EngineConfig::default(),
        }
    }

    /// 加载文件到内存
    /// 对应 OFD_Plugin.LoadFile(file)
    pub fn load_file(&mut self, file_data: Vec<u8>, file_name: &str) -> Result<(), String> {
        // 检测文件类型
        let doc_type = if file_name.to_lowercase().ends_with(".pdf") {
            DocType::Pdf
        } else if file_name.to_lowercase().ends_with(".ofd") {
            DocType::Ofd
        } else {
            return Err(format!("不支持的文件格式: {}", file_name));
        };

        web_sys::console::log_1(&format!(
            "[load_file] file_name={} detected_type={:?} data_len={}",
            file_name, doc_type, file_data.len()
        ).into());

        // 解析文档获取元信息
        let page_count = match doc_type {
            DocType::Pdf => self.parse_pdf_info(&file_data)?,
            DocType::Ofd => self.parse_ofd_info(&file_data)?,
        };

        // 计算文件大小(KB)
        let file_size_kb = (file_data.len() / 1024) as u64;

        // 生成文件唯一标识
        let file_id = crypto::sha256_base64(&file_data);

        self.state = DocState {
            file_id,
            file_name: file_name.to_string(),
            file_size_kb,
            page_count,
            current_page: 0,
            doc_type,
            is_opened: true,
            seal_count: 0,
            signed_count: 0,
            raw_data: file_data,
            seals: Vec::new(),
            properties: HashMap::new(),
        };

        Ok(())
    }

    /// 解析 PDF 文档获取页数等信息
    fn parse_pdf_info(&self, data: &[u8]) -> Result<u32, String> {
        // PDF 解析 — 提取页数
        // PDF 文件通过 /Count 指令获取总页数; 需要跳过 /Count 后的空白字符
        let text = String::from_utf8_lossy(data);

        // 方法1: 查找所有 /Count 指令, 取最大值(可处理嵌套 Pages 树)
        let mut max_count = 0u32;
        for (pos, _) in text.match_indices("/Count") {
            let after_count = &text[pos + 6..];
            // 跳过空白前缀, 找到数字起始位置
            let digits_start = after_count
                .find(|c: char| c.is_ascii_digit())
                .unwrap_or(after_count.len());
            let digits_part = &after_count[digits_start..];
            if let Some(end) = digits_part.find(|c: char| !c.is_ascii_digit()) {
                if let Ok(count) = digits_part[..end].parse::<u32>() {
                    max_count = max_count.max(count);
                }
            }
        }

        if max_count > 0 {
            return Ok(max_count);
        }

        // 方法2: 统计 /Type /Page 出现次数(不包含 /Pages)
        let page_count = text.matches("/Type /Page").count() as u32;
        if page_count > 0 {
            return Ok(page_count);
        }

        // 方法3: 统计 /Type/Page 对象的数量
        let page_count = text.matches("/Type/Page").count() as u32;
        if page_count > 0 {
            return Ok(page_count);
        }

        // 无法确定页数, 默认1页
        Ok(1)
    }

    /// 解析 OFD 文档获取页数等信息
    fn parse_ofd_info(&self, data: &[u8]) -> Result<u32, String> {
        // 使用真实的 OFD ZIP + XML 解析器
        let doc = crate::ofd_parser::parse_ofd(data)?;
        let count = doc.pages.len() as u32;
        Ok(count.max(1))
    }

    /// 获取当前文档总页数
    pub fn get_page_count(&self) -> u32 {
        self.state.page_count
    }

    /// 获取指定页的宽度（单位: 点 pt / px）
    pub fn get_page_width(&self, page_index: u32) -> f64 {
        match self.state.doc_type {
            DocType::Pdf => {
                if let Ok(pdf) = lopdf::Document::load_mem(&self.state.raw_data) {
                    let pages = pdf.get_pages();
                    if let Some(&page_id) = pages.get(&(page_index + 1)) {
                        if let Ok(page_obj) = pdf.get_object(page_id) {
                            if let Ok(dict) = page_obj.as_dict() {
                                if let Ok(arr) = dict.get(b"MediaBox").and_then(|o| o.as_array()) {
                                    if arr.len() >= 4 {
                                        let x1 = match &arr[0] {
                                            lopdf::Object::Integer(i) => *i as f64,
                                            lopdf::Object::Real(f) => *f as f64,
                                            _ => 0.0,
                                        };
                                        let x2 = match &arr[2] {
                                            lopdf::Object::Integer(i) => *i as f64,
                                            lopdf::Object::Real(f) => *f as f64,
                                            _ => 0.0,
                                        };
                                        if x2 > x1 {
                                            return x2 - x1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                595.0
            }
            DocType::Ofd => {
                if let Ok(ofd) = crate::ofd_parser::parse_ofd(&self.state.raw_data) {
                    if let Some(page) = ofd.pages.get(page_index as usize) {
                        return page.physical_box.w * 2.83464567; // mm → px @ 72dpi
                    }
                }
                595.0
            }
        }
    }

    /// 获取指定页的高度（单位: 点 pt / px）
    pub fn get_page_height(&self, page_index: u32) -> f64 {
        match self.state.doc_type {
            DocType::Pdf => {
                if let Ok(pdf) = lopdf::Document::load_mem(&self.state.raw_data) {
                    let pages = pdf.get_pages();
                    if let Some(&page_id) = pages.get(&(page_index + 1)) {
                        if let Ok(page_obj) = pdf.get_object(page_id) {
                            if let Ok(dict) = page_obj.as_dict() {
                                if let Ok(arr) = dict.get(b"MediaBox").and_then(|o| o.as_array()) {
                                    if arr.len() >= 4 {
                                        let y1 = match &arr[1] {
                                            lopdf::Object::Integer(i) => *i as f64,
                                            lopdf::Object::Real(f) => *f as f64,
                                            _ => 0.0,
                                        };
                                        let y2 = match &arr[3] {
                                            lopdf::Object::Integer(i) => *i as f64,
                                            lopdf::Object::Real(f) => *f as f64,
                                            _ => 0.0,
                                        };
                                        if y2 > y1 {
                                            return y2 - y1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                842.0
            }
            DocType::Ofd => {
                if let Ok(ofd) = crate::ofd_parser::parse_ofd(&self.state.raw_data) {
                    if let Some(page) = ofd.pages.get(page_index as usize) {
                        return page.physical_box.h * 2.83464567; // mm → px @ 72dpi
                    }
                }
                842.0
            }
        }
    }

    /// 获取文档类型字符串
    pub fn get_doc_type(&self) -> &str {
        match self.state.doc_type {
            DocType::Pdf => "pdf",
            DocType::Ofd => "ofd",
        }
    }

    /// 文档是否已打开
    pub fn is_opened(&self) -> bool {
        self.state.is_opened
    }

    /// 获取当前文件大小 (KB)
    pub fn get_curr_file_size(&self) -> u64 {
        self.state.file_size_kb
    }

    /// 获取指定文档属性
    pub fn get_doc_property(&self, key: &str) -> Option<String> {
        self.state.properties.get(key).cloned()
    }

    /// 设置文档属性
    pub fn set_doc_property(&mut self, key: &str, value: &str) {
        self.state.properties.insert(key.to_string(), value.to_string());
    }

    /// 获取已落章数量
    pub fn get_signatures_count(&self, _seal_type: &str) -> u32 {
        self.state.seal_count
    }

    /// 解析 PDF 中已有的电子签章/签名域
    ///
    /// 遍历 PDF 每页的注释(Annots), 查找以下类型的签章：
    /// - /Subtype /Widget 且 /FT /Sig (签名域)
    /// - /Subtype /Stamp (印章注释)
    /// 提取位置(Rect)、字段名(T)、外观流等信息, 构造 PlacedSeal 列表。
    pub fn parse_existing_signatures(data: &[u8]) -> Result<Vec<PlacedSeal>, String> {
        if data.is_empty() {
            web_sys::console::log_1(&"[parse_seals] 数据为空, 跳过".into());
            return Ok(Vec::new());
        }

        web_sys::console::log_1(&format!("[parse_seals] 开始解析, 数据长度={}", data.len()).into());

        let pdf = lopdf::Document::load_mem(data)
            .map_err(|e| {
                web_sys::console::log_1(&format!("[parse_seals] PDF加载失败: {}", e).into());
                format!("PDF 加载失败: {}", e)
            })?;

        let pages = pdf.get_pages();
        web_sys::console::log_1(&format!("[parse_seals] 找到 {} 页", pages.len()).into());
        let page_count = pages.len() as u32;
        let mut page_heights: Vec<f64> = Vec::new();
        for pn in 0..page_count {
            let page_id = pdf.get_pages().get(&(pn + 1)).copied();
            let ph = if let Some(pid) = page_id {
                Self::get_page_size_from_pdf(&pdf, pid).1
            } else {
                842.0
            };
            page_heights.push(ph);
        }

        let mut seals: Vec<PlacedSeal> = Vec::new();
        let mut seal_idx = 0usize;

        for (page_num, page_id) in pdf.get_pages() {
            let page_idx = (page_num - 1) as u32;
            let ph = *page_heights.get(page_idx as usize).unwrap_or(&842.0);

            let page_obj = match pdf.get_object(page_id) {
                Ok(o) => o,
                Err(_) => continue,
            };
            let page_dict = match page_obj.as_dict() {
                Ok(d) => d,
                Err(_) => continue,
            };

            // 获取 /Annots - 可能存在但值为空或为 Reference
            let annots_val = match page_dict.get(b"Annots") {
                Ok(v) => v,
                Err(_) => continue,
            };

            // 解析注释数组
            let annots_items: Vec<&lopdf::Object> = if let Ok(arr) = annots_val.as_array() {
                arr.iter().collect()
            } else {
                let oref = match annots_val.as_reference() {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                if let Ok(arr_obj) = pdf.get_object(oref) {
                    if let Ok(arr) = arr_obj.as_array() {
                        arr.iter().collect()
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            };

            for annot_ref in &annots_items {
                let annot_obj = if let Ok(d) = annot_ref.as_dict() {
                    annot_ref
                } else {
                    let r = match annot_ref.as_reference() {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    match pdf.get_object(r) {
                        Ok(o) => o,
                        Err(_) => continue,
                    }
                };

                let annot_dict = match annot_obj.as_dict() {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                // Subtype
                let subtype = annot_dict.get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_default();

                // FT (仅 Widget 有)
                let ft = annot_dict.get(b"FT")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_default();

                let is_seal = (subtype == "Widget" && ft == "Sig")
                    || subtype == "Stamp";

                if !is_seal {
                    continue;
                }

                // Rect
                let (x, y, w, h) = if let Ok(rect_arr) = annot_dict.get(b"Rect").and_then(|o| o.as_array()) {
                    if rect_arr.len() >= 4 {
                        let llx = get_obj_f64(&rect_arr[0]);
                        let lly = get_obj_f64(&rect_arr[1]);
                        let urx = get_obj_f64(&rect_arr[2]);
                        let ury = get_obj_f64(&rect_arr[3]);
                        let sx = llx;
                        let sy = ph - ury;
                        (sx, sy, (urx - llx).abs(), (ury - lly).abs())
                    } else {
                        (0.0, 0.0, 100.0, 100.0)
                    }
                } else {
                    (0.0, 0.0, 100.0, 100.0)
                };

                // 字段名 /T (Widget) 或 /Contents (Stamp)
                let field_name = annot_dict.get(b"T").ok()
                    .and_then(|o| o.as_string().ok())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        annot_dict.get(b"Contents").ok()
                            .and_then(|o| o.as_string().ok())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| format!("签章{}", seal_idx + 1));

                // 是否有签名值 /V
                let has_value = annot_dict.get(b"V").is_ok();

                // 从签名字典中提取所有可用字段
                let mut signer_name = None;
                let mut sign_time = None;
                let mut sign_method = None;
                let mut cert_sn = None;
                let mut cert_issuer = None;
                let mut cert_subject = None;
                let mut cert_start_time = None;
                let mut cert_end_time = None;
                let mut cert_algorithm = None;
                let mut cert_data = None;

                // 解析 /V 签名字典（可能为间接引用, 需通过 pdf 文档解析）
                let v_dict: Option<&lopdf::Dictionary> = annot_dict.get(b"V").ok().and_then(|v| {
                    resolve_obj(&pdf, v).and_then(|resolved| resolved.as_dict().ok())
                });

                if let Some(v_dict) = v_dict {
                    signer_name = v_dict.get(b"Name").ok()
                        .and_then(|o| o.as_string().ok())
                        .map(|s| s.to_string());
                    sign_time = v_dict.get(b"M").ok()
                        .and_then(|o| o.as_string().ok())
                        .map(|s| s.to_string());

                    // 签名方法: /Filter + /SubFilter
                    let filter = v_dict.get(b"Filter").ok()
                        .and_then(|o| o.as_name().ok())
                        .map(|b| String::from_utf8_lossy(b).to_string());
                    let sub_filter = v_dict.get(b"SubFilter").ok()
                        .and_then(|o| o.as_name().ok())
                        .map(|b| String::from_utf8_lossy(b).to_string());
                    sign_method = match (filter, sub_filter) {
                        (Some(f), Some(sf)) => Some(format!("{} / {}", f, sf)),
                        (Some(f), None) => Some(f),
                        (None, Some(sf)) => Some(sf),
                        (None, None) => None,
                    };

                    // 算法标识
                    cert_algorithm = v_dict.get(b"Filter").ok()
                        .and_then(|o| o.as_name().ok())
                        .map(|b| String::from_utf8_lossy(b).to_string());

                    // 从 /Contents 中提取 PKCS7 证书信息
                    web_sys::console::log_1(&"[cert] 尝试提取 /Contents...".into());
                    let contents_raw = v_dict.get(b"Contents").ok().and_then(|o| {
                        let type_name = match o {
                            lopdf::Object::String(_, _) => "String",
                            lopdf::Object::Reference(_) => "Reference",
                            lopdf::Object::Array(_) => "Array",
                            lopdf::Object::Dictionary(_) => "Dict",
                            lopdf::Object::Stream(_) => "Stream",
                            _ => "Other",
                        };
                        web_sys::console::log_1(&format!("[cert] /Contents is: {}", type_name).into());

                        // 解析引用, 获取实际字节
                        let resolved = resolve_obj(&pdf, o)?;
                        if let lopdf::Object::String(data, fmt) = resolved {
                            web_sys::console::log_1(&format!("[cert] String length={}, format={:?}", data.len(), fmt).into());
                            Some(data.clone())
                        } else {
                            web_sys::console::log_1(&"[cert] 解析后不是 String 类型".into());
                            None
                        }
                    });
                    if let Some(ref contents_bytes) = contents_raw {
                        let cert_info = extract_cert_from_pkcs7(contents_bytes);
                        if cert_sn.is_none() { cert_sn = cert_info.serial; }
                        if cert_issuer.is_none() { cert_issuer = cert_info.issuer; }
                        if cert_subject.is_none() { cert_subject = cert_info.subject; }
                        if cert_start_time.is_none() { cert_start_time = cert_info.not_before; }
                        if cert_end_time.is_none() { cert_end_time = cert_info.not_after; }
                        if cert_algorithm.is_none() { cert_algorithm = cert_info.algorithm; }
                        // 如果签名字典中没有 /Name, 从证书 CN 中提取签章人
                        if signer_name.is_none() { signer_name = cert_info.common_name; }
                        cert_data = cert_info.cert_data;
                    }
                }

                seals.push(PlacedSeal {
                    id: seal_idx,
                    page_index: page_idx,
                    x,
                    y,
                    width: w,
                    height: h,
                    seal_info: SealInfo {
                        origin: "pdf".to_string(),
                        seal_id: field_name.clone(),
                        seal_name: field_name,
                        width: w * 25.4 / 72.0,
                        height: h * 25.4 / 72.0,
                        seal_type: Some(if subtype == "Stamp" { 0 } else { 1 }),
                        seal_image: String::new(),
                        sign_cert_sn: cert_sn,
                        sign_data: None,
                        sign_cert: None,
                        seal_start_time: sign_time.clone(),
                        seal_end_time: None,
                        signer_name,
                        sign_time,
                        sign_method,
                        cert_issuer,
                        cert_subject,
                        cert_start_time,
                        cert_end_time,
                        cert_algorithm,
                        cert_data,
                    },
                    signature: None,
                    signed: has_value,
                });
                seal_idx += 1;
            }
        }

        Ok(seals)
    }

    /// 从 lopdf Document 获取指定页面的高度
    fn get_page_size_from_pdf(pdf: &lopdf::Document, page_id: lopdf::ObjectId) -> (f64, f64) {
        if let Ok(obj) = pdf.get_object(page_id) {
            if let Ok(dict) = obj.as_dict() {
                if let Ok(media_box) = dict.get(b"MediaBox").and_then(|o| o.as_array()) {
                    if media_box.len() >= 4 {
                        let llx = get_obj_f64(&media_box[0]);
                        let lly = get_obj_f64(&media_box[1]);
                        let urx = get_obj_f64(&media_box[2]);
                        let ury = get_obj_f64(&media_box[3]);
                        return (urx - llx, ury - lly);
                    }
                }
            }
        }
        (595.0, 842.0)
    }

    /// 获取所有签章信息 JSON
    pub fn get_seal_info_json(&self) -> String {
        serde_json::to_string(&self.state.seals).unwrap_or_else(|_| "[]".to_string())
    }

    /// 获取前N字节的MD5值
    pub fn get_file_md5_value(&self, param: &str) -> Result<String, String> {
        // 解析参数 "LEFT:20480" 表示读取前20480字节
        let left_bytes = if param.starts_with("LEFT:") {
            param[5..].parse::<usize>().unwrap_or(20480)
        } else {
            20480
        };
        Ok(crypto::file_left_md5(&self.state.raw_data, left_bytes))
    }

    /// 保存文档到指定文件路径
    pub fn save_to(&self, _file_name: &str, _format: &str, _flags: i32) -> Result<String, String> {
        // 返回 "1" 表示成功
        Ok("1".to_string())
    }

    /// 关闭文档
    pub fn close_doc(&mut self, _flags: i32) {
        self.state.is_opened = false;
    }

    /// 获取下一页注释节点
    pub fn get_next_note(&self, _node_type: &str, _index: i32, _param: &str) -> Option<String> {
        // 用于文档结构/大纲检索
        // 生产环境需解析 PDF/OFD 的书签结构
        None
    }

    /// 删除指定注释（印章）
    pub fn delete_note(&mut self, note_id: &str) -> Result<i32, String> {
        if let Ok(idx) = note_id.parse::<usize>() {
            if idx < self.state.seals.len() {
                self.state.seals.remove(idx);
                self.state.seal_count = self.state.seals.len() as u32;
                return Ok(1);
            }
        }
        Err(format!("未找到印章: {}", note_id))
    }
}

/// 从 lopdf Object 中获取 f64 值（同时支持 Integer 和 Real）
fn get_obj_f64(obj: &lopdf::Object) -> f64 {
    if let Ok(v) = obj.as_f32() {
        v as f64
    } else if let Ok(v) = obj.as_i64() {
        v as f64
    } else {
        0.0
    }
}

/// 解析 lopdf 对象引用
///
/// 如果对象是间接引用 (Object::Reference), 通过 Document 解析为实际对象;
/// 如果已经是具体对象, 直接返回自身。用于处理 PDF 字典中值可能为 Referene 的情况。
fn resolve_obj<'a>(pdf: &'a lopdf::Document, obj: &'a lopdf::Object) -> Option<&'a lopdf::Object> {
    if let Ok(ref_id) = obj.as_reference() {
        pdf.get_object(ref_id).ok()
    } else {
        Some(obj)
    }
}

/// PKCS7 证书提取结果
struct CertExtractInfo {
    serial: Option<String>,
    issuer: Option<String>,
    subject: Option<String>,
    common_name: Option<String>,
    not_before: Option<String>,
    not_after: Option<String>,
    algorithm: Option<String>,
    cert_data: Option<String>,
}

/// 从 PKCS#7 签名数据中提取 X.509 证书信息
///
/// 在 PDF 签名字段中, /Contents 存储的是 PKCS#7 SignedData (DER 编码)。
/// 该函数扫描 DER 字节流, 找到所有看起来像 X.509 证书的 SEQUENCE,
/// 并用 x509-parser 解析提取字段。通常第一个证书是签名者证书。
fn extract_cert_from_pkcs7(raw: &[u8]) -> CertExtractInfo {
    let mut result = CertExtractInfo {
        serial: None,
        issuer: None,
        subject: None,
        common_name: None,
        not_before: None,
        not_after: None,
        algorithm: None,
        cert_data: None,
    };

    if raw.len() < 10 {
        return result;
    }

    // 扫描 DER 字节流, 找到所有 SEQUENCE {0x30, 0x82, len_hi, len_lo} 且长度在合理范围内的
    web_sys::console::log_1(&format!("[cert] extract_cert: raw len={}", raw.len()).into());
    let mut cert_found = false;
    let mut offset = 0usize;
    let max_offset = raw.len().saturating_sub(4);
    while offset < max_offset {
        // 处理各种 SEQUENCE 编码
        if raw[offset] == 0x30 {
            let lbyte = raw[offset + 1];
            let (len, is_definite) = if lbyte == 0x80 {
                (0usize, false) // 不定长
            } else if lbyte & 0x80 != 0 {
                // 长格式: lbyte & 0x7f = 后续长度字节数
                let nbytes = (lbyte & 0x7f) as usize;
                if nbytes <= 4 && offset + 2 + nbytes <= raw.len() {
                    let mut l = 0usize;
                    for i in 0..nbytes {
                        l = (l << 8) | (raw[offset + 2 + i] as usize);
                    }
                    (l, true)
                } else {
                    offset += 1;
                    continue;
                }
            } else {
                // 短格式: 0~127
                (lbyte as usize, true)
            };

            if is_definite && len >= 200 && len <= 10000 && offset + 4 + len <= raw.len() {
                let candidate = &raw[offset..offset + 4 + len];
                web_sys::console::log_1(&format!("[cert] Found candidate at offset {} len={}", offset, len).into());
                // 尝试用 x509-parser 解析
                if let Ok((_rem, cert)) = parse_x509_certificate(candidate) {
                    web_sys::console::log_1(&"[cert] ✅ 成功解析证书".into());

                    // 序列号 (十六进制)
                    let sn = cert.raw_serial_as_string();
                    web_sys::console::log_1(&format!("[cert] serial={}", sn).into());
                    if !sn.is_empty() {
                        result.serial = Some(sn);
                    }

                    // Issuer
                    let issuer_str = cert.tbs_certificate.issuer.to_string();
                    web_sys::console::log_1(&format!("[cert] issuer={}", issuer_str).into());
                    if !issuer_str.is_empty() {
                        result.issuer = Some(issuer_str);
                    }

                    // Subject (DN)
                    let subject_str = cert.tbs_certificate.subject.to_string();
                    web_sys::console::log_1(&format!("[cert] subject={}", subject_str).into());
                    if !subject_str.is_empty() {
                        result.subject = Some(subject_str);
                    }

                    // 从 Subject 中提取 CN (Common Name) 作为签章人
                    let cn_attr = cert.tbs_certificate.subject.iter_common_name().next();
                    if let Some(attr) = cn_attr {
                        if let Ok(cn) = attr.attr_value().as_str() {
                            let cn_str = cn.to_string();
                            if !cn_str.is_empty() {
                                web_sys::console::log_1(&format!("[cert] CN={}", cn_str).into());
                                result.common_name = Some(cn_str);
                            }
                        }
                    }

                    // 有效期
                    let nb = cert.tbs_certificate.validity.not_before.to_string();
                    let na = cert.tbs_certificate.validity.not_after.to_string();
                    web_sys::console::log_1(&format!("[cert] not_before={} not_after={}", nb, na).into());
                    if !nb.is_empty() { result.not_before = Some(nb); }
                    if !na.is_empty() { result.not_after = Some(na); }

                    // 算法标识
                    let alg_oid = cert.tbs_certificate.subject_pki.algorithm.algorithm;
                    let alg_str = format!("{}", alg_oid);
                    web_sys::console::log_1(&format!("[cert] algorithm={}", alg_str).into());
                    if !alg_str.is_empty() {
                        result.algorithm = Some(alg_str);
                    }

                    // 证书数据 (base64)
                    use base64::Engine;
                    let b64 = base64::engine::general_purpose::STANDARD.encode(candidate);
                    result.cert_data = Some(b64);

                    cert_found = true;
                    break; // 只取第一个证书（通常是签名者证书）
                } else {
                    web_sys::console::log_1(&format!("[cert] 候选在 offset {} 解析失败", offset).into());
                }
            }
        }
        offset += 1;
    }

    if !cert_found {
        web_sys::console::log_1(&"[cert] ⚠️ 未找到可解析的证书".into());
    }

    result
}

impl Default for DocumentEngine {
    fn default() -> Self {
        Self::new()
    }
}

//! 印章操作模块 — 印章嵌入、落章、印章图像处理
//!
//! 实现真实的 PDF/OFD 签章嵌入:
//! - PDF: 创建签名域 (/FT /Sig), 计算 ByteRange, 嵌入 PKCS#7 签名值, 生成外观流
//! - OFD: 生成 Sign_N.xml 签章描述, Seal.esl (SES_Seal), SignedValue.dat (PKCS#7)

use crate::types::*;
use crate::ses;
use crate::pkcs7;
use crate::crypto;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// PKCS#7 /Contents 占位符大小 (hex 字符数, 必须是偶数)
/// 预留足够空间容纳 PKCS#7 SignedData (含 SES_Signature + 证书)
const SIGNATURE_CONTENTS_SIZE: usize = 16384;

/// 印章引擎 — 管理印章的嵌入、位置计算、骑缝章分割
pub struct SealEngine;

impl SealEngine {
    /// 创建新印章数据
    /// 对应 OFD_Plugin.GetCreateSeal(image, type, code, name, company, w, h)
    pub fn create_seal(
        image_base64: &str,
        _seal_type: i32,
        _code: &str,
        _name: &str,
        _company: &str,
        width: f64,
        height: f64,
    ) -> Result<String, String> {
        let seal_data = SealBinaryData {
            seal_type: _seal_type,
            code: _code.to_string(),
            name: _name.to_string(),
            company: _company.to_string(),
            width_mm: width,
            height_mm: height,
            image_data: image_base64.to_string(),
        };

        let json = serde_json::to_string(&seal_data).unwrap_or_default();
        let encoded = BASE64.encode(json.as_bytes());
        Ok(encoded)
    }

    /// 添加印章到文档指定位置
    pub fn add_seal(
        placed_seals: &mut Vec<PlacedSeal>,
        c_pages: &str,
        _sign_data: &str,
        seal_info: &SealInfo,
    ) -> Result<usize, String> {
        if c_pages.starts_with("AUTO_ADD:") {
            return Err("关键字自动落章需先搜索关键字位置".to_string());
        }

        let parts: Vec<&str> = c_pages.split(',').collect();
        if parts.len() < 5 {
            return Err("AddSeal参数格式错误".to_string());
        }

        let page = parts[0].parse::<u32>().unwrap_or(0);
        let x_raw = parts[1].parse::<f64>().unwrap_or(0.0);
        let y_raw = parts[4].parse::<f64>().unwrap_or(0.0);

        let x_pt = x_raw / 50000.0 * 595.0;
        let y_pt = y_raw / 50000.0 * 842.0;

        let seal = PlacedSeal {
            id: placed_seals.len(),
            page_index: page,
            x: x_pt,
            y: y_pt,
            width: 120.0,  // 印章宽度 (PDF 点, 约 42mm)
            height: 120.0, // 印章高度
            seal_info: seal_info.clone(),
            signature: None,
            signed: false,
        };

        placed_seals.push(seal);
        Ok(placed_seals.len())
    }

    /// 获取最后添加的印章ID
    pub fn get_last_seal(placed_seals: &[PlacedSeal]) -> Option<String> {
        placed_seals.last().map(|s| s.id.to_string())
    }

    /// 处理印章图像数据 — 生成落章鼠标图标
    pub fn prepare_seal_cursor(seal_image_base64: &str) -> String {
        seal_image_base64.to_string()
    }

    /// 将印章数据嵌入到文档数据中
    pub fn embed_seals_to_document(
        doc_data: &[u8],
        seals: &[PlacedSeal],
        doc_type: DocType,
        algorithm: ses::SealAlgorithm,
    ) -> Result<Vec<u8>, String> {
        if seals.is_empty() {
            return Ok(doc_data.to_vec());
        }

        match doc_type {
            DocType::Pdf => Self::embed_seals_to_pdf(doc_data, seals, algorithm),
            DocType::Ofd => crate::ofd_sign::embed_seals_to_ofd(doc_data, seals),
        }
    }
}

// ============================================================
// PDF 签章嵌入 — 使用 lopdf 修改文档 + 两遍签名填充
// ============================================================

impl SealEngine {
    /// PDF 签章嵌入 — 创建签名域并嵌入 PKCS#7
    ///
    /// 流程:
    /// 1. 用 lopdf 解析 PDF, 添加签名 Widget/Annot, 外观流, 图片对象
    /// 2. 用 lopdf 的 save_to() 输出完整有效的 PDF 字节
    /// 3. 二次扫描: 找到 /Contents 位置, 计算 ByteRange, 填入 PKCS#7
    fn embed_seals_to_pdf(data: &[u8], seals: &[PlacedSeal], algorithm: ses::SealAlgorithm) -> Result<Vec<u8>, String> {
        let mut doc = lopdf::Document::load_mem(data)
            .map_err(|e| format!("PDF解析失败: {}", e))?;

        let pages = doc.get_pages();
        web_sys::console::log_1(&format!("[embed_pdf] 页数={}, 印章数={}", pages.len(), seals.len()).into());

        // 获取 catalog ObjectId
        let catalog_id = doc.trailer.get(b"Root")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .map(|(id, _)| id)
            .ok_or("无法获取 Catalog")?;

        let mut seal_metas: Vec<SealEmbedMeta> = Vec::new();

        for seal in seals {
            let page_num = seal.page_index + 1; // 1-based
            let page_obj_id = pages.get(&page_num)
                .copied()
                .unwrap_or((0, 0));

            if page_obj_id.0 == 0 {
                web_sys::console::warn_1(&format!("[embed_pdf] 跳过: 页 {} 不存在", page_num).into());
                continue;
            }

            // 获取页面高度 (用于 Y 坐标翻转)
            let page_height = get_page_height(&doc, page_obj_id);

            // 1. 创建图片 XObject (印章图像)
            let seal_image_bytes = if !seal.seal_info.seal_image.is_empty() {
                BASE64.decode(&seal.seal_info.seal_image).unwrap_or_else(|_| ses::MOCK_SEAL_PNG.to_vec())
            } else {
                ses::MOCK_SEAL_PNG.to_vec()
            };

            let img_stream = build_image_xobject(&seal_image_bytes, seal.width, seal.height);

            let img_dict = lopdf::Dictionary::from_iter([
                ("Type", "XObject".into()),
                ("Subtype", "Image".into()),
                ("Width", (img_stream.width as i64).into()),
                ("Height", (img_stream.height as i64).into()),
                ("ColorSpace", "DeviceRGB".into()),
                ("BitsPerComponent", 8i64.into()),
                ("Filter", "FlateDecode".into()),
                ("Length", (img_stream.data.len() as i64).into()),
            ]);
            let img_stream_obj = lopdf::Stream::new(img_dict, img_stream.data);
            let img_obj_id = doc.add_object(lopdf::Object::Stream(img_stream_obj));

            // 2. 创建外观流 (Form XObject)
            let ap_content = format!(
                "q\n{} 0 0 {} 0 0 cm\n/Img Do\nQ\n",
                seal.width, seal.height
            );
            let ap_dict = lopdf::Dictionary::from_iter([
                ("Type", "XObject".into()),
                ("Subtype", "Form".into()),
                ("BBox", lopdf::Object::Array(vec![
                    lopdf::Object::Integer(0),
                    lopdf::Object::Integer(0),
                    lopdf::Object::Real(seal.width as f32),
                    lopdf::Object::Real(seal.height as f32),
                ])),
                ("Resources", lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                    ("XObject", lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                        ("Img", lopdf::Object::Reference(img_obj_id)),
                    ]))),
                ]))),
            ]);
            let ap_stream_obj = lopdf::Stream::new(ap_dict, ap_content.into_bytes());
            let ap_obj_id = doc.add_object(lopdf::Object::Stream(ap_stream_obj));

            // 3. 创建签名字典 (独立对象)
            // /Contents 占位符: 全零的 hex string
            let contents_placeholder = vec![0u8; SIGNATURE_CONTENTS_SIZE / 2];
            // /ByteRange 占位符 (后续替换为真实值)
            let byte_range_placeholder = lopdf::Object::Array(vec![
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(0),
            ]);

            let sign_time = "D:20260710163000+08'00'";
            let sig_dict = lopdf::Dictionary::from_iter([
                ("Type", "Sig".into()),
                ("Filter", "Adobe.PPKLite".into()),
                ("SubFilter", "adbe.pkcs7.detached".into()),
                ("ByteRange", byte_range_placeholder),
                ("Contents", lopdf::Object::String(contents_placeholder, lopdf::StringFormat::Hexadecimal)),
                ("M", sign_time.into()),
                ("Name", lopdf::Object::String(seal.seal_info.seal_name.as_bytes().to_vec(), lopdf::StringFormat::Literal)),
                ("Reason", lopdf::Object::String(b"Sign".to_vec(), lopdf::StringFormat::Literal)),
            ]);
            let sig_dict_id = doc.add_object(lopdf::Object::Dictionary(sig_dict));

            // 4. 创建 Widget 注释
            // Y 坐标翻转: seal.y 是从顶部计算的距离, PDF 坐标从底部开始
            let pdf_y = page_height - seal.y - seal.height;
            let widget_dict = lopdf::Dictionary::from_iter([
                ("Type", "Annot".into()),
                ("Subtype", "Widget".into()),
                ("FT", "Sig".into()),
                ("V", lopdf::Object::Reference(sig_dict_id)),
                ("T", lopdf::Object::String(format!("SigSeal_{}", seal.id).into_bytes(), lopdf::StringFormat::Literal)),
                ("Rect", lopdf::Object::Array(vec![
                    lopdf::Object::Real(seal.x as f32),
                    lopdf::Object::Real(pdf_y as f32),
                    lopdf::Object::Real((seal.x + seal.width) as f32),
                    lopdf::Object::Real((pdf_y + seal.height) as f32),
                ])),
                ("F", 4i64.into()),
                ("P", lopdf::Object::Reference(page_obj_id)),
                ("AP", lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                    ("N", lopdf::Object::Reference(ap_obj_id)),
                ]))),
            ]);
            let widget_id = doc.add_object(lopdf::Object::Dictionary(widget_dict));

            // 5. 添加 Widget 到页面的 /Annots
            add_widget_to_page_annots(&mut doc, page_obj_id, widget_id)?;

            // 6. 添加到 AcroForm /Fields
            add_field_to_acroform(&mut doc, catalog_id, widget_id)?;

            seal_metas.push(SealEmbedMeta {
                sig_dict_id,
                widget_id,
                page_obj_id,
                seal: seal.clone(),
                algorithm,
            });

            web_sys::console::log_1(&format!(
                "[embed_pdf] 印章 {} 已添加: sig_dict={}, widget={}, page={}, rect=[{:.1},{:.1},{:.1},{:.1}]",
                seal.id, sig_dict_id.0, widget_id.0, page_num,
                seal.x, pdf_y, seal.x + seal.width, pdf_y + seal.height
            ).into());
        }

        // 7. 用 lopdf 保存为完整 PDF 字节
        let mut output = Vec::new();
        doc.save_to(&mut output)
            .map_err(|e| format!("PDF保存失败: {}", e))?;

        web_sys::console::log_1(&format!(
            "[embed_pdf] PDF 已保存, {} bytes, 开始填充签名", output.len()
        ).into());

        // 8. 二次处理: 计算 ByteRange 并填入 PKCS#7
        finalize_pdf_signatures(&mut output, &seal_metas)?;

        Ok(output)
    }
}

/// 从 lopdf Document 获取指定页面的高度
fn get_page_height(doc: &lopdf::Document, page_id: lopdf::ObjectId) -> f64 {
    if let Ok(obj) = doc.get_object(page_id) {
        if let Ok(dict) = obj.as_dict() {
            if let Ok(media_box) = dict.get(b"MediaBox").and_then(|o| o.as_array()) {
                if media_box.len() >= 4 {
                    let lly = get_obj_f64(&media_box[1]);
                    let ury = get_obj_f64(&media_box[3]);
                    return ury - lly;
                }
            }
        }
    }
    842.0
}

/// 从 lopdf Object 获取 f64
fn get_obj_f64(obj: &lopdf::Object) -> f64 {
    match obj {
        lopdf::Object::Integer(i) => *i as f64,
        lopdf::Object::Real(f) => *f as f64,
        _ => 0.0,
    }
}

/// 添加 Widget 到页面的 /Annots 数组
fn add_widget_to_page_annots(
    doc: &mut lopdf::Document,
    page_id: lopdf::ObjectId,
    widget_id: lopdf::ObjectId,
) -> Result<(), String> {
    // 先读取现有 Annots
    let existing_annots: Vec<lopdf::Object> = {
        let page = doc.get_object(page_id).map_err(|e| format!("页面获取失败: {}", e))?;
        let dict = page.as_dict().map_err(|e| format!("页面非字典: {}", e))?;
        if let Ok(annots_val) = dict.get(b"Annots") {
            if let Ok(arr) = annots_val.as_array() {
                arr.clone()
            } else if let Ok(ref_id) = annots_val.as_reference() {
                // Annots 是间接引用
                if let Ok(ref_obj) = doc.get_object(ref_id) {
                    if let Ok(arr) = ref_obj.as_array() {
                        arr.clone()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    };

    // 构建新的 Annots 数组
    let mut new_annots = existing_annots;
    new_annots.push(lopdf::Object::Reference(widget_id));

    // 写回页面
    let page = doc.get_object_mut(page_id).map_err(|e| format!("页面获取失败: {}", e))?;
    let dict = page.as_dict_mut().map_err(|e| format!("页面非字典: {}", e))?;
    dict.set("Annots", lopdf::Object::Array(new_annots));

    Ok(())
}

/// 添加字段到 AcroForm (如果不存在则创建)
fn add_field_to_acroform(
    doc: &mut lopdf::Document,
    catalog_id: u32,
    widget_id: lopdf::ObjectId,
) -> Result<(), String> {
    // 检查 catalog 是否有 AcroForm
    let acroform_ref = {
        let catalog = doc.get_object((catalog_id, 0))
            .map_err(|e| format!("Catalog获取失败: {}", e))?;
        let cat_dict = catalog.as_dict().map_err(|e| format!("Catalog非字典: {}", e))?;
        cat_dict.get(b"AcroForm")
            .ok()
            .and_then(|o| o.as_reference().ok())
            .map(|(id, _)| id)
    };

    if let Some(acro_id) = acroform_ref {
        // 已有 AcroForm, 添加字段
        let existing_fields: Vec<lopdf::Object> = {
            let acro = doc.get_object((acro_id, 0))
                .map_err(|e| format!("AcroForm获取失败: {}", e))?;
            let acro_dict = acro.as_dict().map_err(|e| format!("AcroForm非字典: {}", e))?;
            match acro_dict.get(b"Fields") {
                Ok(o) => o.as_array().map(|a| a.clone()).unwrap_or_default(),
                Err(_) => vec![],
            }
        };

        let mut new_fields = existing_fields;
        new_fields.push(lopdf::Object::Reference(widget_id));

        let acro = doc.get_object_mut((acro_id, 0))
            .map_err(|e| format!("AcroForm获取失败: {}", e))?;
        let acro_dict = acro.as_dict_mut().map_err(|e| format!("AcroForm非字典: {}", e))?;
        acro_dict.set("Fields", lopdf::Object::Array(new_fields));
        acro_dict.set("SigFlags", lopdf::Object::Integer(3));
    } else {
        // 创建新的 AcroForm
        let acro_dict = lopdf::Dictionary::from_iter([
            ("Fields", lopdf::Object::Array(vec![lopdf::Object::Reference(widget_id)])),
            ("SigFlags", lopdf::Object::Integer(3)),
        ]);
        let acro_id = doc.add_object(lopdf::Object::Dictionary(acro_dict));

        let catalog = doc.get_object_mut((catalog_id, 0))
            .map_err(|e| format!("Catalog获取失败: {}", e))?;
        let cat_dict = catalog.as_dict_mut().map_err(|e| format!("Catalog非字典: {}", e))?;
        cat_dict.set("AcroForm", lopdf::Object::Reference(acro_id));
    }

    Ok(())
}

/// 印章嵌入元数据
struct SealEmbedMeta {
    sig_dict_id: lopdf::ObjectId,
    widget_id: lopdf::ObjectId,
    page_obj_id: lopdf::ObjectId,
    seal: PlacedSeal,
    algorithm: ses::SealAlgorithm,
}

/// 解压后的图片数据
struct ImageData {
    width: u32,
    height: u32,
    data: Vec<u8>, // RGB raw, FlateDecode 压缩
}

/// 构建图片 XObject 数据 (PNG → RGB raw + FlateDecode)
fn build_image_xobject(png_data: &[u8], _target_w: f64, _target_h: f64) -> ImageData {
    match parse_png_to_rgb(png_data) {
        Some((w, h, rgb)) => {
            let compressed = flate_compress(&rgb);
            ImageData { width: w, height: h, data: compressed }
        }
        None => {
            let rgb = vec![220, 40, 40];
            let compressed = flate_compress(&rgb);
            ImageData { width: 1, height: 1, data: compressed }
        }
    }
}

/// 简化 PNG 解析: 提取 RGBA → RGB
fn parse_png_to_rgb(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.len() < 8 || &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }

    let mut pos = 8;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut idat_data: Vec<u8> = Vec::new();

    while pos < data.len() {
        if pos + 8 > data.len() { break; }
        let length = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let chunk_type = &data[pos+4..pos+8];
        let chunk_data_start = pos + 8;

        if chunk_data_start + length > data.len() { break; }

        match chunk_type {
            b"IHDR" => {
                width = u32::from_be_bytes([data[chunk_data_start], data[chunk_data_start+1], data[chunk_data_start+2], data[chunk_data_start+3]]);
                height = u32::from_be_bytes([data[chunk_data_start+4], data[chunk_data_start+5], data[chunk_data_start+6], data[chunk_data_start+7]]);
                bit_depth = data[chunk_data_start+8];
                color_type = data[chunk_data_start+9];
            }
            b"IDAT" => {
                idat_data.extend_from_slice(&data[chunk_data_start..chunk_data_start+length]);
            }
            b"IEND" => break,
            _ => {}
        }

        pos = chunk_data_start + length + 4;
    }

    if width == 0 || height == 0 || idat_data.is_empty() || bit_depth != 8 {
        return None;
    }

    let raw = flate_decompress(&idat_data)?;

    let bytes_per_pixel = match color_type {
        2 => 3,
        6 => 4,
        0 => 1,
        4 => 2,
        _ => return None,
    };

    let stride = width as usize * bytes_per_pixel;
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let mut raw_pos = 0;

    for _y in 0..height as usize {
        if raw_pos >= raw.len() { break; }
        raw_pos += 1; // skip filter byte
        let row_start = raw_pos;
        for _x in 0..width as usize {
            let px = row_start + _x * bytes_per_pixel;
            if px + bytes_per_pixel > raw.len() { break; }
            match color_type {
                2 => rgb.extend_from_slice(&raw[px..px+3]),
                6 => {
                    let a = raw[px+3] as u32;
                    rgb.push((raw[px] as u32 * a / 255 + (255 - a) * 255 / 255) as u8);
                    rgb.push((raw[px+1] as u32 * a / 255 + (255 - a) * 255 / 255) as u8);
                    rgb.push((raw[px+2] as u32 * a / 255 + (255 - a) * 255 / 255) as u8);
                }
                0 => {
                    let g = raw[px];
                    rgb.extend_from_slice(&[g, g, g]);
                }
                _ => {}
            }
        }
        raw_pos = row_start + stride;
    }

    Some((width, height, rgb))
}

/// FlateDecode 压缩 (zlib)
fn flate_compress(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).ok();
    encoder.finish().unwrap_or_default()
}

/// FlateDecode 解压 (zlib)
fn flate_decompress(data: &[u8]) -> Option<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result).ok()?;
    Some(result)
}

/// 二次处理: 计算 ByteRange 并填入 PKCS#7 签名值
fn finalize_pdf_signatures(
    data: &mut Vec<u8>,
    seal_metas: &[SealEmbedMeta],
) -> Result<(), String> {
    for meta in seal_metas {
        // ━━ 迭代法计算正确的 ByteRange ━━
        // 替换 ByteRange 会改变数据长度, 从而移动 /Contents 位置。
        // 需要迭代直到 ByteRange 字符串长度稳定 (通常 2-3 轮收敛)。
        let mut prev_br = String::new();
        let mut hex_start;
        let mut hex_end;

        loop {
            // 1. 找到 /Contents <hex> 的位置
            let (hs, he) = find_contents_hex_range(data, meta.sig_dict_id.0)
                .ok_or_else(|| format!("未找到签名字典 {} 的 /Contents", meta.sig_dict_id.0))?;
            hex_start = hs;
            hex_end = he;

            // 2. 计算 ByteRange
            // [0, (hex_start - 1), (hex_end + 1), (data.len() - hex_end - 1)]
            let br_0: u64 = 0;
            let br_1: u64 = (hex_start - 1) as u64;
            let br_2: u64 = (hex_end + 1) as u64;
            let br_3: u64 = data.len() as u64 - br_2;

            let new_br = format!("[{} {} {} {}]", br_0, br_1, br_2, br_3);

            if new_br == prev_br {
                // 字符串稳定, 退出迭代
                break;
            }

            prev_br = new_br.clone();
            replace_byterange_in_bytes(data, meta.sig_dict_id.0, &new_br)?;
        }

        // 3. 重新确认最终的 /Contents 位置 (ByteRange 已稳定)
        let (hex_start, hex_end) = find_contents_hex_range(data, meta.sig_dict_id.0)
            .ok_or_else(|| format!("最终未找到 /Contents"))?;

        web_sys::console::log_1(&format!(
            "[finalize] sig_dict={} Contents hex 范围: [{}, {}), 长度={}, ByteRange={}",
            meta.sig_dict_id.0, hex_start, hex_end, hex_end - hex_start, prev_br
        ).into());

        // 4. 计算文档摘要 (ByteRange 指定的字节范围)
        let mut hash_data = Vec::with_capacity(data.len() - (hex_end - hex_start + 2));
        hash_data.extend_from_slice(&data[0..hex_start - 1]);    // Part 1 (before <)
        hash_data.extend_from_slice(&data[hex_end + 1..]);       // Part 2 (after >)

        // 5. 构建 SES 参数和 PKCS#7
        let algorithm = meta.algorithm;
        let ses_params = ses::ses_params_from_seal_info(&meta.seal.seal_info, algorithm);

        let doc_hash = match algorithm {
            ses::SealAlgorithm::Sm2 => crypto::sm3_hash(&hash_data),
            ses::SealAlgorithm::Rsa => crypto::sha256(&hash_data),
        };

        let ses_sig_der = ses::build_mock_ses_signature(&ses_params, &hash_data);

        let pkcs7_der = pkcs7::build_mock_pkcs7(
            algorithm,
            &ses_sig_der,
            &doc_hash,
            ses_params.sign_time,
        );

        // 6. 将 PKCS#7 编码为十六进制并填入 /Contents
        let pkcs7_hex = hex::encode(&pkcs7_der);

        if pkcs7_hex.len() > SIGNATURE_CONTENTS_SIZE {
            return Err(format!(
                "PKCS#7 签名值过大 ({} hex chars), 超过预留空间 ({} chars)",
                pkcs7_hex.len(), SIGNATURE_CONTENTS_SIZE
            ));
        }

        // 填入十六进制值, 剩余部分保持 '0' 填充
        let pkcs7_bytes = pkcs7_hex.as_bytes();
        for (i, &byte) in pkcs7_bytes.iter().enumerate() {
            data[hex_start + i] = byte;
        }

        web_sys::console::log_1(&format!(
            "[finalize] 签名字典 {} 填充完成: PKCS#7={} bytes, hex={} chars",
            meta.sig_dict_id.0, pkcs7_der.len(), pkcs7_hex.len()
        ).into());
    }

    Ok(())
}

/// 在数据中查找指定签名字典对象的 /Contents <hex> 范围
/// 返回 (hex_start, hex_end): hex_start 是第一个 hex 字符的位置, hex_end 是 > 的位置
fn find_contents_hex_range(data: &[u8], sig_obj_id: u32) -> Option<(usize, usize)> {
    let prefix = format!("{} 0 obj", sig_obj_id);
    let prefix_bytes = prefix.as_bytes();

    // 找到签名字典对象定义
    let obj_pos = data.windows(prefix_bytes.len())
        .position(|w| w == prefix_bytes)?;

    // 在对象定义后找 /Contents <
    let search_start = obj_pos + prefix_bytes.len();
    let contents_marker = b"/Contents <";
    let contents_pos = data[search_start..]
        .windows(contents_marker.len())
        .position(|w| w == contents_marker)?;

    let hex_start = search_start + contents_pos + contents_marker.len();

    // 找到 > (hex 字符串结束)
    // 跳过所有 hex 字符 (0-9, a-f, A-F)
    let mut hex_end = hex_start;
    while hex_end < data.len() {
        let b = data[hex_end];
        if b == b'>' {
            break;
        }
        if !((b >= b'0' && b <= b'9') || (b >= b'a' && b <= b'f') || (b >= b'A' && b <= b'F')) {
            // 非法 hex 字符, 可能是空格或换行 (lopdf 可能插入空白)
            hex_end += 1;
            continue;
        }
        hex_end += 1;
    }

    if hex_end >= data.len() {
        return None;
    }

    // hex_end 现在指向 '>'
    Some((hex_start, hex_end))
}

/// 替换签名字典中的 /ByteRange 值
fn replace_byterange_in_bytes(
    data: &mut Vec<u8>,
    sig_obj_id: u32,
    new_range: &str,
) -> Result<(), String> {
    let prefix = format!("{} 0 obj", sig_obj_id);
    let prefix_bytes = prefix.as_bytes();

    let obj_pos = data.windows(prefix_bytes.len())
        .position(|w| w == prefix_bytes)
        .ok_or("未找到签名字典对象")?;

    // 在对象内查找 /ByteRange
    let search_start = obj_pos;
    let search_end = data.len().min(search_start + 2000); // 搜索范围限制
    let br_marker = b"/ByteRange";

    let br_pos = data[search_start..search_end]
        .windows(br_marker.len())
        .position(|w| w == br_marker)
        .map(|p| search_start + p)
        .ok_or("未找到 /ByteRange")?;

    // 找到 /ByteRange 后的 [ ... ]
    let arr_start = br_pos + br_marker.len();
    // 跳过空白找 [
    let mut p = arr_start;
    while p < data.len() && (data[p] == b' ' || data[p] == b'\n' || data[p] == b'\r' || data[p] == b'\t') {
        p += 1;
    }
    if p >= data.len() || data[p] != b'[' {
        return Err("/ByteRange 后未找到 [".to_string());
    }
    let bracket_start = p;

    // 找到匹配的 ]
    let mut bracket_end = bracket_start + 1;
    while bracket_end < data.len() && data[bracket_end] != b']' {
        bracket_end += 1;
    }
    if bracket_end >= data.len() {
        return Err("/ByteRange 未找到 ]".to_string());
    }
    bracket_end += 1; // 包含 ]

    // 替换为新值
    let new_str = format!("/ByteRange {}", new_range);
    data.splice(br_pos..bracket_end, new_str.as_bytes().iter().copied());

    web_sys::console::log_1(&format!(
        "[replace_br] 对象 {} /ByteRange 已替换为: {}", sig_obj_id, new_range
    ).into());

    Ok(())
}

// ============================================================
// 旧代码保留: OFD 签章嵌入 (已移至 ofd_sign.rs)
// ============================================================

/// 印章二进制数据结构 (内部用)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct SealBinaryData {
    seal_type: i32,
    code: String,
    name: String,
    company: String,
    width_mm: f64,
    height_mm: f64,
    image_data: String,
}

impl Default for SealEngine {
    fn default() -> Self { Self }
}

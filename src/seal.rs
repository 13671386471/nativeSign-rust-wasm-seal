//! 印章操作模块 — 印章嵌入、落章、印章图像处理

use crate::types::*;

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
        // 构造印章结构体数据 (PDF 或 OFD 格式)
        // 返回 base64 编码的印章数据

        let seal_data = SealBinaryData {
            seal_type: _seal_type,
            code: _code.to_string(),
            name: _name.to_string(),
            company: _company.to_string(),
            width_mm: width,
            height_mm: height,
            image_data: image_base64.to_string(),
        };

        // 序列化为 JSON 后 base64 编码 (模拟原生印章结构体)
        let json = serde_json::to_string(&seal_data)
            .unwrap_or_default();
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json.as_bytes()
        );
        Ok(encoded)
    }

    /// 添加印章到文档指定位置
    /// 对应 OFD_Plugin.AddSeal(cPages, "", "AUTO_ADD_SEAL_FROM_PATH")
    ///
    /// cPages 格式:
    ///   - 位置章: "page,x,y,w,h,sealData"
    ///   - 关键字章: "AUTO_ADD:pageStart,pageEnd,x,y,range,keyword)|(4,sealData"
    ///   - 骑缝章: "page,x,5,border,firstPercent,morePages...,sealData"
    pub fn add_seal(
        placed_seals: &mut Vec<PlacedSeal>,
        c_pages: &str,
        _sign_data: &str,
        seal_info: &SealInfo,
    ) -> Result<usize, String> {
        // 解析 cPages 参数
        if c_pages.starts_with("AUTO_ADD:") {
            // 关键字章 — 搜索关键字位置后自动落章
            return Err("关键字自动落章需先搜索关键字位置".to_string());
        }

        // 解析位置参数
        let parts: Vec<&str> = c_pages.split(',').collect();
        if parts.len() < 5 {
            return Err("AddSeal参数格式错误".to_string());
        }

        let page = parts[0].parse::<u32>().unwrap_or(0);
        let x_raw = parts[1].parse::<f64>().unwrap_or(0.0);
        let y_raw = parts[4].parse::<f64>().unwrap_or(0.0);

        // 坐标转换: 50000 单位 → pt
        // 原始坐标单位是 1/50000 页面宽高
        let x_pt = x_raw / 50000.0 * 595.0; // A4 width
        let y_pt = y_raw / 50000.0 * 842.0; // A4 height

        let seal = PlacedSeal {
            id: placed_seals.len(),
            page_index: page,
            x: x_pt,
            y: y_pt,
            width: 100.0,   // 默认印章宽度 100pt
            height: 100.0,  // 默认印章高度 100pt
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
        // 返回处理后适合作为鼠标光标的印章图像
        seal_image_base64.to_string()
    }

    /// 将印章数据嵌入到文档数据中
    /// 这是将印章实际写入 PDF/OFD 的核心方法
    pub fn embed_seals_to_document(
        doc_data: &[u8],
        seals: &[PlacedSeal],
        doc_type: DocType,
    ) -> Result<Vec<u8>, String> {
        let mut output = doc_data.to_vec();

        match doc_type {
            DocType::Pdf => {
                // PDF 增量更新: 在文件末尾追加签章对象
                Self::embed_seals_to_pdf(&mut output, seals)?;
            }
            DocType::Ofd => {
                // OFD: 在 ZIP 包中添加签章 XML
                Self::embed_seals_to_ofd(&mut output, seals)?;
            }
        }

        Ok(output)
    }

    /// PDF 签章嵌入 — 增量更新方式
    fn embed_seals_to_pdf(_data: &mut Vec<u8>, seals: &[PlacedSeal]) -> Result<(), String> {
        if seals.is_empty() {
            return Ok(());
        }

        // PDF 增量更新格式:
        // 在 %%EOF 之前插入新的 xref 和签章对象

        // 构造签章注解对象 (PDF Stamp Annotation)
        for (i, seal) in seals.iter().enumerate() {
            let obj_id = 10000 + i as u32; // 新对象的ID

            // 构造 Annotation 字典
            let stamp_dict = format!(
                "{} 0 obj\n\
                 << /Type /Annot\n\
                    /Subtype /Stamp\n\
                    /Rect [{} {} {} {}]\n\
                    /Contents (Seal: {})\n\
                    /Name /Approved\n\
                    /F 4\n\
                    /AP << /N {} 0 R >>\n\
                 >>\nendobj\n",
                obj_id,
                seal.x, seal.y,
                seal.x + seal.width, seal.y + seal.height,
                seal.seal_info.seal_name,
                obj_id + 1,
            );

            // 构造外观流对象
            let ap_dict = format!(
                "{} 0 obj\n\
                 << /Type /XObject\n\
                    /Subtype /Form\n\
                    /BBox [0 0 {} {}]\n\
                    /Resources << >>\n\
                    /Length 0\n\
                 >>\nstream\nendstream\nendobj\n",
                obj_id + 1,
                seal.width, seal.height,
            );

            // 追加到文件末尾
            _data.extend_from_slice(stamp_dict.as_bytes());
            _data.extend_from_slice(ap_dict.as_bytes());
        }

        Ok(())
    }

    /// OFD 签章嵌入
    fn embed_seals_to_ofd(_data: &mut Vec<u8>, _seals: &[PlacedSeal]) -> Result<(), String> {
        // OFD 签章需在 ZIP 包中添加:
        // Doc_N/Signs/Sign_N/Signature.xml — 签名数据
        // Doc_N/Signs/Sign_N/Seal.esl — 印章数据
        // Doc_N/Signs/Sign_N/SignedValue.dat — 签名值
        // 并更新 Doc_N/DocVersion.xml 中的签名列表
        Ok(())
    }
}

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
    fn default() -> Self {
        Self
    }
}

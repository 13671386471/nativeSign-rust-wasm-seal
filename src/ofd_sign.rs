//! OFD 签章嵌入 (GB/T 33190-2016)
//!
//! 在 OFD 文档 (ZIP 包) 中添加签章:
//!
//! ```text
//! OFD.zip
//! ├── OFD.xml                          ← 更新: 添加 Signs 引用
//! ├── Doc_0/
//! │   ├── Document.xml                 ← 计算摘要引用
//! │   ├── Pages/Page_0/Content.xml    ← 计算摘要引用
//! │   └── Signs/
//! │       └── Sign_0/
//! │           ├── Sign_0.xml          ← 新增: 签章描述 XML
//! │           ├── Seal.esl             ← 新增: SES_Seal DER 编码
//! │           └── SignedValue.dat      ← 新增: PKCS#7 签名值
//! ```
//!
//! Sign_N.xml 结构 (GB/T 33190-2016 第 18 章 签章):
//! ```xml
//! <ofd:Signatures xmlns:ofd="http://www.ofdspec.org/2016">
//!   <ofd:Signature ID="s1" Type="Seal">
//!     <ofd:SignedData>
//!       <ofd:Provider ProviderName="..."/>
//!       <ofd:SignatureMethod Algorithm="1.2.156.10197.1.501"/>
//!       <ofd:SignatureDateTime>2026-07-10T16:30:00+08:00</ofd:SignatureDateTime>
//!       <ofd:References>
//!         <ofd:Ref ID="r1" Type="OFD" FileRef="Doc_0/Document.xml">
//!           <ofd:CheckValue>base64(sm3_hash)</ofd:CheckValue>
//!         </ofd:Ref>
//!       </ofd:References>
//!       <ofd:StampAnnot>
//!         <ofd:PageRef PageRef="0"/>
//!         <ofd:ID>stamp-001</ofd:ID>
//!         <ofd:Boundary x="100" y="100" w="100" h="100"/>
//!         <ofd:RefType>OFD</ofd:RefType>
//!         <ofd:FileRef>Doc_0/Pages/Page_0/Content.xml</ofd:FileRef>
//!       </ofd:StampAnnot>
//!     </ofd:SignedData>
//!     <ofd:SignedValue FileRef="SignedValue.dat"/>
//!     <ofd:Seal>
//!       <ofd:SealObj Type="OFD" BaseLoc="Doc_0/Signs/Sign_0/Seal.esl"/>
//!     </ofd:Seal>
//!   </ofd:Signature>
//! </ofd:Signatures>
//! ```

use crate::types::*;
use crate::ses;
use crate::pkcs7;
use crate::crypto;
use std::io::{Read, Write, Seek, SeekFrom};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// OFD XML 命名空间
const OFD_NS: &str = "http://www.ofdspec.org/2016";

/// 嵌入签章到 OFD 文档
pub fn embed_seals_to_ofd(data: &[u8], seals: &[PlacedSeal]) -> Result<Vec<u8>, String> {
    if seals.is_empty() {
        return Ok(data.to_vec());
    }

    // 1. 读取 ZIP 中的现有文件
    let cursor = std::io::Cursor::new(data.to_vec());
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| format!("OFD ZIP解析失败: {}", e))?;

    // 收集所有文件内容
    let mut files: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    let mut file_names: Vec<String> = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("读取ZIP条目失败: {}", e))?;
        let name = file.name().to_string();
        let mut content = Vec::new();
        file.read_to_end(&mut content).map_err(|e| format!("读取文件内容失败: {}", e))?;
        file_names.push(name.clone());
        files.insert(name, content);
    }

    // 2. 查找文档根路径 (通常 Doc_0/)
    let doc_root = find_doc_root(&files)?;

    // 3. 对每枚印章, 创建签章结构
    for (idx, seal) in seals.iter().enumerate() {
        let sign_id = format!("Sign_{}", idx);
        let sign_dir = format!("{}/Signs/{}", doc_root, sign_id);

        // 3a. 计算 Document.xml 的摘要
        let doc_xml_path = format!("{}/Document.xml", doc_root);
        let doc_xml_data = files.get(&doc_xml_path)
            .ok_or_else(|| format!("未找到 {}", doc_xml_path))?;
        let doc_xml_hash = crypto::sm3_hash(doc_xml_data);

        // 3b. 计算页面 Content.xml 的摘要
        let page_xml_path = format!("{}/Pages/Page_{}/Content.xml", doc_root, seal.page_index);
        let page_xml_hash = files.get(&page_xml_path)
            .map(|d| crypto::sm3_hash(d))
            .unwrap_or_default();

        // 3c. 构建 SES 参数
        let algorithm = ses::SealAlgorithm::Sm2;
        let ses_params = ses::ses_params_from_seal_info(&seal.seal_info, algorithm);

        // 3d. 构建 Seal.esl (SES_Seal DER 编码)
        let seal_esl_der = ses::build_mock_ses_seal(&ses_params);
        files.insert(format!("{}/Seal.esl", sign_dir), seal_esl_der);

        // 3e. 构建 SignedValue.dat (PKCS#7 SignedData)
        // 待签名数据 = 签章描述 XML 的摘要
        let sign_xml = build_sign_xml(
            &sign_dir, &doc_root, seal, idx,
            &doc_xml_hash, &page_xml_hash,
            &ses_params,
        );
        let sign_xml_bytes = sign_xml.as_bytes();
        let sign_xml_hash = crypto::sm3_hash(sign_xml_bytes);

        let ses_sig_der = ses::build_mock_ses_signature(&ses_params, sign_xml_bytes);
        let pkcs7_der = pkcs7::build_mock_pkcs7(
            algorithm,
            &ses_sig_der,
            &sign_xml_hash,
            ses_params.sign_time,
        );
        files.insert(format!("{}/SignedValue.dat", sign_dir), pkcs7_der);

        // 3f. 保存 Sign_N.xml
        files.insert(format!("{}/{}.xml", sign_dir, sign_id), sign_xml_bytes.to_vec());
    }

    // 4. 更新 OFD.xml, 添加 Signs 引用
    let ofd_xml_path = "OFD.xml";
    if let Some(ofd_xml_data) = files.get(ofd_xml_path).cloned() {
        let ofd_xml = String::from_utf8_lossy(&ofd_xml_data).to_string();
        let updated_ofd_xml = update_ofd_xml(&ofd_xml, &doc_root, seals.len());
        files.insert(ofd_xml_path.to_string(), updated_ofd_xml.into_bytes());
    }

    // 5. 重新打包 ZIP
    let mut output = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut output));
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // 写入所有文件 (保持原始顺序)
        for name in &file_names {
            if let Some(content) = files.get(name) {
                writer.start_file(name, options).map_err(|e| format!("ZIP写入失败: {}", e))?;
                writer.write_all(content).map_err(|e| format!("ZIP写入失败: {}", e))?;
            }
        }

        // 写入新增的签章文件
        for (idx, _seal) in seals.iter().enumerate() {
            let sign_id = format!("Sign_{}", idx);
            let sign_dir = format!("{}/Signs/{}", doc_root, sign_id);

            // Seal.esl
            let seal_path = format!("{}/Seal.esl", sign_dir);
            if !file_names.contains(&seal_path) {
                if let Some(content) = files.get(&seal_path) {
                    writer.start_file(&seal_path, options).map_err(|e| format!("ZIP写入失败: {}", e))?;
                    writer.write_all(content).map_err(|e| format!("ZIP写入失败: {}", e))?;
                }
            }

            // SignedValue.dat
            let sv_path = format!("{}/SignedValue.dat", sign_dir);
            if !file_names.contains(&sv_path) {
                if let Some(content) = files.get(&sv_path) {
                    writer.start_file(&sv_path, options).map_err(|e| format!("ZIP写入失败: {}", e))?;
                    writer.write_all(content).map_err(|e| format!("ZIP写入失败: {}", e))?;
                }
            }

            // Sign_N.xml
            let sign_xml_path = format!("{}/{}.xml", sign_dir, sign_id);
            if !file_names.contains(&sign_xml_path) {
                if let Some(content) = files.get(&sign_xml_path) {
                    writer.start_file(&sign_xml_path, options).map_err(|e| format!("ZIP写入失败: {}", e))?;
                    writer.write_all(content).map_err(|e| format!("ZIP写入失败: {}", e))?;
                }
            }
        }

        writer.finish().map_err(|e| format!("ZIP打包失败: {}", e))?;
    }

    Ok(output)
}

/// 查找 OFD 文档根路径 (如 "Doc_0")
fn find_doc_root(files: &std::collections::HashMap<String, Vec<u8>>) -> Result<String, String> {
    // 优先从 OFD.xml 解析 DocRoot
    if let Some(ofd_xml_data) = files.get("OFD.xml") {
        let ofd_xml = String::from_utf8_lossy(ofd_xml_data);
        // 查找 <DocRoot>Doc_0/Document.xml</DocRoot>
        if let Some(start) = ofd_xml.find("<ofd:DocRoot>") {
            let content_start = start + "<ofd:DocRoot>".len();
            if let Some(end) = ofd_xml[content_start..].find("</ofd:DocRoot>") {
                let doc_root_path = &ofd_xml[content_start..content_start + end];
                // 提取 Doc_0 部分
                if let Some(slash_pos) = doc_root_path.find('/') {
                    return Ok(doc_root_path[..slash_pos].to_string());
                }
                return Ok(doc_root_path.to_string());
            }
        }
        // 也尝试不带命名空间前缀
        if let Some(start) = ofd_xml.find("<DocRoot>") {
            let content_start = start + "<DocRoot>".len();
            if let Some(end) = ofd_xml[content_start..].find("</DocRoot>") {
                let doc_root_path = &ofd_xml[content_start..content_start + end];
                if let Some(slash_pos) = doc_root_path.find('/') {
                    return Ok(doc_root_path[..slash_pos].to_string());
                }
                return Ok(doc_root_path.to_string());
            }
        }
    }

    // 回退: 查找 Doc_N/Document.xml
    for name in files.keys() {
        if name.ends_with("/Document.xml") && name.contains("Doc_") {
            if let Some(slash_pos) = name.find('/') {
                return Ok(name[..slash_pos].to_string());
            }
        }
    }

    Err("未找到OFD文档根路径".to_string())
}

/// 构建签章描述 XML (Sign_N.xml)
fn build_sign_xml(
    sign_dir: &str,
    doc_root: &str,
    seal: &PlacedSeal,
    sign_index: usize,
    doc_xml_hash: &[u8],
    page_xml_hash: &[u8],
    ses_params: &ses::SesParams,
) -> String {
    let doc_xml_hash_b64 = BASE64.encode(doc_xml_hash);
    let page_xml_hash_b64 = BASE64.encode(page_xml_hash);

    let sign_time = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+08:00",
        ses_params.sign_time.0, ses_params.sign_time.1, ses_params.sign_time.2,
        ses_params.sign_time.3, ses_params.sign_time.4, ses_params.sign_time.5
    );

    let sign_id = format!("Sign_{}", sign_index);
    let seal_esl_path = format!("{}/Seal.esl", sign_dir);
    let signed_value_path = "SignedValue.dat";
    let doc_xml_ref = format!("{}/Document.xml", doc_root);
    let page_xml_ref = format!("{}/Pages/Page_{}/Content.xml", doc_root, seal.page_index);

    // 坐标转换: pt → mm (1pt = 0.3528mm)
    let x_mm = seal.x * 0.3528;
    let y_mm = seal.y * 0.3528;
    let w_mm = seal.width * 0.3528;
    let h_mm = seal.height * 0.3528;

    let signature_method = ses_params.algorithm.signature_oid();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ofd:Signatures xmlns:ofd="{ns}">
  <ofd:Signature ID="s{idx}" Type="Seal">
    <ofd:SignedData>
      <ofd:Provider ProviderName="DianJu WASM Seal Engine" Version="1.0"/>
      <ofd:SignatureMethod Algorithm="{method}"/>
      <ofd:SignatureDateTime>{time}</ofd:SignatureDateTime>
      <ofd:References>
        <ofd:Ref ID="r1" Type="OFD" FileRef="{doc_ref}">
          <ofd:CheckValue>{doc_hash}</ofd:CheckValue>
        </ofd:Ref>
        <ofd:Ref ID="r2" Type="OFD" FileRef="{page_ref}">
          <ofd:CheckValue>{page_hash}</ofd:CheckValue>
        </ofd:Ref>
      </ofd:References>
      <ofd:StampAnnot>
        <ofd:PageRef PageRef="{page}"/>
        <ofd:ID>stamp-{idx:03}</ofd:ID>
        <ofd:Boundary x="{x:.2}" y="{y:.2}" w="{w:.2}" h="{h:.2}"/>
        <ofd:RefType>OFD</ofd:RefType>
        <ofd:FileRef>{page_ref}</ofd:FileRef>
      </ofd:StampAnnot>
    </ofd:SignedData>
    <ofd:SignedValue FileRef="{sv_path}"/>
    <ofd:Seal>
      <ofd:SealObj Type="OFD" BaseLoc="{seal_path}"/>
    </ofd:Seal>
  </ofd:Signature>
</ofd:Signatures>
"#,
        ns = OFD_NS,
        idx = sign_index,
        method = signature_method,
        time = sign_time,
        doc_ref = doc_xml_ref,
        doc_hash = doc_xml_hash_b64,
        page_ref = page_xml_ref,
        page_hash = page_xml_hash_b64,
        page = seal.page_index,
        x = x_mm, y = y_mm, w = w_mm, h = h_mm,
        sv_path = signed_value_path,
        seal_path = seal_esl_path,
    )
}

/// 更新 OFD.xml, 添加 Signs 引用
fn update_ofd_xml(ofd_xml: &str, doc_root: &str, seal_count: usize) -> String {
    // 构建 Signs 引用
    let mut signs_xml = String::new();
    for i in 0..seal_count {
        signs_xml.push_str(&format!(
            "    <ofd:SignID>{}/Signs/Sign_{}/Sign_{}.xml</ofd:SignID>\n",
            doc_root, i, i
        ));
    }

    // 检查是否已有 <Signs> 标签
    if ofd_xml.contains("<ofd:Signs>") || ofd_xml.contains("<Signs>") {
        // 已有 Signs, 追加新的 SignID
        // 简化: 在 </Signs> 前插入
        let updated = ofd_xml
            .replace("</ofd:Signs>", &format!("{}\n  </ofd:Signs>", signs_xml.trim_end()))
            .replace("</Signs>", &format!("{}\n  </Signs>", signs_xml.trim_end()));
        return updated;
    }

    // 没有 Signs, 在 DocBody 中添加
    // 查找 </ofd:DocBody> 或 </DocBody>
    let signs_block = format!(
        "    <ofd:Signs>\n{}\n    </ofd:Signs>\n  ",
        signs_xml.trim_end()
    );

    let updated = ofd_xml
        .replace("</ofd:DocBody>", &format!("{}  </ofd:DocBody>", signs_block))
        .replace("</DocBody>", &format!("{}  </DocBody>", signs_block));

    if updated != ofd_xml {
        return updated;
    }

    // 如果没有 DocBody, 在 </ofd:OFD> 前添加
    format!("{}\n  <ofd:DocBody>\n    <ofd:Signs>\n{}\n    </ofd:Signs>\n  </ofd:DocBody>\n",
        ofd_xml.trim_end_matches("</ofd:OFD>"),
        signs_xml.trim_end()
    ) + "</ofd:OFD>"
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_sign_xml() {
        let seal = PlacedSeal {
            id: 0,
            page_index: 0,
            x: 100.0,
            y: 200.0,
            width: 100.0,
            height: 100.0,
            seal_info: SealInfo {
                origin: "cloud".to_string(),
                seal_id: "test-001".to_string(),
                seal_name: "测试印章".to_string(),
                width: 100.0,
                height: 100.0,
                seal_type: Some(1),
                seal_image: String::new(),
                sign_cert_sn: None,
                sign_data: None,
                sign_cert: None,
                seal_start_time: None,
                seal_end_time: None,
                signer_name: None,
                sign_time: None,
                sign_method: None,
                cert_issuer: None,
                cert_subject: None,
                cert_start_time: None,
                cert_end_time: None,
                cert_algorithm: None,
                cert_data: None,
            },
            signature: None,
            signed: false,
        };

        let params = ses::SesParams::default();
        let doc_hash = crypto::sm3_hash(b"<Document/>");
        let page_hash = crypto::sm3_hash(b"<Page/>");

        let xml = build_sign_xml(
            "Doc_0/Signs/Sign_0", "Doc_0", &seal, 0,
            &doc_hash, &page_hash, &params
        );

        assert!(xml.contains("ofd:Signatures"));
        assert!(xml.contains("ofd:Signature"));
        assert!(xml.contains("Seal.esl"));
        assert!(xml.contains("SignedValue.dat"));
        assert!(xml.contains("StampAnnot"));
    }
}

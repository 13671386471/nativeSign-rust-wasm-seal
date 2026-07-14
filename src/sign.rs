//! 数字签名模块 — 签名值生成、验证、签章合成
//!
//! 集成 SES 签章结构 (GM/T 0031-2014):
//! - get_value_ex() 返回 TBS_Sign 的摘要 (SM3/SHA256)
//! - set_value_ex() 构建 SES_Signature + PKCS#7 SignedData
//! - finalize_signature() 将签章嵌入文档
//!
//! ⚠️ MOCK 说明:
//! - 云签签名通过 HTTP 请求服务端完成，此处模拟服务端响应
//! - UKey 硬件签名通过 WebSocket 代理完成，此处模拟硬件交互
//! - 证书/密钥使用 MOCK 数据 (见 ses.rs)

use crate::crypto;
use crate::ses;
use crate::pkcs7;
use crate::types::*;
use std::collections::HashMap;

/// 签名引擎 — 管理签名计算、签章合成、签名值写入
pub struct SignEngine {
    /// 当前会话使用的证书公钥 (base64)
    pub current_cert: Option<String>,
    /// 签名模式配置
    settings: HashMap<String, String>,
    /// 最后一次错误码
    last_error: i32,
    /// 最后一次错误信息
    last_error_string: String,
    /// 待签名文档数据 (在 get_value_ex 调用前设置)
    pending_doc_data: Option<Vec<u8>>,
    /// 待签名印章信息
    pending_seal_info: Option<SealInfo>,
    /// 已构建的 PKCS#7 签名值 (set_value_ex 后存储)
    pending_pkcs7: Option<Vec<u8>>,
    /// 签章算法
    algorithm: ses::SealAlgorithm,
}

impl SignEngine {
    pub fn new() -> Self {
        let mut settings = HashMap::new();
        settings.insert("SET_JAVA_SM2MODE".to_string(), "0".to_string());
        settings.insert("SET_PDFSEAL_ALPHA".to_string(), "180".to_string());
        settings.insert("SET_SKFKEY_PIN".to_string(), String::new());

        Self {
            current_cert: None,
            settings,
            last_error: 0,
            last_error_string: String::new(),
            pending_doc_data: None,
            pending_seal_info: None,
            pending_pkcs7: None,
            algorithm: ses::SealAlgorithm::Sm2,
        }
    }

    /// 准备签名 — 在调用 get_value_ex 之前设置文档数据和印章信息
    pub fn prepare_for_signing(&mut self, doc_data: Vec<u8>, seal_info: SealInfo) {
        self.pending_doc_data = Some(doc_data);
        self.pending_seal_info = Some(seal_info);
        self.pending_pkcs7 = None;
    }

    /// 设置签章算法
    pub fn set_algorithm(&mut self, algorithm: ses::SealAlgorithm) {
        self.algorithm = algorithm;
    }

    /// 获取已构建的 PKCS#7 签名值 (用于嵌入文档)
    pub fn get_pending_pkcs7(&self) -> Option<&[u8]> {
        self.pending_pkcs7.as_deref()
    }

    /// 设置全局配置值
    pub fn set_value(&mut self, key: &str, value: &str) {
        match key {
            "SET_JAVA_SM2MODE" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            "SET_PDFSEAL_ALPHA" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            "ADD_FORCETYPE_VALUE8" => {
                self.settings.insert("force_type".to_string(), "8".to_string());
                self.settings.insert(key.to_string(), value.to_string());
            }
            "DEL_FORCETYPE_VALUE8" => {
                self.settings.remove("force_type");
                self.settings.insert(key.to_string(), value.to_string());
            }
            "ADD_FORCETYPE_VALUE7" => {
                self.settings.insert("force_type".to_string(), "7".to_string());
                self.settings.insert(key.to_string(), value.to_string());
            }
            "DEL_FORCETYPE_VALUE" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            "SET_UTF8_MODE" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            "SET_JMJSERVER_GMCERTDATA" => {
                self.current_cert = Some(value.to_string());
                self.settings.insert(key.to_string(), value.to_string());
            }
            "SET_SKFKEY_PIN" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            "SET_CURRENT_COOKIE" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            _ => {
                self.settings.insert(key.to_string(), value.to_string());
            }
        }
    }

    /// 获取全局配置值
    pub fn get_value(&self, key: &str) -> Option<String> {
        match key {
            "GET_CURRENT_CERT" => self.current_cert.clone(),
            "GET_LAST_SEAL" => None,
            _ => self.settings.get(key).cloned(),
        }
    }

    /// 获取扩展值（签名相关）
    /// 对应 OFD_Plugin.GetValueEx("GET_AIPSIGN_ORIDATA", type, "", 0, "")
    ///
    /// 返回 TBS_Sign 结构的 DER 编码的摘要值
    ///
    /// lType 含义:
    ///   2 = SM2 + 国办标准章
    ///   3 = SM2 + P7章
    pub fn get_value_ex(&mut self, key: &str, l_type: i32) -> Result<String, String> {
        if key != "GET_AIPSIGN_ORIDATA" {
            return Err(format!("未知的 GetValueEx key: {}", key));
        }

        // 根据签章格式类型选择算法
        let algorithm = match l_type {
            2 | 3 => {
                self.algorithm = ses::SealAlgorithm::Sm2;
                ses::SealAlgorithm::Sm2
            }
            _ => {
                self.algorithm = ses::SealAlgorithm::Rsa;
                ses::SealAlgorithm::Rsa
            }
        };

        // 获取文档数据
        let doc_data = self.pending_doc_data.as_ref()
            .ok_or_else(|| "未设置待签名文档数据, 请先调用 prepare_for_signing".to_string())?;

        // 获取印章信息
        let seal_info = self.pending_seal_info.as_ref()
            .ok_or_else(|| "未设置印章信息".to_string())?;

        // 构建 SES 参数
        let ses_params = ses::ses_params_from_seal_info(seal_info, algorithm);

        // 计算文档摘要
        let doc_hash = match algorithm {
            ses::SealAlgorithm::Sm2 => crypto::sm3_hash(doc_data),
            ses::SealAlgorithm::Rsa => crypto::sha256(doc_data),
        };

        // 构建 TBS_Sign DER
        let tbs_sign_der = ses::build_tbs_sign(&ses_params, &doc_hash);

        // 计算 TBS_Sign 的摘要 (返回给外部签名服务)
        let tbs_hash = match algorithm {
            ses::SealAlgorithm::Sm2 => crypto::sm3_hash(&tbs_sign_der),
            ses::SealAlgorithm::Rsa => crypto::sha256(&tbs_sign_der),
        };

        // 存储 TBS_Sign DER 和参数, 供 set_value_ex 使用
        self.settings.insert("pending_tbs_der".to_string(),
            crypto::b64_encode(&tbs_sign_der));
        self.settings.insert("pending_doc_hash".to_string(),
            crypto::b64_encode(&doc_hash));

        // 返回摘要的 hex 字符串
        Ok(hex::encode(&tbs_hash))
    }

    /// 设置扩展值（签名合成）
    /// 对应 OFD_Plugin.SetValueEx("SET_AIPSIGN_ORIDATA:certPk", lType, 0, signdata)
    ///
    /// 将外部签名服务返回的签名值与 TBS_Sign 合成为 SES_Signature,
    /// 再包装为 PKCS#7 SignedData
    pub fn set_value_ex(
        &mut self,
        key: &str,
        l_type: i32,
        _reserved: i32,
        signdata: &str,
    ) -> Result<i32, String> {
        if !key.starts_with("SET_AIPSIGN_ORIDATA:") {
            return Err(format!("未知的 SetValueEx key: {}", key));
        }

        let cert_pk = &key[21..];

        if signdata.is_empty() {
            self.last_error = -1;
            self.last_error_string = "签名数据为空".to_string();
            return Ok(0);
        }

        // 获取存储的 TBS_Sign DER 和文档摘要
        let tbs_sign_der = self.settings.get("pending_tbs_der")
            .and_then(|s| crypto::b64_decode(s).ok())
            .ok_or_else(|| "未找到待签名数据, 请先调用 get_value_ex".to_string())?;

        let doc_hash = self.settings.get("pending_doc_hash")
            .and_then(|s| crypto::b64_decode(s).ok())
            .unwrap_or_default();

        // 解析签名值 (外部签名服务返回的 hex 或 base64)
        let signature_value = if signdata.starts_with("0x") || signdata.len() % 2 == 0 {
            // hex 编码
            hex::decode(signdata.trim_start_matches("0x"))
                .unwrap_or_else(|_| crypto::b64_decode(signdata).unwrap_or_default())
        } else {
            // base64 编码
            crypto::b64_decode(signdata).unwrap_or_default()
        };

        // 获取印章信息
        let seal_info = self.pending_seal_info.as_ref()
            .ok_or_else(|| "未设置印章信息".to_string())?;

        let ses_params = ses::ses_params_from_seal_info(seal_info, self.algorithm);

        // 构建 SES_Signature
        // SES_Signature = { TBS_Sign, signatureAlgo, signature }
        let ses_sig_der = ses::build_ses_signature(
            &ses_params,
            &doc_hash,
            &signature_value,
        );

        // 构建 PKCS#7 SignedData (包含 SES_Signature)
        let pkcs7_der = pkcs7::build_mock_pkcs7(
            self.algorithm,
            &ses_sig_der,
            &doc_hash,
            ses_params.sign_time,
        );

        // 存储 PKCS#7 供后续嵌入使用
        self.pending_pkcs7 = Some(pkcs7_der.clone());

        // 存储签名结果
        self.current_cert = Some(cert_pk.to_string());
        self.settings.insert("last_sign_result".to_string(),
            crypto::b64_encode(&pkcs7_der));
        self.settings.insert("last_sign_cert".to_string(), cert_pk.to_string());
        self.settings.insert("last_sign_type".to_string(), l_type.to_string());

        // 清理临时数据
        self.settings.remove("pending_tbs_der");
        self.settings.remove("pending_doc_hash");

        Ok(1)
    }

    /// 获取待签名的 SHA 哈希数据 (RSA 算法)
    pub fn get_sign_sha_data(&mut self) -> Result<String, String> {
        let doc_data = self.pending_doc_data.as_ref()
            .ok_or_else(|| "未设置待签名文档数据".to_string())?;
        Ok(crypto::sha256_hex(doc_data))
    }

    /// 执行云签 — 发送到服务端签名
    pub async fn cloud_sign(
        &self,
        _file_id: &str,
        _cert_sn: &str,
        sign_content: &str,
        _data_type: &str,
        data_format: &str,
        _seal_id: &str,
    ) -> Result<CloudSignResponse, String> {
        let algorithm = if data_format == "p7a" { "rsa" } else { "sm2" };

        let (sign_result, sign_cert) = match algorithm {
            "rsa" => {
                let sig = crypto::rsa_sign(sign_content.as_bytes())?;
                (crypto::b64_encode(&sig), crypto::MOCK_RSA_CERT.to_string())
            }
            _ => {
                let sig = crypto::sm2_sign(sign_content.as_bytes(), &[])?;
                (crypto::b64_encode(&sig), crypto::MOCK_SM2_CERT.to_string())
            }
        };

        Ok(CloudSignResponse {
            sign_result,
            sign_cert: Some(sign_cert),
            sign_cert_sn: Some(format!("MOCK_SN_{}", _cert_sn)),
            msg: None,
            code: 200,
        })
    }

    /// UKey 硬件签名
    pub fn ukey_sign(&self, data: &str, _pin_code: &str) -> Result<String, String> {
        let sig = crypto::sm2_sign(data.as_bytes(), &[])?;
        Ok(crypto::b64_encode(&sig))
    }

    /// 获取内部错误码
    pub fn get_re_value(&self) -> i32 {
        self.last_error
    }

    /// 获取错误信息
    pub fn get_error_string(&self, _code: &str) -> String {
        if !self.last_error_string.is_empty() {
            self.last_error_string.clone()
        } else {
            "未知错误".to_string()
        }
    }

    /// 重新加载文档显示
    pub fn reload_doc_data(&self, _action: &str) -> Result<(), String> {
        Ok(())
    }

    /// 设置印章模式
    pub fn set_seal_mode(&mut self, mode: i32) {
        self.settings.insert("seal_mode".to_string(), mode.to_string());
    }

    /// 设置单文件模式
    pub fn set_single_mode(&mut self, enabled: bool) {
        self.settings.insert("single_mode".to_string(), enabled.to_string());
    }
}

/// 云签响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CloudSignResponse {
    #[serde(rename = "signResult")]
    pub sign_result: String,
    #[serde(rename = "signCert")]
    pub sign_cert: Option<String>,
    #[serde(rename = "signCertSn")]
    pub sign_cert_sn: Option<String>,
    pub msg: Option<String>,
    pub code: i32,
}

impl Default for SignEngine {
    fn default() -> Self {
        Self::new()
    }
}

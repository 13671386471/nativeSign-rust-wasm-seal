//! 数字签名模块 — 签名值生成、验证、签章合成
//!
//! ⚠️ MOCK 说明:
//! - 云签签名通过 HTTP 请求服务端完成，此处模拟服务端响应
//! - UKey 硬件签名通过 WebSocket 代理完成，此处模拟硬件交互
//! - 标记 FIXME: REPLACE_WITH_REAL_SIGN_SERVICE 的位置需替换为真实签名服务

use crate::crypto;
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
        }
    }

    /// 设置全局配置值
    /// 对应 OFD_Plugin.SetValue(key, value)
    pub fn set_value(&mut self, key: &str, value: &str) {
        match key {
            "SET_JAVA_SM2MODE" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            "SET_PDFSEAL_ALPHA" => {
                self.settings.insert(key.to_string(), value.to_string());
            }
            "ADD_FORCETYPE_VALUE8" => {
                // 设置为国办签章格式
                self.settings.insert("force_type".to_string(), "8".to_string());
                self.settings.insert(key.to_string(), value.to_string());
            }
            "DEL_FORCETYPE_VALUE8" => {
                // 取消国办签章格式
                self.settings.remove("force_type");
                self.settings.insert(key.to_string(), value.to_string());
            }
            "ADD_FORCETYPE_VALUE7" => {
                // 设置为 P7 签章格式
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
                // 设置证书公钥
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
    /// 对应 OFD_Plugin.GetValue(key)
    pub fn get_value(&self, key: &str) -> Option<String> {
        match key {
            "GET_CURRENT_CERT" => self.current_cert.clone(),
            "GET_LAST_SEAL" => None, // 由 SealEngine 管理
            _ => self.settings.get(key).cloned(),
        }
    }

    /// 获取扩展值（签名相关）
    /// 对应 OFD_Plugin.GetValueEx("GET_AIPSIGN_ORIDATA", type, "", 0, "")
    ///
    /// lType 含义:
    ///   2 = SM2 + 国办标准章
    ///   3 = SM2 + P7章
    pub fn get_value_ex(&self, key: &str, l_type: i32) -> Result<String, String> {
        if key != "GET_AIPSIGN_ORIDATA" {
            return Err(format!("未知的 GetValueEx key: {}", key));
        }

        // FIXME: 生产环境需从文档中实际提取待签名数据
        // 当前返回模拟的待签名哈希

        // 根据签章格式类型生成不同的待签名数据
        let ori_data = match l_type {
            2 => {
                // SM2 + 国办标准章: 返回 SM3 哈希
                let mock_content = b"SM2_OFFICIAL_SEAL_SIGNING_DATA";
                crypto::sm3_hash_hex(mock_content)
            }
            3 => {
                // SM2 + P7章: 返回 SM3 哈希
                let mock_content = b"SM2_P7_SEAL_SIGNING_DATA";
                crypto::sm3_hash_hex(mock_content)
            }
            _ => {
                return Err(format!("不支持的签名类型: {}", l_type));
            }
        };

        Ok(ori_data)
    }

    /// 设置扩展值（签名合成）
    /// 对应 OFD_Plugin.SetValueEx("SET_AIPSIGN_ORIDATA:certPk", lType, 0, signdata)
    ///
    /// 将签名值和证书公钥写回文档完成签章合成
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

        // 提取证书公钥
        let cert_pk = &key[21..]; // 去掉 "SET_AIPSIGN_ORIDATA:" 前缀

        // 验证签名数据
        if signdata.is_empty() {
            self.last_error = -1;
            self.last_error_string = "签名数据为空".to_string();
            return Ok(0);
        }

        // FIXME: 生产环境需将证书+签名值写入文档签章结构体
        // 当前模拟合成成功

        // lType: 0=OFD, 2=PDF
        self.current_cert = Some(cert_pk.to_string());
        self.settings.insert("last_sign_result".to_string(), signdata.to_string());
        self.settings.insert("last_sign_cert".to_string(), cert_pk.to_string());
        self.settings.insert("last_sign_type".to_string(), l_type.to_string());

        Ok(1) // 1 = 成功
    }

    /// 获取待签名的 SHA 哈希数据 (RSA 算法)
    /// 对应 OFD_Plugin.GetSignSHAData()
    pub fn get_sign_sha_data(&self) -> Result<String, String> {
        // FIXME: 生产环境需从文档中实际提取待签名 SHA 哈希
        let mock_data = b"RSA_SIGNING_CONTENT_FOR_SEAL";
        Ok(crypto::sha256_hex(mock_data))
    }

    /// 执行云签 — 发送到服务端签名
    /// 对应 POST /local/sign/sealSign
    ///
    /// 参数:
    ///   localFileCode: 文件标识
    ///   signCertSn: 证书序列号
    ///   signContent: 待签名原文
    ///   dataType: "2"
    ///   dataFormat: "p1"(SM2) 或 "p7a"(RSA)
    ///   sealId: 印章ID
    pub async fn cloud_sign(
        &self,
        _file_id: &str,
        _cert_sn: &str,
        sign_content: &str,
        _data_type: &str,
        data_format: &str,
        _seal_id: &str,
    ) -> Result<CloudSignResponse, String> {
        // FIXME: REPLACE_WITH_REAL_SIGN_SERVICE
        // 生产环境通过 HTTP POST /local/sign/sealSign 发送签名请求
        // 当前返回模拟数据

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
    /// 对应 OFD_Plugin.SignData(data, pinCode)
    pub fn ukey_sign(
        &self,
        data: &str,
        _pin_code: &str,
    ) -> Result<String, String> {
        // FIXME: REPLACE_WITH_REAL_SIGN_SERVICE
        // 生产环境通过 WebSocket 代理向 UKey 硬件发送签名指令
        // 当前模拟 SM2 签名

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
        // 通知渲染引擎重绘文档
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

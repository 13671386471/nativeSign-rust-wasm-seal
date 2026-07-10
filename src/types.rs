//! 数据类型定义 — 与现有 WASM 引擎 API 保持兼容

use serde::{Deserialize, Serialize};

/// 签署模式
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SignMode {
    #[serde(rename = "cloud")]
    Cloud,
    #[serde(rename = "ukey")]
    Ukey,
    #[serde(rename = "mobile")]
    Mobile,
}

impl Default for SignMode {
    fn default() -> Self { SignMode::Cloud }
}

/// 证书算法
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Algorithm {
    #[serde(rename = "rsa")]
    Rsa,
    #[serde(rename = "sm2")]
    Sm2,
}

impl Default for Algorithm {
    fn default() -> Self { Algorithm::Sm2 }
}

/// 文档类型
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum DocType {
    #[serde(rename = "pdf")]
    Pdf,
    #[serde(rename = "ofd")]
    Ofd,
}

/// 签章类型
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SealMode {
    #[serde(rename = "place")]
    Place,
    #[serde(rename = "keyword")]
    Keyword,
    #[serde(rename = "seam")]
    Seam,
    #[serde(rename = "draw")]
    Draw,
}

/// 印章数据对象 — 对应 Vue 中的 wish.seals[] 结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealInfo {
    pub origin: String,          // "cloud" / "ukey" / "mobile"
    #[serde(rename = "sealId")]
    pub seal_id: String,
    #[serde(rename = "sealName")]
    pub seal_name: String,
    pub width: f64,               // mm
    pub height: f64,              // mm
    pub seal_type: Option<i32>,
    #[serde(rename = "sealImage")]
    pub seal_image: String,       // base64 印章图像
    #[serde(rename = "signCertSn")]
    pub sign_cert_sn: Option<String>,
    #[serde(rename = "signData")]
    pub sign_data: Option<String>, // 印章结构体数据
    #[serde(rename = "signCert")]
    pub sign_cert: Option<String>, // 签名证书公钥 (base64)
    #[serde(rename = "sealStartTime")]
    pub seal_start_time: Option<String>,
    #[serde(rename = "sealEndTime")]
    pub seal_end_time: Option<String>,
    /// 签名者名称（如 "张三"）
    #[serde(rename = "signerName")]
    pub signer_name: Option<String>,
    /// 签名时间
    #[serde(rename = "signTime")]
    pub sign_time: Option<String>,
    /// 签名方法（如 "Adobe.PPKMS / adbe.pkcs7.sha1"）
    #[serde(rename = "signMethod")]
    pub sign_method: Option<String>,
    /// 签章证书颁发者
    #[serde(rename = "certIssuer")]
    pub cert_issuer: Option<String>,
    /// 签章证书主题 DN
    #[serde(rename = "certSubject")]
    pub cert_subject: Option<String>,
    /// 签章证书起始时间
    #[serde(rename = "certStartTime")]
    pub cert_start_time: Option<String>,
    /// 签章证书终止时间
    #[serde(rename = "certEndTime")]
    pub cert_end_time: Option<String>,
    /// 签章证书算法标识
    #[serde(rename = "certAlgorithm")]
    pub cert_algorithm: Option<String>,
    /// 签章证书数据 (base64)
    #[serde(rename = "certData")]
    pub cert_data: Option<String>,
}

/// 文档状态
#[derive(Debug, Clone)]
pub struct DocState {
    pub file_id: String,
    pub file_name: String,
    pub file_size_kb: u64,
    pub page_count: u32,
    pub current_page: u32,
    pub doc_type: DocType,
    pub is_opened: bool,
    pub seal_count: u32,
    pub signed_count: u32,
    /// RAW 文档字节数据
    pub raw_data: Vec<u8>,
    /// 印章列表 (已落章的)
    pub seals: Vec<PlacedSeal>,
    /// 文档属性 key-value
    pub properties: std::collections::HashMap<String, String>,
}

impl Default for DocState {
    fn default() -> Self {
        Self {
            file_id: String::new(),
            file_name: String::new(),
            file_size_kb: 0,
            page_count: 0,
            current_page: 0,
            doc_type: DocType::Pdf,
            is_opened: false,
            seal_count: 0,
            signed_count: 0,
            raw_data: Vec::new(),
            seals: Vec::new(),
            properties: std::collections::HashMap::new(),
        }
    }
}

/// 已落章的印章记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacedSeal {
    pub id: usize,
    pub page_index: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub seal_info: SealInfo,
    pub signature: Option<Vec<u8>>,   // 签名值
    pub signed: bool,
}

/// UKey 设备信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UkeyInfo {
    pub status: i32,
    pub errmsg: Option<String>,
    pub retstr: Option<Vec<String>>,
}

/// UKey 印章列表
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UkeySealList {
    #[serde(rename = "DevList")]
    pub dev_list: Vec<UkeyDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UkeyDevice {
    #[serde(rename = "DevID")]
    pub dev_id: String,
    #[serde(rename = "SealList")]
    pub seal_list: Vec<UkeySeal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UkeySeal {
    #[serde(rename = "SealID")]
    pub seal_id: String,
    #[serde(rename = "SealName")]
    pub seal_name: String,
}

/// 签名配置
#[derive(Debug, Clone)]
pub struct SignConfig {
    pub algorithm: Algorithm,
    pub is_sm2_seal: bool,
    pub sign_mode: SignMode,
    pub file_format: DocType,
    pub sm2_mode: bool,
}

impl Default for SignConfig {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::Sm2,
            is_sm2_seal: false,
            sign_mode: SignMode::Cloud,
            file_format: DocType::Pdf,
            sm2_mode: false,
        }
    }
}

/// 签章结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignResult {
    pub success: bool,
    pub seal_index: usize,
    pub signature: Option<String>,  // base64
    pub cert: Option<String>,        // base64
    pub cert_sn: Option<String>,
    pub error: Option<String>,
}

/// 引擎全局配置
#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    pub sign_config: SignConfig,
    pub seal_mode: i32,          // 1 = 外部签名模式
    pub force_type: i32,         // 签章格式类型: 7=P7, 8=国办
    pub single_mode: bool,
}

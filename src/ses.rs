//! SES 电子签章数据结构 (GM/T 0031-2014 安全电子签章技术规范)
//!
//! 实现以下 ASN.1 结构的 DER 编码:
//!
//! ```text
//! SES_Signature ::= SEQUENCE {
//!     toSign            TBS_Sign,            -- 待签名数据
//!     signatureAlgo     OBJECT IDENTIFIER,    -- 签名算法 OID
//!     signature         BIT STRING            -- 签名值
//! }
//!
//! TBS_Sign ::= SEQUENCE {
//!     version           INTEGER,              -- 版本号 (1)
//!     esn               OCTET STRING,         -- 电子印章编号
//!     signatureAlgo     OBJECT IDENTIFIER,    -- 签名算法
//!     signatureTime     UTCTime,              -- 签名时间
//!     signatureValue    BIT STRING,           -- 原文摘要值
//!     cert              Certificate           -- 签名者证书
//! }
//!
//! SES_Seal ::= SEQUENCE {
//!     eSealInfo         SES_SealInfo,         -- 印章信息
//!     signatureAlgo     OBJECT IDENTIFIER,    -- 签名算法
//!     signature         BIT STRING            -- 对印章信息的签名值
//! }
//!
//! SES_SealInfo ::= SEQUENCE {
//!     header            SES_Header,           -- 印章头
//!     esn               OCTET STRING,         -- 印章编号
//!     property          SES_SealProperty,     -- 印章属性
//!     picture           SES_SealPicture,      -- 印章图片
//!     cert              Certificate           -- 印章证书
//! }
//!
//! SES_Header ::= SEQUENCE {
//!     version           INTEGER,              -- 印章版本
//!     esID              PrintableString,      -- 制作单位标识
//!     type              PrintableString       -- 印章类型
//! }
//!
//! SES_SealProperty ::= SEQUENCE {
//!     type              PrintableString,      -- 印章类型
//!     name              UTF8String,           -- 印章名称
//!     validStart        UTCTime,              -- 有效起始时间
//!     validEnd          UTCTime               -- 有效结束时间
//! }
//!
//! SES_SealPicture ::= SEQUENCE {
//!     type              PrintableString,      -- 图像格式 ("PNG"/"OFD"/"JPEG")
//!     data              BIT STRING            -- 图像数据
//! }
//! ```
//!
//! ⚠️ MOCK 说明: 证书、密钥均为假数据, 生产环境需替换为真实 CA 签发的证书

use crate::der;
use crate::crypto;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

// ============================================================
// MOCK 数据 — 证书和印章图片
// FIXME: REPLACE_WITH_REAL_CERT — 生产环境替换为真实 CA 证书
// ============================================================

/// MOCK RSA X.509 证书 (DER)
/// CN=test-signer@dianju.com, O=DianJu Test CA, C=CN
/// 有效期: 2025-01-01 ~ 2030-12-31
pub const MOCK_RSA_CERT_DER: &[u8] = include_bytes!("../mock_data/mock_rsa_cert.der");

/// MOCK RSA 私钥 (DER, PKCS#8) — 仅用于 MOCK 签名
pub const MOCK_RSA_PRIVKEY_DER: &[u8] = include_bytes!("../mock_data/mock_rsa_key.der");

/// MOCK EC (P-256, SM2 替代) X.509 证书 (DER)
/// CN=sm2-signer@dianju.com, O=DianJu Test SM2 CA, C=CN
pub const MOCK_EC_CERT_DER: &[u8] = include_bytes!("../mock_data/mock_ec_cert.der");

/// MOCK EC 私钥 (DER, PKCS#8) — 仅用于 MOCK 签名
pub const MOCK_EC_PRIVKEY_DER: &[u8] = include_bytes!("../mock_data/mock_ec_key.der");

/// MOCK 印章图片 (120x120 PNG, 红色圆环)
include!("mock_seal_png.rs");

/// MOCK 印章编号
pub const MOCK_SEAL_ESN: &[u8] = b"MOCK-ESN-2026-0001";

/// MOCK 制作单位
pub const MOCK_ES_ID: &str = "DianJu-Mock-CA";

// ============================================================
// SES 结构体定义
// ============================================================

/// 签章算法类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SealAlgorithm {
    /// SM2 + SM3 (国密)
    Sm2,
    /// RSA + SHA256
    Rsa,
}

impl SealAlgorithm {
    /// 获取签名算法 OID
    pub fn signature_oid(&self) -> &'static str {
        match self {
            SealAlgorithm::Sm2 => der::oids::SM2_WITH_SM3,
            SealAlgorithm::Rsa => der::oids::RSA_WITH_SHA256,
        }
    }

    /// 获取摘要算法 OID
    pub fn digest_oid(&self) -> &'static str {
        match self {
            SealAlgorithm::Sm2 => der::oids::SM3,
            SealAlgorithm::Rsa => der::oids::SHA256,
        }
    }

    /// 获取证书 DER
    pub fn cert_der(&self) -> &'static [u8] {
        match self {
            SealAlgorithm::Sm2 => MOCK_EC_CERT_DER,
            SealAlgorithm::Rsa => MOCK_RSA_CERT_DER,
        }
    }

    /// 获取私钥 DER (仅 MOCK)
    pub fn privkey_der(&self) -> &'static [u8] {
        match self {
            SealAlgorithm::Sm2 => MOCK_EC_PRIVKEY_DER,
            SealAlgorithm::Rsa => MOCK_RSA_PRIVKEY_DER,
        }
    }
}

/// SES 签章参数 — 构造 SES 结构所需的所有信息
#[derive(Debug, Clone)]
pub struct SesParams {
    /// 签章算法
    pub algorithm: SealAlgorithm,
    /// 印章编号 (ESN)
    pub seal_esn: Vec<u8>,
    /// 印章名称
    pub seal_name: String,
    /// 印章图片数据 (PNG/JPEG)
    pub seal_image: Vec<u8>,
    /// 图片格式 ("PNG" / "JPEG" / "OFD")
    pub image_format: String,
    /// 签名者证书 DER
    pub cert_der: Vec<u8>,
    /// 签名时间 (year, month, day, hour, min, sec)
    pub sign_time: (u32, u32, u32, u32, u32, u32),
    /// 证书有效期起始
    pub cert_valid_start: (u32, u32, u32, u32, u32, u32),
    /// 证书有效期截止
    pub cert_valid_end: (u32, u32, u32, u32, u32, u32),
    /// 印章有效期起始
    pub seal_valid_start: (u32, u32, u32, u32, u32, u32),
    /// 印章有效期截止
    pub seal_valid_end: (u32, u32, u32, u32, u32, u32),
}

impl Default for SesParams {
    fn default() -> Self {
        Self {
            algorithm: SealAlgorithm::Sm2,
            seal_esn: MOCK_SEAL_ESN.to_vec(),
            seal_name: "测试电子印章".to_string(),
            seal_image: MOCK_SEAL_PNG.to_vec(),
            image_format: "PNG".to_string(),
            cert_der: MOCK_EC_CERT_DER.to_vec(),
            sign_time: (2026, 7, 10, 16, 30, 0),
            cert_valid_start: (2025, 1, 1, 0, 0, 0),
            cert_valid_end: (2030, 12, 31, 23, 59, 59),
            seal_valid_start: (2025, 1, 1, 0, 0, 0),
            seal_valid_end: (2030, 12, 31, 23, 59, 59),
        }
    }
}

// ============================================================
// DER 编码: SES_Header
// ============================================================

fn encode_ses_header(version: i64, es_id: &str, seal_type: &str) -> Vec<u8> {
    der::sequence(&[
        der::integer(version),
        der::printable_string(es_id),
        der::printable_string(seal_type),
    ])
}

// ============================================================
// DER 编码: SES_SealProperty
// ============================================================

fn encode_ses_seal_property(
    seal_type: &str,
    name: &str,
    valid_start: (u32, u32, u32, u32, u32, u32),
    valid_end: (u32, u32, u32, u32, u32, u32),
) -> Vec<u8> {
    der::sequence(&[
        der::printable_string(seal_type),
        der::utf8_string(name),
        der::utc_time(valid_start.0, valid_start.1, valid_start.2,
                       valid_start.3, valid_start.4, valid_start.5),
        der::utc_time(valid_end.0, valid_end.1, valid_end.2,
                       valid_end.3, valid_end.4, valid_end.5),
    ])
}

// ============================================================
// DER 编码: SES_SealPicture
// ============================================================

fn encode_ses_seal_picture(format: &str, data: &[u8]) -> Vec<u8> {
    der::sequence(&[
        der::printable_string(format),
        der::bit_string(data, 0),
    ])
}

// ============================================================
// DER 编码: SES_SealInfo
// ============================================================

/// 编码 SES_SealInfo
///
/// SES_SealInfo ::= SEQUENCE {
///     header      SES_Header,
///     esn         OCTET STRING,
///     property    SES_SealProperty,
///     picture     SES_SealPicture,
///     cert        Certificate
/// }
fn encode_ses_seal_info(params: &SesParams) -> Vec<u8> {
    der::sequence(&[
        // header: SES_Header
        encode_ses_header(1, MOCK_ES_ID, "ES"),
        // esn: 印章编号
        der::octet_string(&params.seal_esn),
        // property: SES_SealProperty
        encode_ses_seal_property(
            "official",
            &params.seal_name,
            params.seal_valid_start,
            params.seal_valid_end,
        ),
        // picture: SES_SealPicture
        encode_ses_seal_picture(&params.image_format, &params.seal_image),
        // cert: Certificate (直接嵌入 DER)
        params.cert_der.to_vec(),
    ])
}

// ============================================================
// DER 编码: SES_Seal (电子印章数据)
// ============================================================

/// 构建 SES_Seal (电子印章数据结构)
///
/// SES_Seal ::= SEQUENCE {
///     eSealInfo       SES_SealInfo,
///     signatureAlgo   OBJECT IDENTIFIER,
///     signature       BIT STRING   -- 对 eSealInfo 的签名
/// }
///
/// # 参数
/// - `params`: SES 参数
/// - `seal_signature`: 对 eSealInfo DER 编码的签名值
///
/// # 返回
/// SES_Seal 的 DER 编码
pub fn build_ses_seal(params: &SesParams, seal_signature: &[u8]) -> Vec<u8> {
    let e_seal_info = encode_ses_seal_info(params);

    der::sequence(&[
        // eSealInfo
        e_seal_info,
        // signatureAlgo
        der::oid(params.algorithm.signature_oid()),
        // signature (BIT STRING)
        der::bit_string(seal_signature, 0),
    ])
}

/// 构建 MOCK SES_Seal (使用 MOCK 签名)
///
/// 自动生成印章签名值 (MOCK), 然后构建完整的 SES_Seal
pub fn build_mock_ses_seal(params: &SesParams) -> Vec<u8> {
    let e_seal_info = encode_ses_seal_info(params);

    // 对 eSealInfo 进行签名 (MOCK)
    let seal_sig = match params.algorithm {
        SealAlgorithm::Sm2 => crypto::sm2_sign(&e_seal_info, params.algorithm.privkey_der())
            .unwrap_or_default(),
        SealAlgorithm::Rsa => crypto::rsa_sign(&e_seal_info)
            .unwrap_or_default(),
    };

    der::sequence(&[
        e_seal_info,
        der::oid(params.algorithm.signature_oid()),
        der::bit_string(&seal_sig, 0),
    ])
}

// ============================================================
// DER 编码: TBS_Sign (待签名数据)
// ============================================================

/// 构建 TBS_Sign (待签名数据)
///
/// TBS_Sign ::= SEQUENCE {
///     version         INTEGER,              -- 版本号 (1)
///     esn             OCTET STRING,         -- 电子印章编号
///     signatureAlgo   OBJECT IDENTIFIER,    -- 签名算法
///     signatureTime   UTCTime,              -- 签名时间
///     signatureValue  BIT STRING,           -- 原文摘要值
///     cert            Certificate           -- 签名者证书
/// }
///
/// # 参数
/// - `params`: SES 参数
/// - `doc_hash`: 文档摘要值 (SM3 或 SHA256)
///
/// # 返回
/// TBS_Sign 的 DER 编码
pub fn build_tbs_sign(params: &SesParams, doc_hash: &[u8]) -> Vec<u8> {
    der::sequence(&[
        // version
        der::integer(1),
        // esn: 电子印章编号
        der::octet_string(&params.seal_esn),
        // signatureAlgo
        der::oid(params.algorithm.signature_oid()),
        // signatureTime
        der::utc_time(
            params.sign_time.0, params.sign_time.1, params.sign_time.2,
            params.sign_time.3, params.sign_time.4, params.sign_time.5,
        ),
        // signatureValue: 原文摘要值
        der::bit_string(doc_hash, 0),
        // cert: 签名者证书
        params.cert_der.to_vec(),
    ])
}

// ============================================================
// DER 编码: SES_Signature (电子签名数据)
// ============================================================

/// 构建 SES_Signature (完整的电子签名数据)
///
/// SES_Signature ::= SEQUENCE {
///     toSign          TBS_Sign,
///     signatureAlgo   OBJECT IDENTIFIER,
///     signature       BIT STRING
/// }
///
/// # 参数
/// - `params`: SES 参数
/// - `doc_hash`: 文档摘要值
/// - `signature`: 对 TBS_Sign DER 编码的签名值
///
/// # 返回
/// SES_Signature 的 DER 编码
pub fn build_ses_signature(params: &SesParams, doc_hash: &[u8], signature: &[u8]) -> Vec<u8> {
    let tbs_sign = build_tbs_sign(params, doc_hash);

    der::sequence(&[
        // toSign: TBS_Sign
        tbs_sign,
        // signatureAlgo
        der::oid(params.algorithm.signature_oid()),
        // signature
        der::bit_string(signature, 0),
    ])
}

/// 构建 MOCK SES_Signature (使用 MOCK 签名)
///
/// 自动计算文档摘要, 生成 MOCK 签名值, 构建完整的 SES_Signature
///
/// # 参数
/// - `params`: SES 参数
/// - `doc_data`: 原始文档数据 (用于计算摘要)
///
/// # 返回
/// SES_Signature 的 DER 编码
pub fn build_mock_ses_signature(params: &SesParams, doc_data: &[u8]) -> Vec<u8> {
    // 1. 计算文档摘要
    let doc_hash = match params.algorithm {
        SealAlgorithm::Sm2 => crypto::sm3_hash(doc_data),
        SealAlgorithm::Rsa => crypto::sha256(doc_data),
    };

    // 2. 构建 TBS_Sign
    let tbs_sign = build_tbs_sign(params, &doc_hash);

    // 3. 对 TBS_Sign 进行签名 (MOCK)
    let signature = match params.algorithm {
        SealAlgorithm::Sm2 => crypto::sm2_sign(&tbs_sign, params.algorithm.privkey_der())
            .unwrap_or_default(),
        SealAlgorithm::Rsa => crypto::rsa_sign(&tbs_sign)
            .unwrap_or_default(),
    };

    // 4. 构建 SES_Signature
    der::sequence(&[
        tbs_sign,
        der::oid(params.algorithm.signature_oid()),
        der::bit_string(&signature, 0),
    ])
}

// ============================================================
// 辅助函数
// ============================================================

/// 获取 MOCK 证书的 base64 编码
pub fn get_mock_cert_base64(algorithm: SealAlgorithm) -> String {
    BASE64.encode(algorithm.cert_der())
}

/// 获取 MOCK 证书的序列号 (从 DER 中提取, 简化版)
pub fn get_mock_cert_sn(algorithm: SealAlgorithm) -> String {
    // 简化: 直接返回固定序列号
    match algorithm {
        SealAlgorithm::Sm2 => "MOCK-SM2-SN-2026".to_string(),
        SealAlgorithm::Rsa => "MOCK-RSA-SN-2026".to_string(),
    }
}

/// 获取 MOCK 证书的主题信息
pub fn get_mock_cert_subject(algorithm: SealAlgorithm) -> String {
    match algorithm {
        SealAlgorithm::Sm2 => "CN=sm2-signer@dianju.com,OU=Mock SM2 Certificate,O=DianJu Test SM2 CA,C=CN".to_string(),
        SealAlgorithm::Rsa => "CN=test-signer@dianju.com,OU=Mock Certificate,O=DianJu Test CA,C=CN".to_string(),
    }
}

/// 获取 MOCK 证书的颁发者信息
pub fn get_mock_cert_issuer(algorithm: SealAlgorithm) -> String {
    // 自签名证书, 颁发者 = 主题
    get_mock_cert_subject(algorithm)
}

/// 从 SealInfo 构建 SesParams
pub fn ses_params_from_seal_info(
    seal_info: &crate::types::SealInfo,
    algorithm: SealAlgorithm,
) -> SesParams {
    let seal_image = if !seal_info.seal_image.is_empty() {
        BASE64.decode(&seal_info.seal_image).unwrap_or_else(|_| MOCK_SEAL_PNG.to_vec())
    } else {
        MOCK_SEAL_PNG.to_vec()
    };

    let seal_esn = if let Some(ref sn) = seal_info.sign_cert_sn {
        if !sn.is_empty() {
            sn.as_bytes().to_vec()
        } else {
            MOCK_SEAL_ESN.to_vec()
        }
    } else {
        MOCK_SEAL_ESN.to_vec()
    };

    let seal_name = seal_info.seal_name.clone();

    // 使用当前时间作为签名时间
    let now = current_time_tuple();

    SesParams {
        algorithm,
        seal_esn,
        seal_name,
        seal_image,
        image_format: "PNG".to_string(),
        cert_der: algorithm.cert_der().to_vec(),
        sign_time: now,
        cert_valid_start: (2025, 1, 1, 0, 0, 0),
        cert_valid_end: (2030, 12, 31, 23, 59, 59),
        seal_valid_start: (2025, 1, 1, 0, 0, 0),
        seal_valid_end: (2030, 12, 31, 23, 59, 59),
    }
}

/// 获取当前时间元组 (年, 月, 日, 时, 分, 秒) — 简化版
/// 注意: WASM 环境中无法直接获取系统时间, 使用编译时时间或固定时间
pub fn current_time_tuple() -> (u32, u32, u32, u32, u32, u32) {
    // WASM 中无法获取真实系统时间, 使用固定时间
    // 生产环境应通过 JS 注入时间
    (2026, 7, 10, 16, 30, 0)
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_mock_ses_seal() {
        let params = SesParams::default();
        let seal_der = build_mock_ses_seal(&params);
        assert!(!seal_der.is_empty());
        assert_eq!(seal_der[0], 0x30); // SEQUENCE tag
        println!("SES_Seal DER size: {} bytes", seal_der.len());
    }

    #[test]
    fn test_build_mock_ses_signature() {
        let params = SesParams::default();
        let doc_data = b"test document data for signing";
        let sig_der = build_mock_ses_signature(&params, doc_data);
        assert!(!sig_der.is_empty());
        assert_eq!(sig_der[0], 0x30); // SEQUENCE tag
        println!("SES_Signature DER size: {} bytes", sig_der.len());
    }

    #[test]
    fn test_build_tbs_sign() {
        let params = SesParams::default();
        let doc_hash = crypto::sm3_hash(b"test");
        let tbs = build_tbs_sign(&params, &doc_hash);
        assert!(!tbs.is_empty());
        assert_eq!(tbs[0], 0x30); // SEQUENCE tag
        println!("TBS_Sign DER size: {} bytes", tbs.len());
    }

    #[test]
    fn test_mock_certs_loaded() {
        assert!(!MOCK_RSA_CERT_DER.is_empty());
        assert!(!MOCK_EC_CERT_DER.is_empty());
        assert!(MOCK_RSA_CERT_DER.len() > 100);
        assert!(MOCK_EC_CERT_DER.len() > 100);
        assert_eq!(MOCK_RSA_CERT_DER[0], 0x30); // SEQUENCE
        assert_eq!(MOCK_EC_CERT_DER[0], 0x30); // SEQUENCE
    }
}

//! PKCS#7/CMS SignedData 结构 (RFC 2315 / RFC 5652)
//!
//! 用于将 SES_Signature 包装为 PDF 签名所需的 PKCS#7 格式
//!
//! ```text
//! ContentInfo ::= SEQUENCE {
//!     contentType  OBJECT IDENTIFIER,   -- 1.2.840.113549.1.7.2 (signedData)
//!     content      [0] EXPLICIT ANY     -- SignedData
//! }
//!
//! SignedData ::= SEQUENCE {
//!     version            INTEGER,                   -- 1
//!     digestAlgorithms   SET OF AlgorithmIdentifier,
//!     encapContentInfo   EncapsulatedContentInfo,
//!     certificates       [0] IMPLICIT SET OF Certificate OPTIONAL,
//!     signerInfos        SET OF SignerInfo
//! }
//!
//! EncapsulatedContentInfo ::= SEQUENCE {
//!     eContentType  OBJECT IDENTIFIER,
//!     eContent      [0] EXPLICIT OCTET STRING OPTIONAL   -- SES_Signature DER
//! }
//!
//! SignerInfo ::= SEQUENCE {
//!     version                  INTEGER,                  -- 1
//!     sid                      SignerIdentifier,         -- IssuerAndSerialNumber
//!     digestAlgorithm          AlgorithmIdentifier,
//!     authenticatedAttributes  [0] IMPLICIT SET OF Attribute OPTIONAL,
//!     digestEncryptionAlgorithm AlgorithmIdentifier,
//!     encryptedDigest          OCTET STRING,             -- 签名值
//!     unauthenticatedAttributes [1] IMPLICIT SET OF Attribute OPTIONAL
//! }
//!
//! IssuerAndSerialNumber ::= SEQUENCE {
//!     issuer       Name,
//!     serialNumber CertificateSerialNumber
//! }
//!
//! Attribute ::= SEQUENCE {
//!     attrType  OBJECT IDENTIFIER,
//!     attrValues SET OF ANY
//! }
//! ```

use crate::der;
use crate::ses::SealAlgorithm;

// ============================================================
// AlgorithmIdentifier 编码
// ============================================================

/// 编码 AlgorithmIdentifier (OID + NULL 参数)
fn algo_id(oid_str: &str) -> Vec<u8> {
    der::algorithm_identifier(oid_str)
}

// ============================================================
// IssuerAndSerialNumber 编码 (简化版)
// ============================================================

/// 从证书 DER 中提取 IssuerAndSerialNumber
///
/// 简化实现: 扫描 DER 找到 Issuer 和 SerialNumber
/// 生产环境应使用完整的 X.509 解析器
fn extract_issuer_and_serial(cert_der: &[u8]) -> (Vec<u8>, Vec<u8>) {
    // X.509 Certificate ::= SEQUENCE { tbsCertificate, signatureAlgorithm, signatureValue }
    // TBSCertificate ::= SEQUENCE { version [0], serialNumber INTEGER, signature, issuer Name, ... }
    //
    // 我们需要:
    // 1. serialNumber: 在 TBSCertificate 中的第二个字段 (跳过 version [0])
    // 2. issuer: 在 TBSCertificate 中的第四个字段

    // 简化: 直接从 DER 中定位 serialNumber 和 issuer
    // 跳过外层 SEQUENCE
    let (_, inner) = match der_decode_tlv(cert_der) {
        Some(v) => v,
        None => return (vec![], vec![]),
    };

    // 跳过 TBSCertificate SEQUENCE
    let (_, tbs) = match der_decode_tlv(&inner) {
        Some(v) => v,
        None => return (vec![], vec![]),
    };

    let mut pos = 0;

    // 跳过 version [0] EXPLICIT (如果存在)
    if pos < tbs.len() && tbs[pos] == 0xA0 {
        let (_, version_val) = der_decode_tlv(&tbs[pos..]).unwrap();
        pos += tlv_len(0xA0, &version_val);
    }

    // 读取 serialNumber (INTEGER)
    if pos < tbs.len() && tbs[pos] == 0x02 {
        let (sn_len, sn_val) = der_decode_tlv(&tbs[pos..]).unwrap();
        let serial_der = tbs[pos..pos + sn_len].to_vec();
        pos += sn_len;

        // 跳过 signature (AlgorithmIdentifier SEQUENCE)
        if pos < tbs.len() && tbs[pos] == 0x30 {
            let (sig_len, _) = der_decode_tlv(&tbs[pos..]).unwrap();
            pos += sig_len;
        }

        // 读取 issuer (Name = SEQUENCE)
        if pos < tbs.len() && tbs[pos] == 0x30 {
            let issuer_len = der_decode_tlv(&tbs[pos..]).unwrap().0;
            let issuer_der = tbs[pos..pos + issuer_len].to_vec();
            return (issuer_der, serial_der);
        }
    }

    (vec![], vec![])
}

/// 简单 DER TLV 解码: 返回 (总长度, 值)
fn der_decode_tlv(data: &[u8]) -> Option<(usize, Vec<u8>)> {
    if data.len() < 2 {
        return None;
    }
    let _tag = data[0];
    let len_byte = data[1];
    let (header_len, content_len) = if len_byte < 0x80 {
        (2, len_byte as usize)
    } else {
        let num_bytes = (len_byte & 0x7F) as usize;
        if data.len() < 2 + num_bytes {
            return None;
        }
        let mut len = 0usize;
        for i in 0..num_bytes {
            len = (len << 8) | data[2 + i] as usize;
        }
        (2 + num_bytes, len)
    };
    let total = header_len + content_len;
    if data.len() < total {
        return None;
    }
    Some((total, data[header_len..total].to_vec()))
}

/// 计算 TLV 长度 (给定 tag 和 value)
fn tlv_len(_tag: u8, value: &[u8]) -> usize {
    let header = if value.len() < 0x80 {
        2
    } else {
        let mut n = value.len();
        let mut count = 0;
        while n > 0 {
            count += 1;
            n >>= 8;
        }
        2 + count
    };
    header + value.len()
}

// ============================================================
// SignerInfo 编码
// ============================================================

/// 构建 SignerInfo
///
/// # 参数
/// - `algorithm`: 签章算法
/// - `cert_der`: 证书 DER (用于提取 IssuerAndSerialNumber)
/// - `doc_hash`: 文档摘要值
/// - `ses_signature_der`: SES_Signature 的 DER (作为 encapsulated content)
/// - `encrypted_digest`: 签名值 (对 authenticatedAttributes 的签名)
fn build_signer_info(
    algorithm: SealAlgorithm,
    cert_der: &[u8],
    doc_hash: &[u8],
    sign_time: (u32, u32, u32, u32, u32, u32),
    encrypted_digest: &[u8],
) -> Vec<u8> {
    let (issuer_der, serial_der) = extract_issuer_and_serial(cert_der);

    // IssuerAndSerialNumber
    let sid = der::sequence(&[issuer_der, serial_der]);

    // authenticatedAttributes [0] IMPLICIT SET
    // 包含: contentType, messageDigest, signingTime
    let content_type_attr = der::sequence(&[
        der::oid(der::oids::CONTENT_TYPE),
        der::set(&[der::oid(der::oids::SES_DATA_TYPE)]),
    ]);

    let message_digest_attr = der::sequence(&[
        der::oid(der::oids::MESSAGE_DIGEST),
        der::set(&[der::octet_string(doc_hash)]),
    ]);

    let signing_time_attr = der::sequence(&[
        der::oid(der::oids::SIGNING_TIME),
        der::set(&[der::utc_time(
            sign_time.0, sign_time.1, sign_time.2,
            sign_time.3, sign_time.4, sign_time.5,
        )]),
    ]);

    let auth_attrs = der::context_implicit_constructed(0, &[
        content_type_attr,
        message_digest_attr,
        signing_time_attr,
    ]);

    der::sequence(&[
        // version
        der::integer(1),
        // sid: IssuerAndSerialNumber
        sid,
        // digestAlgorithm
        algo_id(algorithm.digest_oid()),
        // authenticatedAttributes [0] IMPLICIT
        auth_attrs,
        // digestEncryptionAlgorithm (签名算法)
        algo_id(algorithm.signature_oid()),
        // encryptedDigest
        der::octet_string(encrypted_digest),
    ])
}

// ============================================================
// SignedData 编码
// ============================================================

/// 构建 PKCS#7 SignedData 并包装在 ContentInfo 中
///
/// 这是写入 PDF /Contents 的完整签名值
///
/// # 参数
/// - `algorithm`: 签章算法
/// - `cert_der`: 签名证书 DER
/// - `ses_signature_der`: SES_Signature 的 DER 编码 (作为 encapsulated content)
/// - `doc_hash`: 文档摘要值
/// - `sign_time`: 签名时间
/// - `encrypted_digest`: 对 authenticatedAttributes 的签名值
///
/// # 返回
/// ContentInfo(包含 SignedData) 的 DER 编码
pub fn build_pkcs7_signed_data(
    algorithm: SealAlgorithm,
    cert_der: &[u8],
    ses_signature_der: &[u8],
    doc_hash: &[u8],
    sign_time: (u32, u32, u32, u32, u32, u32),
    encrypted_digest: &[u8],
) -> Vec<u8> {
    // digestAlgorithms SET
    let digest_algos = der::set(&[algo_id(algorithm.digest_oid())]);

    // EncapsulatedContentInfo
    let encap_content = der::sequence(&[
        // eContentType: SES 数据类型
        der::oid(der::oids::SES_DATA_TYPE),
        // eContent [0] EXPLICIT OCTET STRING (包含 SES_Signature DER)
        der::context_explicit(0, &der::octet_string(ses_signature_der)),
    ]);

    // certificates [0] IMPLICIT SET OF Certificate
    let certs = der::context_implicit_constructed(0, &[cert_der.to_vec()]);

    // SignerInfo
    let signer_info = build_signer_info(
        algorithm,
        cert_der,
        doc_hash,
        sign_time,
        encrypted_digest,
    );

    let signer_infos = der::set(&[signer_info]);

    // SignedData SEQUENCE
    let signed_data = der::sequence(&[
        // version
        der::integer(1),
        // digestAlgorithms
        digest_algos,
        // encapContentInfo
        encap_content,
        // certificates [0]
        certs,
        // signerInfos
        signer_infos,
    ]);

    // ContentInfo
    der::sequence(&[
        // contentType: signedData
        der::oid(der::oids::PKCS7_SIGNED_DATA),
        // content [0] EXPLICIT
        der::context_explicit(0, &signed_data),
    ])
}

/// 构建 MOCK PKCS#7 SignedData
///
/// 使用 MOCK 签名值, 自动构建完整的 PKCS#7 结构
///
/// # 参数
/// - `algorithm`: 签章算法
/// - `ses_signature_der`: SES_Signature 的 DER 编码
/// - `doc_hash`: 文档摘要值
/// - `sign_time`: 签名时间
///
/// # 返回
/// ContentInfo(包含 SignedData) 的 DER 编码
pub fn build_mock_pkcs7(
    algorithm: SealAlgorithm,
    ses_signature_der: &[u8],
    doc_hash: &[u8],
    sign_time: (u32, u32, u32, u32, u32, u32),
) -> Vec<u8> {
    let cert_der = algorithm.cert_der();

    // 构建 authenticatedAttributes 并计算其摘要
    let (issuer_der, serial_der) = extract_issuer_and_serial(cert_der);
    let sid = der::sequence(&[issuer_der, serial_der]);

    let content_type_attr = der::sequence(&[
        der::oid(der::oids::CONTENT_TYPE),
        der::set(&[der::oid(der::oids::SES_DATA_TYPE)]),
    ]);
    let message_digest_attr = der::sequence(&[
        der::oid(der::oids::MESSAGE_DIGEST),
        der::set(&[der::octet_string(doc_hash)]),
    ]);
    let signing_time_attr = der::sequence(&[
        der::oid(der::oids::SIGNING_TIME),
        der::set(&[der::utc_time(
            sign_time.0, sign_time.1, sign_time.2,
            sign_time.3, sign_time.4, sign_time.5,
        )]),
    ]);

    // authenticatedAttributes 需要作为 SET 编码后再哈希
    // 注意: 在 IMPLICIT [0] 标签下, SET 的 tag 被替换为 0xA0
    // 但计算摘要时, 需要使用原始 SET tag (0x31)
    let auth_attrs_set = der::set(&[
        content_type_attr.clone(),
        message_digest_attr.clone(),
        signing_time_attr.clone(),
    ]);

    // 计算 authenticatedAttributes 的摘要
    let auth_attrs_hash = match algorithm {
        SealAlgorithm::Sm2 => crate::crypto::sm3_hash(&auth_attrs_set),
        SealAlgorithm::Rsa => crate::crypto::sha256(&auth_attrs_set),
    };

    // 对 authenticatedAttributes 摘要进行签名 (MOCK)
    let encrypted_digest = match algorithm {
        SealAlgorithm::Sm2 => crate::crypto::sm2_sign(&auth_attrs_hash, algorithm.privkey_der())
            .unwrap_or_default(),
        SealAlgorithm::Rsa => crate::crypto::rsa_sign(&auth_attrs_hash)
            .unwrap_or_default(),
    };

    build_pkcs7_signed_data(
        algorithm,
        cert_der,
        ses_signature_der,
        doc_hash,
        sign_time,
        &encrypted_digest,
    )
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ses::{SesParams, build_mock_ses_signature};
    use crate::crypto;

    #[test]
    fn test_build_pkcs7() {
        let params = SesParams::default();
        let doc_data = b"test document data";
        let doc_hash = crypto::sm3_hash(doc_data);

        let ses_sig = build_mock_ses_signature(&params, doc_data);
        let pkcs7 = build_mock_pkcs7(
            params.algorithm,
            &ses_sig,
            &doc_hash,
            params.sign_time,
        );

        assert!(!pkcs7.is_empty());
        assert_eq!(pkcs7[0], 0x30); // SEQUENCE
        println!("PKCS#7 DER size: {} bytes", pkcs7.len());
    }

    #[test]
    fn test_extract_issuer_serial() {
        let cert = SealAlgorithm::Sm2.cert_der();
        let (issuer, serial) = extract_issuer_and_serial(cert);
        assert!(!issuer.is_empty(), "issuer should not be empty");
        assert!(!serial.is_empty(), "serial should not be empty");
        assert_eq!(issuer[0], 0x30); // SEQUENCE (Name)
        assert_eq!(serial[0], 0x02); // INTEGER
    }
}

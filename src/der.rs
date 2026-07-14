//! ASN.1 DER 编码器 — 最小化实现, 仅支持 SES 签章结构所需的类型
//!
//! 参照 ITU-T X.690, 实现以下 DER 编码:
//! - SEQUENCE (0x30), SET (0x31)
//! - INTEGER (0x02), OBJECT IDENTIFIER (0x06)
//! - BIT STRING (0x03), OCTET STRING (0x04)
//! - NULL (0x05), BOOLEAN (0x01)
//! - UTF8String (0x0C), PrintableString (0x13)
//! - UTCTime (0x17), GeneralizedTime (0x18)
//! - CONTEXT 标签 [0]~[3] (explicit: 0xA0~0xA3, implicit: 0x80~0x83)

// ============================================================
// DER 标签常量
// ============================================================

pub const TAG_BOOLEAN: u8 = 0x01;
pub const TAG_INTEGER: u8 = 0x02;
pub const TAG_BIT_STRING: u8 = 0x03;
pub const TAG_OCTET_STRING: u8 = 0x04;
pub const TAG_NULL: u8 = 0x05;
pub const TAG_OID: u8 = 0x06;
pub const TAG_UTF8STRING: u8 = 0x0C;
pub const TAG_PRINTABLE_STRING: u8 = 0x13;
pub const TAG_IA5STRING: u8 = 0x16;
pub const TAG_UTCTIME: u8 = 0x17;
pub const TAG_GENERALIZED_TIME: u8 = 0x18;
pub const TAG_SEQUENCE: u8 = 0x30; // constructed
pub const TAG_SET: u8 = 0x31; // constructed

// ============================================================
// 长度编码
// ============================================================

/// 编码 DER 长度字段
/// 短格式: < 128 时直接用 1 字节
/// 长格式: >= 128 时用 0x80 | num_bytes, 然后大端字节
fn encode_length(len: usize) -> Vec<u8> {
    if len < 0x80 {
        vec![len as u8]
    } else {
        // 计算需要的字节数
        let mut bytes = Vec::new();
        let mut tmp = len;
        while tmp > 0 {
            bytes.push((tmp & 0xFF) as u8);
            tmp >>= 8;
        }
        bytes.reverse();
        let mut result = vec![0x80 | bytes.len() as u8];
        result.extend(bytes);
        result
    }
}

/// 组装 TLV (Tag-Length-Value)
fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(2 + value.len());
    result.push(tag);
    result.extend(encode_length(value.len()));
    result.extend_from_slice(value);
    result
}

// ============================================================
// 基本类型编码
// ============================================================

/// 编码 BOOLEAN
pub fn boolean(val: bool) -> Vec<u8> {
    tlv(TAG_BOOLEAN, &[if val { 0xFF } else { 0x00 }])
}

/// 编码 INTEGER (从 i64)
pub fn integer(val: i64) -> Vec<u8> {
    let mut bytes = if val >= 0 {
        // 非负整数: 找最小表示, 确保最高位 bit0 = 0 (否则补零字节)
        let mut v = val as u64;
        let mut buf = Vec::new();
        if v == 0 {
            buf.push(0u8);
        } else {
            while v > 0 {
                buf.push((v & 0xFF) as u8);
                v >>= 8;
            }
            buf.reverse();
            // 如果最高位是 1, 需要补一个 0x00 前导字节
            if buf[0] & 0x80 != 0 {
                buf.insert(0, 0x00);
            }
        }
        buf
    } else {
        // 负整数: 用补码表示 (较少使用, 此处简化处理)
        let mut v = val as u128; // 取低 128 位的补码
        let mut buf = Vec::new();
        for _ in 0..16 {
            buf.push((v & 0xFF) as u8);
            v >>= 8;
        }
        // 去掉前导 0xFF
        while buf.len() > 1 && buf.last() == Some(&0xFF) && buf[buf.len() - 2] & 0x80 != 0 {
            buf.pop();
        }
        buf.reverse();
        buf
    };
    tlv(TAG_INTEGER, &bytes)
}

/// 编码 INTEGER (从原始字节, 已是大端)
pub fn integer_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut padded = bytes.to_vec();
    if padded.is_empty() {
        padded.push(0);
    } else if padded[0] & 0x80 != 0 {
        padded.insert(0, 0x00);
    }
    tlv(TAG_INTEGER, &padded)
}

/// 编码 OBJECT IDENTIFIER
/// 输入: 点分字符串如 "1.2.156.10197.1.301"
pub fn oid(oid_str: &str) -> Vec<u8> {
    let parts: Vec<u64> = oid_str
        .split('.')
        .filter_map(|s| s.trim().parse::<u64>().ok())
        .collect();
    if parts.len() < 2 {
        return tlv(TAG_OID, &[]);
    }

    let mut content = Vec::new();
    // 第一个字节 = first * 40 + second
    content.push((parts[0] * 40 + parts[1]) as u8);

    // 后续部分用 base-128 编码
    for &n in &parts[2..] {
        if n < 128 {
            content.push(n as u8);
        } else {
            let mut tmp = Vec::new();
            let mut v = n;
            tmp.push((v & 0x7F) as u8);
            v >>= 7;
            while v > 0 {
                tmp.push(0x80 | (v & 0x7F) as u8);
                v >>= 7;
            }
            tmp.reverse();
            content.extend(tmp);
        }
    }

    tlv(TAG_OID, &content)
}

/// 编码 NULL
pub fn null() -> Vec<u8> {
    vec![TAG_NULL, 0x00]
}

/// 编码 OCTET STRING
pub fn octet_string(data: &[u8]) -> Vec<u8> {
    tlv(TAG_OCTET_STRING, data)
}

/// 编码 BIT STRING
/// unused_bits = 0 (通常情况)
pub fn bit_string(data: &[u8], unused_bits: u8) -> Vec<u8> {
    let mut content = Vec::with_capacity(1 + data.len());
    content.push(unused_bits);
    content.extend_from_slice(data);
    tlv(TAG_BIT_STRING, &content)
}

/// 编码 UTF8String
pub fn utf8_string(s: &str) -> Vec<u8> {
    tlv(TAG_UTF8STRING, s.as_bytes())
}

/// 编码 PrintableString
pub fn printable_string(s: &str) -> Vec<u8> {
    tlv(TAG_PRINTABLE_STRING, s.as_bytes())
}

/// 编码 IA5String (ASCII)
pub fn ia5_string(s: &str) -> Vec<u8> {
    tlv(TAG_IA5STRING, s.as_bytes())
}

/// 编码 UTCTime (YYMMDDHHMMSSZ)
pub fn utc_time(year: u32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> Vec<u8> {
    let yy = year % 100;
    let s = format!(
        "{:02}{:02}{:02}{:02}{:02}{:02}Z",
        yy, month, day, hour, min, sec
    );
    tlv(TAG_UTCTIME, s.as_bytes())
}

/// 编码 GeneralizedTime (YYYYMMDDHHMMSSZ)
pub fn generalized_time(
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    min: u32,
    sec: u32,
) -> Vec<u8> {
    let s = format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}Z",
        year, month, day, hour, min, sec
    );
    tlv(TAG_GENERALIZED_TIME, s.as_bytes())
}

// ============================================================
// 构造类型编码
// ============================================================

/// 编码 SEQUENCE (拼接多个已编码元素)
pub fn sequence(elements: &[Vec<u8>]) -> Vec<u8> {
    let mut content = Vec::new();
    for e in elements {
        content.extend_from_slice(e);
    }
    tlv(TAG_SEQUENCE, &content)
}

/// 编码 SET (拼接多个已编码元素, 调用方应确保已排序)
pub fn set(elements: &[Vec<u8>]) -> Vec<u8> {
    let mut content = Vec::new();
    for e in elements {
        content.extend_from_slice(e);
    }
    tlv(TAG_SET, &content)
}

/// 编码 CONTEXT [n] EXPLICIT (包裹一个已编码元素)
pub fn context_explicit(n: u8, element: &[u8]) -> Vec<u8> {
    let tag = 0xA0 | (n & 0x0F); // constructed context tag
    tlv(tag, element)
}

/// 编码 CONTEXT [n] IMPLICIT (直接替换标签)
pub fn context_implicit(n: u8, content: &[u8]) -> Vec<u8> {
    let tag = 0x80 | (n & 0x0F); // primitive context tag
    tlv(tag, content)
}

/// 编码 CONTEXT [n] IMPLICIT CONSTRUCTED
pub fn context_implicit_constructed(n: u8, elements: &[Vec<u8>]) -> Vec<u8> {
    let tag = 0xA0 | (n & 0x0F);
    let mut content = Vec::new();
    for e in elements {
        content.extend_from_slice(e);
    }
    tlv(tag, &content)
}

// ============================================================
// OID 常量
// ============================================================

pub mod oids {
    // PKCS#7 / CMS
    pub const PKCS7_DATA: &str = "1.2.840.113549.1.7.1";
    pub const PKCS7_SIGNED_DATA: &str = "1.2.840.113549.1.7.2";

    // 签名算法
    pub const SM2_WITH_SM3: &str = "1.2.156.10197.1.501"; // SM2 签名 (SM3 摘要)
    pub const RSA_WITH_SHA256: &str = "1.2.840.113549.1.1.11";
    pub const RSA_WITH_SHA1: &str = "1.2.840.113549.1.1.5";

    // 摘要算法
    pub const SM3: &str = "1.2.156.10197.1.401";
    pub const SHA256: &str = "2.16.840.1.101.3.4.2.1";
    pub const SHA1: &str = "1.3.14.3.2.26";

    // SM2 加密/签名
    pub const SM2: &str = "1.2.156.10197.1.301";

    // X.509 / Attribute
    pub const CONTENT_TYPE: &str = "1.2.840.113549.1.9.3";
    pub const MESSAGE_DIGEST: &str = "1.2.840.113549.1.9.4";
    pub const SIGNING_TIME: &str = "1.2.840.113549.1.9.5";
    pub const SIGNING_CERTIFICATE_V2: &str = "1.2.840.113549.1.9.16.2.47";

    // 国密 SES 相关
    pub const SES_DATA_TYPE: &str = "1.2.156.10197.6.1.4.1"; // 电子签章数据类型
    pub const SES_SEAL_TYPE: &str = "1.2.156.10197.6.1.4.2"; // 电子印章数据类型
}

// ============================================================
// AlgorithmIdentifier 编码
// ============================================================

/// 编码 AlgorithmIdentifier (算法 OID + 可选参数 NULL)
pub fn algorithm_identifier(oid_str: &str) -> Vec<u8> {
    sequence(&[oid(oid_str), null()])
}

// ============================================================
// 测试
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer() {
        // INTEGER 0
        assert_eq!(integer(0), vec![0x02, 0x01, 0x00]);
        // INTEGER 1
        assert_eq!(integer(1), vec![0x02, 0x01, 0x01]);
        // INTEGER 127
        assert_eq!(integer(127), vec![0x02, 0x01, 0x7F]);
        // INTEGER 128 (需要前导零)
        assert_eq!(integer(128), vec![0x02, 0x02, 0x00, 0x80]);
        // INTEGER 256
        assert_eq!(integer(256), vec![0x02, 0x02, 0x01, 0x00]);
    }

    #[test]
    fn test_oid() {
        // OID 1.2.840.113549.1.7.2 (PKCS#7 SignedData)
        let encoded = oid(oids::PKCS7_SIGNED_DATA);
        assert_eq!(encoded[0], 0x06); // tag
        assert_eq!(encoded[1], 0x09); // length = 9
        let content = &encoded[2..];
        assert_eq!(content[0], 42); // 1*40+2 = 42
    }

    #[test]
    fn test_sequence() {
        let seq = sequence(&[integer(1), integer(2)]);
        assert_eq!(seq[0], 0x30); // SEQUENCE tag
        assert_eq!(seq[1], 6); // length
        assert_eq!(&seq[2..], &[0x02, 0x01, 0x01, 0x02, 0x01, 0x02]);
    }

    #[test]
    fn test_bit_string() {
        let bs = bit_string(&[0x01, 0x02, 0x03], 0);
        assert_eq!(bs, vec![0x03, 0x04, 0x00, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_null() {
        assert_eq!(null(), vec![0x05, 0x00]);
    }
}

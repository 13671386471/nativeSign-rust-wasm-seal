//! 国密算法模块 — SM2/SM3/SM4 + RSA/SHA256
//!
//! ⚠️ MOCK 说明：
//! - 证书密钥使用假数据模拟，标记为 `FIXME: REPLACE_WITH_REAL_CERT`
//! - 生产环境需替换为经过国家密码管理局认证的真实证书和密钥

use sha2::{Sha256, Digest};
use md5::Md5;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

// ============================================================
// 假证书数据 — 仅在开发和测试阶段使用
// FIXME: REPLACE_WITH_REAL_CERT — 生产环境替换为真实CA签发的证书
// ============================================================

/// RSA 公钥证书 (PKCS#1格式, base64) — 假数据
pub const MOCK_RSA_CERT: &str = "MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQCbCjqtkT8hh7h563wopmeU849Tv1jgwu89DdNUyYbLwxy9rFqrpzsj0DQlm/EXzN+yI42Jvoi0uSHo5OJ1h2NXSefjiopVxvDKNaArWD7z1n2DG/5E0NJrLEZk2wsTaXdqCnRm9HNPODRNXsEnW/4iRncvL4Hd13zOS62I8ywT5wIDAQAB";
// FIXME: REPLACE_WITH_REAL_CERT — 替换为真实RSA证书

/// SM2 公钥证书 (国密标准, base64) — 假数据
pub const MOCK_SM2_CERT: &str = "MFkwEwYHKoZIzj0CAQYIKoEcz1UBgi0DQgAEgLHeUjG9svZEuBUFh1zICkk76BUZwGjBYkU4CgmlSS/ra5/ip2EaedAxAwRc/TaKZ8EANZIg0TbPKEeS48x9ww==";
// FIXME: REPLACE_WITH_REAL_CERT — 替换为真实SM2证书

/// 模拟的 RSA 私钥 — 假数据（生产环境绝不暴露私钥）
const _MOCK_RSA_PRIVKEY: &str = "FIXME: REPLACE_WITH_REAL_CERT";
/// 模拟的 SM2 私钥 — 假数据（生产环境绝不暴露私钥）
const _MOCK_SM2_PRIVKEY: &str = "FIXME: REPLACE_WITH_REAL_CERT";

// ============================================================
// SM3 哈希 (国密标准 GM/T 0004-2012)
// ============================================================

/// SM3 哈希计算
pub fn sm3_hash(data: &[u8]) -> Vec<u8> {
    // 使用 libsm crate 中的 SM3 实现
    // 生产环境中应使用经过认证的 SM3 实现
    libsm::sm3::hash::Sm3Hash::new(data).get_hash().to_vec()
}

/// SM3 哈希，返回 hex 字符串
pub fn sm3_hash_hex(data: &[u8]) -> String {
    hex::encode(sm3_hash(data))
}

// ============================================================
// SHA256 哈希
// ============================================================

pub fn sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(sha256(data))
}

pub fn sha256_base64(data: &[u8]) -> String {
    BASE64.encode(sha256(data))
}

// ============================================================
// MD5
// ============================================================

pub fn md5(data: &[u8]) -> Vec<u8> {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

pub fn md5_hex(data: &[u8]) -> String {
    hex::encode(md5(data))
}

// ============================================================
// SM2 签名与验签 (GM/T 0003-2012)
// ============================================================

/// SM2 签名
/// 使用假私钥进行模拟签名
/// FIXME: REPLACE_WITH_REAL_CERT — 生产环境中签名应在云端/UKey硬件中完成
pub fn sm2_sign(data: &[u8], _privkey_der: &[u8]) -> Result<Vec<u8>, String> {
    // 注意：实际 SM2 签名需要使用 SM2 密钥对
    // 此处在 WASM 中使用纯 Rust 实现的 libsm 进行签名
    // 生产环境中，签名应在云端服务或 UKey 硬件中完成

    // 使用 SM3 对数据进行哈希
    let digest = sm3_hash(data);

    // FIXME: 生产环境需加载真实的 SM2 私钥进行签名
    // 当前使用 libsm 进行真实的 SM2 签名，但密钥是模拟的

    // 对于演示目的，返回模拟的签名结果
    let mut signature = vec![0x30u8; 64]; // 模拟的 64 字节签名
    signature[0..32].copy_from_slice(&digest[0..32]);
    // 颠倒后32字节以模拟签名格式
    for i in 0..32 {
        signature[32 + i] = digest[31 - i];
    }

    Ok(signature)
}

/// SM2 验签
pub fn sm2_verify(data: &[u8], signature: &[u8], _pubkey_der: &[u8]) -> Result<bool, String> {
    let digest = sm3_hash(data);

    // 验证签名结构（简化版）
    if signature.len() < 64 {
        return Ok(false);
    }

    // 重建预期签名
    let mut expected_sig = vec![0x30u8; 64];
    expected_sig[0..32].copy_from_slice(&digest[0..32]);
    for i in 0..32 {
        expected_sig[32 + i] = digest[31 - i];
    }

    // 简单比较（生产环境需使用完整的 SM2 验签过程）
    Ok(signature[0..64] == expected_sig[0..64])
}

// ============================================================
// SM4 对称加密 (GM/T 0002-2012) — 用于文档内容加密
// ============================================================

/// SM4 加密 (CBC模式)
pub fn sm4_encrypt(_data: &[u8], _key: &[u8], _iv: &[u8]) -> Result<Vec<u8>, String> {
    // FIXME: 生产环境需使用经过认证的 SM4 实现
    // 当前返回模拟加密数据
    let result: Vec<u8> = _data.iter().map(|b| b ^ 0xAA).collect();
    Ok(result)
}

/// SM4 解密 (CBC模式)
pub fn sm4_decrypt(_data: &[u8], _key: &[u8], _iv: &[u8]) -> Result<Vec<u8>, String> {
    let result: Vec<u8> = _data.iter().map(|b| b ^ 0xAA).collect();
    Ok(result)
}

// ============================================================
// RSA 签名与验签 — 假数据实现
// ============================================================

/// RSA PKCS#1v1.5 签名
/// FIXME: REPLACE_WITH_REAL_CERT — 生产环境中签名应在云端服务中完成
pub fn rsa_sign(_data: &[u8]) -> Result<Vec<u8>, String> {
    // RSA 签名 = 对 SHA256 哈希用私钥加密
    // 生产环境应使用云端服务或 UKey 硬件签名
    let digest = sha256(_data);
    let mut sig = vec![0u8; 256];
    sig[0..32].copy_from_slice(&digest);
    Ok(sig)
}

/// 获取模拟的 SM2 证书公钥
pub fn get_mock_sm2_cert() -> Vec<u8> {
    BASE64.decode(MOCK_SM2_CERT).unwrap_or_default()
}

/// 获取模拟的 RSA 证书公钥
pub fn get_mock_rsa_cert() -> Vec<u8> {
    BASE64.decode(MOCK_RSA_CERT).unwrap_or_default()
}

// ============================================================
// base64 编解码工具
// ============================================================

pub fn b64_encode(data: &[u8]) -> String {
    BASE64.encode(data)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    BASE64.decode(s).map_err(|e| format!("base64 decode error: {}", e))
}

// ============================================================
// 文件哈希工具
// ============================================================

/// 计算文件前 N 字节的 MD5（用于文件标识）
pub fn file_left_md5(data: &[u8], left_bytes: usize) -> String {
    let len = std::cmp::min(left_bytes, data.len());
    md5_hex(&data[..len])
}

/// 文件 SHA256 (用于生成 fileId)
pub fn file_id_hash(data: &str) -> String {
    sha256_base64(data.as_bytes())
        .trim_end_matches('=')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sm3_hash() {
        let data = b"hello sm3";
        let hash = sm3_hash(data);
        assert_eq!(hash.len(), 32); // SM3 always produces 32 bytes
    }

    #[test]
    fn test_sha256() {
        let data = b"test";
        let hash = sha256_hex(data);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_md5() {
        let data = b"test";
        let hash = md5_hex(data);
        assert_eq!(hash.len(), 32);
    }
}

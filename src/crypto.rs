//! 国密算法模块 — SM2/SM3/SM4 + RSA/SHA256
//!
//! 实现状态:
//! - SM3: 真实现 (libsm::sm3, GM/T 0004-2012)
//! - SM2: 真实现 (libsm::sm2::signature, GM/T 0003-2012)
//! - SM4: 真实现 (libsm::sm4, CBC模式, GM/T 0002-2012)
//! - RSA: 仍为假实现 (非国密算法, 仅用于兼容)
//!
//! ⚠️ MOCK 说明：
//! - SM2 密钥对在首次使用时由 libsm 随机生成, 每次加载 WASM 时不同
//! - 证书仍为假数据, 标记为 `FIXME: REPLACE_WITH_REAL_CERT`
//! - 生产环境需替换为经过国家密码管理局认证的真实证书和密钥

use sha2::{Sha256, Digest};
use md5::Md5;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::sync::OnceLock;

use libsm::sm2::signature::{SigCtx, Signature};
use libsm::sm4::{Cipher as Sm4Cipher, Mode as Sm4Mode};

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

// ============================================================
// SM3 哈希 (国密标准 GM/T 0004-2012) — 真实实现
// ============================================================

/// SM3 哈希计算
pub fn sm3_hash(data: &[u8]) -> Vec<u8> {
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
// SM2 签名与验签 (GM/T 0003-2012) — 真实实现
// ============================================================

/// SM2 密钥对 (pubkey: 65字节非压缩格式 0x04||x||y, seckey: 32字节)
struct Sm2KeyPair {
    pubkey: Vec<u8>,
    seckey: Vec<u8>,
}

/// SM2 密钥对全局缓存 — 首次使用时由 libsm 随机生成
static SM2_KEYPAIR: OnceLock<Sm2KeyPair> = OnceLock::new();

/// 获取或生成 SM2 密钥对
///
/// 首次调用时使用 libsm 生成随机 SM2 密钥对 (sm2p256v1 曲线),
/// 后续调用直接返回缓存。密钥对在 WASM 模块生命周期内保持不变。
///
/// ⚠️ MOCK 说明: 每次加载 WASM 时密钥对不同, 不具备持久性。
/// FIXME: REPLACE_WITH_REAL_CERT — 生产环境应加载固定 SM2 证书和私钥。
fn ensure_sm2_keypair() -> &'static Sm2KeyPair {
    SM2_KEYPAIR.get_or_init(|| {
        let ctx = SigCtx::new();
        let (pk, sk) = ctx.new_keypair()
            .expect("[crypto] SM2 keypair generation failed");
        let pubkey = ctx.serialize_pubkey(&pk, false)
            .expect("[crypto] SM2 public key serialization failed");
        let seckey = ctx.serialize_seckey(&sk)
            .expect("[crypto] SM2 private key serialization failed");
        Sm2KeyPair { pubkey, seckey }
    })
}

/// 获取当前 SM2 公钥 (65字节, 非压缩格式, 0x04||x||y)
pub fn get_sm2_pubkey() -> Vec<u8> {
    ensure_sm2_keypair().pubkey.clone()
}

/// 获取当前 SM2 私钥 (32字节)
pub fn get_sm2_seckey() -> Vec<u8> {
    ensure_sm2_keypair().seckey.clone()
}

/// SM2 签名 (GM/T 0003-2012)
///
/// 使用 libsm 进行真实的 SM2 椭圆曲线数字签名。
/// 签名流程: Z_A = SM3(ID||a||b||G||P_A), e = SM3(Z_A||M),
///           生成随机 k, 计算 (r, s) = SM2_Sign(e, k, d_A)。
/// 默认用户 ID 为 "1234567812345678" (GM/T 0003-2012 推荐值)。
///
/// # 参数
/// - `data`: 待签名数据
/// - `_privkey_der`: (预留, 当前忽略) PKCS#8 DER 格式私钥,
///   生产环境应从此参数加载真实私钥
///
/// # 返回
/// DER 编码的签名值 (ASN.1 SEQUENCE { r INTEGER, s INTEGER })
///
/// ⚠️ MOCK 说明: 使用动态生成的 SM2 密钥对签名
/// FIXME: REPLACE_WITH_REAL_CERT — 生产环境应使用真实 CA 签发的 SM2 证书和私钥
pub fn sm2_sign(data: &[u8], _privkey_der: &[u8]) -> Result<Vec<u8>, String> {
    let kp = ensure_sm2_keypair();
    let ctx = SigCtx::new();
    let sk = ctx.load_seckey(&kp.seckey)
        .map_err(|e| format!("SM2 load seckey error: {}", e))?;
    let pk = ctx.load_pubkey(&kp.pubkey)
        .map_err(|e| format!("SM2 load pubkey error: {}", e))?;
    let sig = ctx.sign(data, &sk, &pk)
        .map_err(|e| format!("SM2 sign error: {}", e))?;
    Ok(sig.der_encode())
}

/// SM2 验签 (GM/T 0003-2012)
///
/// 使用 libsm 进行真实的 SM2 椭圆曲线签名验证。
///
/// # 参数
/// - `data`: 原始数据
/// - `signature`: DER 编码的签名值 (ASN.1 SEQUENCE { r INTEGER, s INTEGER })
/// - `_pubkey_der`: (预留, 当前忽略) PKCS#8 DER 格式公钥
///
/// # 返回
/// `Ok(true)` 验签通过, `Ok(false)` 验签失败, `Err` 格式错误
pub fn sm2_verify(data: &[u8], signature: &[u8], _pubkey_der: &[u8]) -> Result<bool, String> {
    let kp = ensure_sm2_keypair();
    let ctx = SigCtx::new();
    let pk = ctx.load_pubkey(&kp.pubkey)
        .map_err(|e| format!("SM2 load pubkey error: {}", e))?;
    let sig = Signature::der_decode(signature)
        .map_err(|e| format!("SM2 signature DER decode error: {}", e))?;
    ctx.verify(data, &pk, &sig)
        .map_err(|e| format!("SM2 verify error: {}", e))
}

// ============================================================
// SM4 对称加密 (GM/T 0002-2012) — 真实实现, CBC模式
// ============================================================

/// SM4 加密 (CBC模式, PKCS#7 自动填充)
///
/// 使用 libsm 实现真实的 SM4 分组密码加密。
/// - 密钥长度: 128 位 (16 字节)
/// - IV 长度: 128 位 (16 字节)
/// - 填充模式: PKCS#7 (CBC模式自动处理)
///
/// # 参数
/// - `data`: 明文数据 (任意长度)
/// - `key`: 密钥 (必须 16 字节)
/// - `iv`: 初始化向量 (必须 16 字节)
///
/// # 返回
/// 密文数据 (长度为 16 的整数倍)
pub fn sm4_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, String> {
    if key.len() != 16 {
        return Err(format!("SM4 key must be 16 bytes, got {}", key.len()));
    }
    if iv.len() != 16 {
        return Err(format!("SM4 IV must be 16 bytes, got {}", iv.len()));
    }
    let cipher = Sm4Cipher::new(key, Sm4Mode::Cbc)
        .map_err(|e| format!("SM4 cipher init error: {}", e))?;
    cipher.encrypt(&[], data, iv)
        .map_err(|e| format!("SM4 encrypt error: {}", e))
}

/// SM4 解密 (CBC模式, PKCS#7 自动去填充)
///
/// # 参数
/// - `data`: 密文数据 (长度必须是 16 的整数倍)
/// - `key`: 密钥 (必须 16 字节)
/// - `iv`: 初始化向量 (必须 16 字节)
///
/// # 返回
/// 明文数据
pub fn sm4_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>, String> {
    if key.len() != 16 {
        return Err(format!("SM4 key must be 16 bytes, got {}", key.len()));
    }
    if iv.len() != 16 {
        return Err(format!("SM4 IV must be 16 bytes, got {}", iv.len()));
    }
    let cipher = Sm4Cipher::new(key, Sm4Mode::Cbc)
        .map_err(|e| format!("SM4 cipher init error: {}", e))?;
    cipher.decrypt(&[], data, iv)
        .map_err(|e| format!("SM4 decrypt error: {}", e))
}

// ============================================================
// RSA 签名 — 假数据实现 (非国密算法, 仅用于兼容)
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

// ============================================================
// 测试
// ============================================================

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

    // --- SM2 签名/验签测试 ---

    #[test]
    fn test_sm2_keypair_gen() {
        let kp = ensure_sm2_keypair();
        assert_eq!(kp.pubkey.len(), 65); // 04 || x(32) || y(32)
        assert_eq!(kp.pubkey[0], 0x04);  // 非压缩格式前缀
        assert_eq!(kp.seckey.len(), 32); // 256-bit private key
    }

    #[test]
    fn test_sm2_sign_and_verify() {
        let msg = b"hello world, SM2 test message";
        let sig = sm2_sign(msg, &[]).expect("SM2 sign should succeed");
        // DER 编码的签名: SEQUENCE { r INTEGER, s INTEGER }
        assert_eq!(sig[0], 0x30); // SEQUENCE tag
        assert!(sig.len() >= 70 && sig.len() <= 72); // 典型 DER SM2 签名长度

        let ok = sm2_verify(msg, &sig, &[]).expect("SM2 verify should not error");
        assert!(ok, "SM2 verify should pass for valid signature");
    }

    #[test]
    fn test_sm2_verify_wrong_data() {
        let msg = b"original message";
        let sig = sm2_sign(msg, &[]).expect("SM2 sign should succeed");

        let ok = sm2_verify(b"tampered message", &sig, &[]).expect("SM2 verify should not error");
        assert!(!ok, "SM2 verify should fail for tampered data");
    }

    // --- SM4 加密/解密测试 ---

    #[test]
    fn test_sm4_cbc_roundtrip() {
        let key = [0x01u8; 16];
        let iv = [0x02u8; 16];
        let plaintext = b"hello world, this is a SM4 CBC test message";

        let ciphertext = sm4_encrypt(plaintext, &key, &iv).expect("SM4 encrypt should succeed");
        assert_eq!(ciphertext.len() % 16, 0); // 密文长度必须是 16 的倍数
        assert_ne!(&ciphertext[..], &plaintext[..]); // 密文不能等于明文

        let decrypted = sm4_decrypt(&ciphertext, &key, &iv).expect("SM4 decrypt should succeed");
        assert_eq!(&decrypted[..], &plaintext[..]); // 解密后应等于明文
    }

    #[test]
    fn test_sm4_cbc_empty() {
        let key = [0xAAu8; 16];
        let iv = [0xBBu8; 16];
        let plaintext = b"";

        let ciphertext = sm4_encrypt(plaintext, &key, &iv).expect("SM4 encrypt empty should succeed");
        // 空数据加密后应有一个完整填充块 (16 字节)
        assert_eq!(ciphertext.len(), 16);

        let decrypted = sm4_decrypt(&ciphertext, &key, &iv).expect("SM4 decrypt should succeed");
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_sm4_cbc_exact_block() {
        let key = [0x33u8; 16];
        let iv = [0x44u8; 16];
        let plaintext = [0x55u8; 16]; // 正好一个块

        let ciphertext = sm4_encrypt(&plaintext, &key, &iv).expect("SM4 encrypt should succeed");
        // 恰好一个块时, PKCS#7 会添加一个完整填充块 → 32 字节
        assert_eq!(ciphertext.len(), 32);

        let decrypted = sm4_decrypt(&ciphertext, &key, &iv).expect("SM4 decrypt should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_sm4_wrong_key_length() {
        let result = sm4_encrypt(b"data", &[0u8; 15], &[0u8; 16]);
        assert!(result.is_err());
    }

    #[test]
    fn test_sm4_wrong_iv_length() {
        let result = sm4_encrypt(b"data", &[0u8; 16], &[0u8; 15]);
        assert!(result.is_err());
    }
}

#!/usr/bin/env python3
"""
生成 MOCK X.509 证书用于 SES 签章结构测试
输出 Rust 代码格式的 const 字节数组
"""
import datetime
from cryptography import x509
from cryptography.x509.oid import NameOID
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa, ec

def gen_rsa_cert():
    """生成 RSA 自签名证书"""
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    subject = issuer = x509.Name([
        x509.NameAttribute(NameOID.COUNTRY_NAME, "CN"),
        x509.NameAttribute(NameOID.ORGANIZATION_NAME, "DianJu Test CA"),
        x509.NameAttribute(NameOID.ORGANIZATIONAL_UNIT_NAME, "Mock Certificate"),
        x509.NameAttribute(NameOID.COMMON_NAME, "test-signer@dianju.com"),
    ])
    cert = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(issuer)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(datetime.datetime(2025, 1, 1))
        .not_valid_after(datetime.datetime(2030, 12, 31))
        .add_extension(
            x509.BasicConstraints(ca=False, path_length=None),
            critical=True,
        )
        .add_extension(
            x509.KeyUsage(
                digital_signature=True,
                content_commitment=True,
                key_encipherment=True,
                data_encipherment=False,
                key_agreement=False,
                key_cert_sign=False,
                crl_sign=False,
                encipher_only=False,
                decipher_only=False,
            ),
            critical=True,
        )
        .sign(key, hashes.SHA256())
    )
    return cert, key

def gen_ec_cert():
    """生成 EC P-256 自签名证书 (SM2 替代)"""
    key = ec.generate_private_key(ec.SECP256R1())
    subject = issuer = x509.Name([
        x509.NameAttribute(NameOID.COUNTRY_NAME, "CN"),
        x509.NameAttribute(NameOID.ORGANIZATION_NAME, "DianJu Test SM2 CA"),
        x509.NameAttribute(NameOID.ORGANIZATIONAL_UNIT_NAME, "Mock SM2 Certificate"),
        x509.NameAttribute(NameOID.COMMON_NAME, "sm2-signer@dianju.com"),
    ])
    cert = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(issuer)
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(datetime.datetime(2025, 1, 1))
        .not_valid_after(datetime.datetime(2030, 12, 31))
        .add_extension(
            x509.BasicConstraints(ca=False, path_length=None),
            critical=True,
        )
        .sign(key, hashes.SHA256())
    )
    return cert, key

def to_rust_bytes(name, data):
    """将字节数据格式化为 Rust const 数组"""
    lines = []
    lines.append(f"pub const {name}: &[u8] = &[")
    for i in range(0, len(data), 12):
        chunk = data[i:i+12]
        hex_vals = ", ".join(f"0x{b:02x}" for b in chunk)
        lines.append(f"    {hex_vals},")
    lines.append("];")
    return "\n".join(lines)

def main():
    rsa_cert, rsa_key = gen_rsa_cert()
    ec_cert, ec_key = gen_ec_cert()

    rsa_der = rsa_cert.public_bytes(serialization.Encoding.DER)
    ec_der = ec_cert.public_bytes(serialization.Encoding.DER)

    rsa_key_der = rsa_key.private_bytes(
        serialization.Encoding.DER,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )
    ec_key_der = ec_key.private_bytes(
        serialization.Encoding.DER,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )

    rsa_pub_der = rsa_cert.public_key().public_bytes(
        serialization.Encoding.DER,
        serialization.PublicFormat.SubjectPublicKeyInfo,
    )
    ec_pub_der = ec_cert.public_key().public_bytes(
        serialization.Encoding.DER,
        serialization.PublicFormat.SubjectPublicKeyInfo,
    )

    print(f"// RSA 证书: {len(rsa_der)} bytes, 序列号: {rsa_cert.serial_number}")
    print(f"// EC 证书: {len(ec_der)} bytes, 序列号: {ec_cert.serial_number}")
    print()
    print(to_rust_bytes("MOCK_RSA_CERT_DER", rsa_der))
    print()
    print(to_rust_bytes("MOCK_RSA_PUBKEY_DER", rsa_pub_der))
    print()
    print(to_rust_bytes("MOCK_RSA_PRIVKEY_DER", rsa_key_der))
    print()
    print(to_rust_bytes("MOCK_EC_CERT_DER", ec_der))
    print()
    print(to_rust_bytes("MOCK_EC_PUBKEY_DER", ec_pub_der))
    print()
    print(to_rust_bytes("MOCK_EC_PRIVKEY_DER", ec_key_der))

    # 也保存到文件
    import os
    out_dir = os.path.join(os.path.dirname(__file__), "..", "mock_data")
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "mock_rsa_cert.der"), "wb") as f:
        f.write(rsa_der)
    with open(os.path.join(out_dir, "mock_ec_cert.der"), "wb") as f:
        f.write(ec_der)
    with open(os.path.join(out_dir, "mock_rsa_key.der"), "wb") as f:
        f.write(rsa_key_der)
    with open(os.path.join(out_dir, "mock_ec_key.der"), "wb") as f:
        f.write(ec_key_der)

    # 输出证书信息
    print("\n// === 证书信息 ===")
    print(f"// RSA cert subject: {rsa_cert.subject.rfc4514_string()}")
    print(f"// RSA cert serial: {rsa_cert.serial_number}")
    print(f"// RSA cert not_before: {rsa_cert.not_valid_before_utc}")
    print(f"// RSA cert not_after: {rsa_cert.not_valid_after_utc}")
    print(f"// EC cert subject: {ec_cert.subject.rfc4514_string()}")
    print(f"// EC cert serial: {ec_cert.serial_number}")
    print(f"// EC cert not_before: {ec_cert.not_valid_before_utc}")
    print(f"// EC cert not_after: {ec_cert.not_valid_after_utc}")

if __name__ == "__main__":
    main()

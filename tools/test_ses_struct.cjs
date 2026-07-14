// 验证 SES 签章结构的正确性
const path = require('path');
const wasm = require(path.join(__dirname, '..', 'pkg_node', 'dianju_wasm_seal.js'));

async function main() {
    console.log('=== SES 签章结构验证 ===\n');

    // 1. 构建 SES 电子印章 (Seal.esl)
    console.log('1. 构建 SES_Seal (电子印章)...');
    const sealB64 = wasm.build_ses_seal('测试电子印章', '', 'sm2');
    const sealDer = Buffer.from(sealB64, 'base64');
    console.log(`   SES_Seal DER 大小: ${sealDer.length} bytes`);
    console.log(`   DER 起始字节: ${sealDer.slice(0, 4).toString('hex')}`);
    console.log(`   预期: 30 82 xx xx (SEQUENCE)\n`);

    // 2. 构建 SES 签名 (SES_Signature)
    console.log('2. 构建 SES_Signature (电子签名)...');
    const docData = Buffer.from('测试文档内容 - SES签名验证', 'utf8');
    const docB64 = docData.toString('base64');
    const sigB64 = wasm.build_ses_signature(docB64, 'sm2');
    const sigDer = Buffer.from(sigB64, 'base64');
    console.log(`   SES_Signature DER 大小: ${sigDer.length} bytes`);
    console.log(`   DER 起始字节: ${sigDer.slice(0, 4).toString('hex')}\n`);

    // 3. 构建 PKCS#7 SignedData
    console.log('3. 构建 PKCS#7 SignedData...');
    const pkcs7B64 = wasm.build_pkcs7_signature(docB64, 'sm2');
    const pkcs7Der = Buffer.from(pkcs7B64, 'base64');
    console.log(`   PKCS#7 DER 大小: ${pkcs7Der.length} bytes`);
    console.log(`   DER 起始字节: ${pkcs7Der.slice(0, 4).toString('hex')}`);
    console.log(`   预期: 30 82 xx xx (SEQUENCE - ContentInfo)\n`);

    // 4. 验证 DER 结构 (ASN.1 解析)
    console.log('4. ASN.1 DER 结构验证:');

    // 验证 SES_Seal 结构
    verifyAsn1(sealDer, 'SES_Seal', [
        { name: 'eSealInfo (SEQUENCE)', tag: 0x30 },
        { name: 'signatureAlgo (OID)', tag: 0x06 },
        { name: 'signature (BIT STRING)', tag: 0x03 },
    ]);

    // 验证 PKCS#7 结构
    verifyPkcs7(pkcs7Der);

    // 5. 使用 RSA 算法验证
    console.log('\n5. RSA 算法验证...');
    const rsaSealB64 = wasm.build_ses_seal('RSA测试印章', '', 'rsa');
    const rsaSealDer = Buffer.from(rsaSealB64, 'base64');
    console.log(`   RSA SES_Seal DER 大小: ${rsaSealDer.length} bytes`);

    const rsaPkcs7B64 = wasm.build_pkcs7_signature(docB64, 'rsa');
    const rsaPkcs7Der = Buffer.from(rsaPkcs7B64, 'base64');
    console.log(`   RSA PKCS#7 DER 大小: ${rsaPkcs7Der.length} bytes\n`);

    console.log('=== 验证完成 ===');
    console.log('\n结论:');
    console.log(`  - SES_Seal 结构: ${sealDer[0] === 0x30 ? '✓ 正确' : '✗ 错误'}`);
    console.log(`  - SES_Signature 结构: ${sigDer[0] === 0x30 ? '✓ 正确' : '✗ 错误'}`);
    console.log(`  - PKCS#7 SignedData 结构: ${pkcs7Der[0] === 0x30 ? '✓ 正确' : '✗ 错误'}`);
    console.log(`  - RSA 兼容性: ${rsaSealDer[0] === 0x30 ? '✓ 正确' : '✗ 错误'}`);
}

function verifyAsn1(der, name, expectedFields) {
    console.log(`\n   ${name} DER 解析:`);

    // 简单 ASN.1 解析器
    let pos = 0;
    const tag = der[pos];
    const lenInfo = readLength(der, pos + 1);
    pos = lenInfo.nextPos;

    console.log(`   外层 SEQUENCE: tag=0x${tag.toString(16)}, length=${lenInfo.length}`);

    // 解析内部字段
    let fieldIdx = 0;
    while (pos < der.length && fieldIdx < expectedFields.length) {
        const fieldTag = der[pos];
        const fieldLen = readLength(der, pos + 1);
        const expected = expectedFields[fieldIdx];

        const tagMatch = fieldTag === expected.tag;
        console.log(`   - ${expected.name}: tag=0x${fieldTag.toString(16)} ${tagMatch ? '✓' : '✗ (预期 0x' + expected.tag.toString(16) + ')'}`);

        pos = fieldLen.nextPos + fieldLen.length;
        fieldIdx++;
    }
}

function verifyPkcs7(der) {
    console.log('\n   PKCS#7 ContentInfo DER 解析:');

    let pos = 0;
    // 外层 SEQUENCE (ContentInfo)
    const outerTag = der[pos];
    const outerLen = readLength(der, pos + 1);
    pos = outerLen.nextPos;
    console.log(`   ContentInfo SEQUENCE: tag=0x${outerTag.toString(16)}, length=${outerLen.length}`);

    // contentType OID
    if (der[pos] === 0x06) {
        const oidLen = readLength(der, pos + 1);
        const oidData = der.slice(oidLen.nextPos, oidLen.nextPos + oidLen.length);
        const oidStr = decodeOid(oidData);
        console.log(`   contentType OID: ${oidStr} ${oidStr === '1.2.840.113549.1.7.2' ? '✓ (signedData)' : '✗'}`);
        pos = oidLen.nextPos + oidLen.length;
    }

    // content [0] EXPLICIT
    if (der[pos] === 0xA0) {
        const ctxLen = readLength(der, pos + 1);
        pos = ctxLen.nextPos;
        console.log(`   content [0] EXPLICIT: tag=0xA0, length=${ctxLen.length}`);

        // SignedData SEQUENCE
        if (der[pos] === 0x30) {
            const sdLen = readLength(der, pos + 1);
            pos = sdLen.nextPos;
            console.log(`   SignedData SEQUENCE: tag=0x30, length=${sdLen.length}`);

            // version INTEGER
            if (der[pos] === 0x02) {
                const vLen = readLength(der, pos + 1);
                const version = der[vLen.nextPos];
                console.log(`   version: ${version} ${version === 1 ? '✓' : '✗'}`);
                pos = vLen.nextPos + vLen.length;
            }

            // digestAlgorithms SET
            if (der[pos] === 0x31) {
                const daLen = readLength(der, pos + 1);
                console.log(`   digestAlgorithms SET: tag=0x31, length=${daLen.length}`);
                pos = daLen.nextPos + daLen.length;
            }

            // encapContentInfo SEQUENCE
            if (der[pos] === 0x30) {
                const ecLen = readLength(der, pos + 1);
                console.log(`   encapContentInfo SEQUENCE: tag=0x30, length=${ecLen.length}`);
                pos = ecLen.nextPos + ecLen.length;
            }

            // certificates [0] IMPLICIT
            if (der[pos] === 0xA0) {
                const certLen = readLength(der, pos + 1);
                console.log(`   certificates [0]: tag=0xA0, length=${certLen.length}`);
                pos = certLen.nextPos + certLen.length;
            }

            // signerInfos SET
            if (der[pos] === 0x31) {
                const siLen = readLength(der, pos + 1);
                console.log(`   signerInfos SET: tag=0x31, length=${siLen.length}`);
                pos = siLen.nextPos + siLen.length;
            }
        }
    }
}

function readLength(data, pos) {
    const b = data[pos];
    if (b < 0x80) {
        return { length: b, nextPos: pos + 1 };
    }
    const numBytes = b & 0x7F;
    let len = 0;
    for (let i = 0; i < numBytes; i++) {
        len = (len << 8) | data[pos + 1 + i];
    }
    return { length: len, nextPos: pos + 1 + numBytes };
}

function decodeOid(data) {
    if (data.length === 0) return '';
    let result = `${Math.floor(data[0] / 40)}.${data[0] % 40}`;
    let i = 1;
    while (i < data.length) {
        let val = 0;
        while (i < data.length) {
            val = (val << 7) | (data[i] & 0x7F);
            if ((data[i] & 0x80) === 0) {
                i++;
                break;
            }
            i++;
        }
        result += `.${val}`;
    }
    return result;
}

main().catch(console.error);

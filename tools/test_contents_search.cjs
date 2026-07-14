// 测试 find_contents_hex_range 的 lopdf 输出格式兼容性
// lopdf 0.34 need_separator 对 String 类型返回 false,
// 输出 /Contents<000...000> 而非 /Contents <000...000>

const path = require('path');
const wasm = require(path.join(__dirname, '..', 'pkg_node', 'dianju_wasm_seal.js'));

// 手动构造模拟 lopdf 输出的两种格式, 验证搜索逻辑

function findContentsHexRange(data, sigObjId) {
    const prefix = `${sigObjId} 0 obj`;
    const prefixBytes = Buffer.from(prefix);
    
    // 找到签名字典对象定义
    let objPos = -1;
    for (let i = 0; i <= data.length - prefixBytes.length; i++) {
        if (data.slice(i, i + prefixBytes.length).equals(prefixBytes)) {
            objPos = i;
            break;
        }
    }
    if (objPos === -1) return null;
    
    const searchStart = objPos + prefixBytes.length;
    const contentsKey = Buffer.from('/Contents');
    
    // 找 /Contents
    let contentsPos = -1;
    for (let i = searchStart; i <= data.length - contentsKey.length; i++) {
        if (data.slice(i, i + contentsKey.length).equals(contentsKey)) {
            contentsPos = i;
            break;
        }
    }
    if (contentsPos === -1) return null;
    
    const afterKey = searchStart + contentsPos - searchStart + contentsKey.length;
    
    // 跳过空白找 <
    let p = afterKey;
    while (p < data.length && (data[p] === 0x20 || data[p] === 0x0A || data[p] === 0x0D || data[p] === 0x09)) {
        p++;
    }
    if (p >= data.length || data[p] !== 0x3C) return null; // '<'
    
    const hexStart = p + 1;
    
    // 找 >
    let hexEnd = hexStart;
    while (hexEnd < data.length) {
        const b = data[hexEnd];
        if (b === 0x3E) break; // '>'
        hexEnd++;
    }
    if (hexEnd >= data.length) return null;
    
    return { hexStart, hexEnd };
}

// 测试用例 1: lopdf 输出格式 - 无空格 (实际格式)
const noSpacePdf = Buffer.from(
    `29 0 obj\n<< /Type/Sig /Filter/Adobe.PPKLite /SubFilter/adbe.pkcs7.detached /ByteRange[0 0 0 0] /Contents<${'00'.repeat(8192)}> /M(D:20260710163000+08'00') /Name(SigSeal_0) /Reason(Sign) >>\nendobj\n`
);

// 测试用例 2: 带 space 格式 (旧假设)
const withSpacePdf = Buffer.from(
    `29 0 obj\n<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached /ByteRange [0 0 0 0] /Contents <${'00'.repeat(8192)}> /M (D:20260710163000+08'00') /Name (SigSeal_0) /Reason (Sign) >>\nendobj\n`
);

console.log('=== 测试 findContentsHexRange 兼容性 ===\n');

const result1 = findContentsHexRange(noSpacePdf, 29);
console.log('无空格格式 (lopdf 实际输出):');
if (result1) {
    console.log(`  ✅ 找到! hexStart=${result1.hexStart}, hexEnd=${result1.hexEnd}`);
    console.log(`  hex 长度=${result1.hexEnd - result1.hexStart}, 期望=${8192 * 2}`);
    // 验证 hexStart 后面是 hex 字符
    const firstChars = noSpacePdf.slice(result1.hexStart, result1.hexStart + 10).toString('ascii');
    console.log(`  前10字符: "${firstChars}" (应为全 0)`);
} else {
    console.log('  ❌ 未找到!');
}

const result2 = findContentsHexRange(withSpacePdf, 29);
console.log('\n带空格格式 (旧假设):');
if (result2) {
    console.log(`  ✅ 找到! hexStart=${result2.hexStart}, hexEnd=${result2.hexEnd}`);
    console.log(`  hex 长度=${result2.hexEnd - result2.hexStart}, 期望=${8192 * 2}`);
} else {
    console.log('  ❌ 未找到!');
}

console.log('\n=== 测试完成 ===');

// 在 Node 中运行真实构建出的 WASM, 验证 preprocess_pdf_for_cjk 运行时行为
// 用法: node tools/node_test_embed.cjs
const fs = require('fs');
const path = require('path');

const ROOT = 'D:/workspace/self/rust-wasm-seal';
const wasm = require('../pkg_node/dianju_wasm_seal.js');

// 1. 注册中文字体 (模拟浏览器端 boot 流程中的 register_font)
const fontData = fs.readFileSync(path.join(ROOT, 'fonts/NotoSansSC-Regular.otf'));
wasm.register_font('notosanssc', new Uint8Array(fontData));
console.log('[test] 已注册字体, 字节:', fontData.length);

function process(srcName, dstName) {
  const src = path.join(ROOT, srcName);
  const data = fs.readFileSync(src);
  console.log(`\n[test] 处理 ${srcName} (${data.length} 字节)`);
  const out = wasm.preprocess_pdf_for_cjk(new Uint8Array(data));
  console.log(`[test]   -> 输出 ${out.length} 字节 (delta ${out.length - data.length})`);
  fs.writeFileSync(path.join(ROOT, dstName), Buffer.from(out));
  return out;
}

// 主用例: 含未嵌入 CID 中文字体 (STSong-Light) 的劳动合同 PDF
process('test_labor_contract.pdf', 'test_labor_contract.embedded_by_wasm.pdf');
// WinAnsi 简单字体用例 (Helvetica)
process('test_helvetica.pdf', 'test_helvetica.embedded_by_wasm.pdf');
// 幂等性: 已嵌入 ToUnicode 的旧产物不应被破坏
process('sample.embedded.pdf', 'sample.embedded.twice_by_wasm.pdf');

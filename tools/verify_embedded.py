#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""验证 WASM 预处理产出的 PDF: 解析、页数、嵌入字体、ToUnicode、文本提取。"""
import sys
from pypdf import PdfReader

def analyze(path, label):
    print(f"\n===== {label}: {path} =====")
    try:
        reader = PdfReader(path)
    except Exception as e:
        print("  [FAIL] 无法解析 PDF:", e)
        return False

    n = len(reader.pages)
    print(f"  页数: {n}")

    text = ""
    for i, page in enumerate(reader.pages):
        try:
            t = page.extract_text() or ""
        except Exception as e:
            t = f"<提取异常:{e}>"
        text += t
        if i == 0:
            print(f"  第0页文本前160字符: {repr(t[:160])}")

    cjk = [c for c in text if '\u4e00' <= c <= '\u9fff']
    printable = [c for c in text if c.isprintable()]
    tofu = text.count('\ufffd')
    print(f"  提取总字符数: {len(text)} | 可打印: {len(printable)} | CJK汉字: {len(cjk)} | 豆腐码U+FFFD: {tofu}")

    ok = (len(cjk) > 20) and (tofu == 0)
    print(f"  结论: {'PASS (含真实中文且无豆腐)' if ok else 'CHECK (需人工确认)'}")
    return ok

if __name__ == "__main__":
    base = "D:/workspace/self/rust-wasm-seal"
    results = {}
    results['labor_contract_wasm'] = analyze(f"{base}/test_labor_contract.embedded_by_wasm.pdf",
                                             "WASM预处理: 劳动合同(CID中文字体)")
    results['helvetica_wasm'] = analyze(f"{base}/test_helvetica.embedded_by_wasm.pdf",
                                         "WASM预处理: Helvetica(WinAnsi)")
    results['embedded_twice'] = analyze(f"{base}/sample.embedded.twice_by_wasm.pdf",
                                         "WASM再处理: 已嵌入PDF(幂等)")
    # 对照: 原始劳动合同 (无 ToUnicode, 预期提取差)
    results['labor_contract_orig'] = analyze(f"{base}/test_labor_contract.pdf",
                                             "对照: 原始劳动合同(预期乱码/空)")

    print("\n===== 汇总 =====")
    for k, v in results.items():
        print(f"  {k}: {'PASS' if v else 'CHECK/FAIL'}")
    sys.exit(0 if results.get('labor_contract_wasm') else 1)

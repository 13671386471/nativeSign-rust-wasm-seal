#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""检查 WASM 产物 page0 内容流中的奇数长度 hex 字符串及其上下文。"""
import re
from pypdf import PdfReader

def hex_strings_with_issue(path):
    r = PdfReader(path)
    page = r.pages[0]
    data = page.get_contents().get_data()
    text = data.decode('latin-1')
    print(f"[info] content length={len(text)} bytes")
    # 找所有 <...> hex 串
    hexes = re.findall(r'<([0-9A-Fa-f]*)>', text)
    odd = [h for h in hexes if len(h) % 2 != 0]
    print(f"[info] 总 hex 串: {len(hexes)} | 奇数长度: {len(odd)}")
    for h in odd[:15]:
        # 上下文
        idx = text.find('<' + h + '>')
        ctx = text[max(0, idx-30): idx+len(h)+10]
        print(f"  ODD hex '{h}' (len {len(h)}) ctx=...{repr(ctx)}...")
    # 同时统计 Tj/TJ 中正常偶数 hex 串长度分布
    lens = {}
    for h in hexes:
        if len(h) % 2 == 0:
            lens[len(h)] = lens.get(len(h), 0) + 1
    print(f"[info] 偶数 hex 串长度分布(前几): {dict(sorted(lens.items())[:8])}")

if __name__ == "__main__":
    import sys
    base = "D:/workspace/self/rust-wasm-seal"
    for fn in ["test_labor_contract.embedded_by_wasm.pdf", "sample.embedded.pdf"]:
        print("\n##########", fn)
        try:
            hex_strings_with_issue(f"{base}/{fn}")
        except Exception as e:
            print("  ERROR:", e)

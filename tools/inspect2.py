#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""诊断: 打印 page0 第一个 Tj 字符串的真实字节, 以及 ToUnicode 映射, 手工核对。"""
import re
from pypdf import PdfReader

def get_content(pdf):
    r = PdfReader(pdf)
    page = r.pages[0]
    return page.get_contents().get_data().decode('latin-1')

def get_tounicode(pdf):
    r = PdfReader(pdf)
    page = r.pages[0]
    res = page.get("/Resources") or {}
    fonts = res.get("/Font") if hasattr(res, "get") else None
    # 遍历字体找 ToUnicode
    out = []
    if fonts:
        for k, v in fonts.items():
            try:
                fobj = v.get_object()
                tu = fobj.get("/ToUnicode")
                if tu:
                    out.append(tu.get_object().get_data().decode('latin-1'))
            except Exception:
                pass
    return out

def parse_bfchar(cmap):
    # 简单解析 bfchar 段: <cid> <unicode>
    m = {}
    for mm in re.finditer(r'<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>', cmap):
        cid = int(mm.group(1), 16)
        uni = int(mm.group(2), 16)
        m[cid] = uni
    return m

def first_tj_hex(content):
    # 找第一个 (...) 或 <...> 紧跟 Tj
    # 简化: 找所有 <hex> Tj
    for mm in re.finditer(r'<([0-9A-Fa-f]*)>\s*Tj', content):
        return mm.group(1)
    # literal
    for mm in re.finditer(r'\((.*?)\)\s*Tj', content):
        return mm.group(1).encode('latin-1').hex()
    return None

if __name__ == "__main__":
    base = "D:/workspace/self/rust-wasm-seal"
    for fn in ["test_labor_contract.embedded_by_wasm.pdf", "test_labor_contract.pdf"]:
        print("\n##########", fn)
        try:
            c = get_content(f"{base}/{fn}")
            h = first_tj_hex(c)
            print("  first Tj hex:", h)
            if h and len(h) % 2 == 0:
                b = bytes.fromhex(h)
                print("  Tj bytes:", b[:24].hex())
                # 作为 big-endian 2字节码
                codes = [ (b[i]<<8)|b[i+1] for i in range(0, min(len(b),16), 2) ]
                print("  2字节码(BE):", [hex(x) for x in codes])
            tus = get_tounicode(f"{base}/{fn}")
            print("  ToUnicode 段数:", len(tus))
            if tus:
                m = parse_bfchar(tus[0])
                print("  ToUnicode 样例 (前5):", dict(list(m.items())[:5]))
                if h and len(h)%2==0 and m:
                    b = bytes.fromhex(h)
                    uni = []
                    for i in range(0, len(b)-1, 2):
                        cid = (b[i]<<8)|b[i+1]
                        u = m.get(cid)
                        uni.append(chr(u) if u else '?')
                    print("  用ToUnicode解出的文本:", repr(''.join(uni[:20])))
        except Exception as e:
            print("  ERROR:", repr(e))

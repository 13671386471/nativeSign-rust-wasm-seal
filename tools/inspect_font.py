#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""打印原始 PDF 与 WASM 产物中页面字体字典的 Subtype/Encoding/ToUnicode/BaseFont。"""
from pypdf import PdfReader

def dump_fonts(pdf):
    r = PdfReader(pdf)
    page = r.pages[0]
    res = page.get("/Resources") or {}
    fonts = res.get("/Font")
    lines = []
    if fonts:
        for k, v in fonts.items():
            try:
                f = v.get_object()
                sub = f.get("/Subtype")
                enc = f.get("/Encoding")
                base = f.get("/BaseFont")
                has_tu = f.get("/ToUnicode") is not None
                # descendant
                df = f.get("/DescendantFonts")
                dfinfo = ""
                if df:
                    for d in df:
                        dd = d.get_object()
                        dfinfo = f"descSub={dd.get('/Subtype')} dfEnc={dd.get('/CIDSystemInfo')}"
                lines.append(f"  {k}: Subtype={sub} BaseFont={base} Encoding={enc} ToUnicode={has_tu} {dfinfo}")
            except Exception as e:
                lines.append(f"  {k}: ERR {e}")
    return lines

if __name__ == "__main__":
    base = "D:/workspace/self/rust-wasm-seal"
    for fn in ["test_labor_contract.pdf", "test_labor_contract.embedded_by_wasm.pdf"]:
        print("\n##########", fn)
        for l in dump_fonts(f"{base}/{fn}"):
            print(l)

#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""对比: 原始 / Python产物 / WASM产物 在 pypdf 下的解析与文本提取。"""
import io, sys, contextlib, json
from pypdf import PdfReader

def analyze(path):
    warn_buf = io.StringIO()
    cjk = 0
    n = 0
    first = ""
    with contextlib.redirect_stderr(warn_buf):
        try:
            r = PdfReader(path)
            n = len(r.pages)
            txt = ""
            for i, p in enumerate(r.pages):
                try:
                    t = p.extract_text() or ""
                except Exception:
                    t = ""
                txt += t
                if i == 0:
                    first = t[:80]
            cjk = sum(1 for c in txt if '\u4e00' <= c <= '\u9fff')
        except Exception as e:
            return {"err": str(e), "n": 0, "cjk": 0, "warn": 0}
    warns = warn_buf.getvalue().count("Odd-length")
    return {"n": n, "cjk": cjk, "warn": warns, "first": first}

if __name__ == "__main__":
    base = "D:/workspace/self/rust-wasm-seal"
    rep = []
    for label, fn in [
        ("原始 test_labor_contract.pdf", "test_labor_contract.pdf"),
        ("Python sample.embedded.pdf", "sample.embedded.pdf"),
        ("WASM test_labor_contract.embedded_by_wasm.pdf", "test_labor_contract.embedded_by_wasm.pdf"),
        ("WASM test_helvetica.embedded_by_wasm.pdf", "test_helvetica.embedded_by_wasm.pdf"),
    ]:
        d = analyze(f"{base}/{fn}")
        if "err" in d:
            line = f"{label}: PARSE ERROR {d['err']}"
        else:
            line = f"{label}: pages={d['n']} CJK={d['cjk']} odd_hex_warn={d['warn']} first={d['first']!r}"
        rep.append(line)
        print(line.encode('ascii', 'replace').decode())  # 控制台只打 ASCII 安全版
    with open(f"{base}/compare_report.txt", "w", encoding="utf-8") as f:
        f.write("\n".join(rep) + "\n")

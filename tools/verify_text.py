#!/usr/bin/env python3
# 对比原始 PDF 与 WASM 自动嵌入产物的中文文本提取, 确认字符码改写正确
from pypdf import PdfReader

def extract(path):
    r = PdfReader(path)
    out = []
    for p in r.pages:
        try:
            out.append(p.extract_text() or "")
        except Exception as e:
            out.append(f"<提取异常:{e}>")
    return "\n".join(out)

orig = extract("D:/workspace/self/rust-wasm-seal/test_labor_contract.pdf")
new = extract("D:/workspace/self/rust-wasm-seal/test_labor_contract.embedded_by_wasm.pdf")

import re
cjk = re.compile(r"[\u3400-\u9fff\uf900-\ufaff]")
def cjkset(t):
    return set(cjk.findall(t))

so, sn = cjkset(orig), cjkset(new)
print(f"原始提取 CJK 字符数(去重): {len(so)}")
print(f"嵌入后提取 CJK 字符数(去重): {len(sn)}")
print(f"原始与嵌入共有: {len(so & sn)}")
print(f"原始有但嵌入缺失: {len(so - sn)}")
print(f"嵌入有但原始无(可能是 tofu/异常): {len(sn - so)}")

# 抽样: 原始前 300 字 vs 嵌入前 300 字
print("\n--- 原文抽样(前200字) ---")
print(orig[:200])
print("\n--- 嵌入后抽样(前200字) ---")
print(new[:200])

# 检查嵌入后是否出现 tofu 标志 (U+FFFD 或 .notdef 常见)
tofu = new.count("\ufffd")
print(f"\n嵌入后 U+FFFD(tofu) 出现次数: {tofu}")

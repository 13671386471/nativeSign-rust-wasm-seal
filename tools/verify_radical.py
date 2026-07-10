#!/usr/bin/env python3
# 聚焦验证: 嵌入产物的文本提取中是否仍泄漏 Kangxi 字根 / 兼容区 / 字根补充码点。
# 期望: 这些非标准码点应被标准统一汉字取代 (⼀->一, ⺠->民 ...)。
import re
from pypdf import PdfReader

ROOT = "D:/workspace/self/rust-wasm-seal"

def extract(path):
    r = PdfReader(path)
    out = []
    for p in r.pages:
        try:
            out.append(p.extract_text() or "")
        except Exception as e:
            out.append(f"<提取异常:{e}>")
    return "\n".join(out)

orig = extract(f"{ROOT}/test_labor_contract.pdf")
new = extract(f"{ROOT}/test_labor_contract.embedded_by_wasm.pdf")

KANGXI = re.compile(r"[\u2F00-\u2FDF]")         # Kangxi 字根
RADSUP = re.compile(r"[\u2E80-\u2EFF]")         # CJK 字根补充
COMPAT = re.compile(r"[\uF900-\uFAFF]")         # CJK 兼容区
CJK = re.compile(r"[\u3400-\u9fff]")

def counter(t, rx):
    return {}
for label, rx in [("Kangxi字根", KANGXI), ("字根补充", RADSUP), ("兼容区", COMPAT)]:
    o = rx.findall(orig)
    n = rx.findall(new)
    print(f"{label}: 原始={len(o)}  嵌入={len(n)}")
    if n:
        from collections import Counter
        c = Counter(n)
        sample = ", ".join(f"{ch}(U+{ord(ch):04X})x{cnt}" for ch, cnt in c.most_common(12))
        print(f"   嵌入出现: {sample}")

so, sn = set(CJK.findall(orig)), set(CJK.findall(new))
print(f"\n标准CJK统一汉字: 原始去重={len(so)}  嵌入去重={len(sn)}")
print(f"  原始有嵌入缺: {''.join(sorted(so - sn)) or '无'}")
print(f"  嵌入有原始无: {''.join(sorted(sn - so)) or '无'}")

tofu = new.count("\ufffd")
print(f"\nU+FFFD(tofu): {tofu}")
print(f"嵌入总字符数: {len(new)}  原始总字符数: {len(orig)}")

#!/usr/bin/env python3
# 诊断 test_labor_contract.pdf 的字体与 ToUnicode, 确认方案 A 替换是否可行
import sys
from pypdf import PdfReader
from pypdf.generic import IndirectObject, DictionaryObject

PATH = "D:/workspace/self/rust-wasm-seal/test_labor_contract.pdf"
reader = PdfReader(PATH)
print("页数:", len(reader.pages))

def resolve(o):
    if isinstance(o, IndirectObject):
        return o.get_object()
    return o

for pi, page in enumerate(reader.pages):
    res = page.get("/Resources")
    if res is None:
        continue
    res = resolve(res)
    fonts = res.get("/Font") if hasattr(res, "get") else None
    if fonts is None:
        continue
    fonts = resolve(fonts)
    for key, fref in fonts.items():
        fdict = resolve(fref)
        if not isinstance(fdict, DictionaryObject):
            continue
        base = fdict.get("/BaseFont")
        base = str(base) if base is not None else None
        subtype = fdict.get("/Subtype")
        subtype = str(subtype) if subtype is not None else None
        enc = fdict.get("/Encoding")
        enc = str(enc) if enc is not None else None
        has_tu = "/ToUnicode" in fdict
        print(f"\n[页{pi}] 资源键={key} BaseFont={base} Subtype={subtype} Encoding={enc} ToUnicode={has_tu}")
        if has_tu:
            tu = resolve(fdict["/ToUnicode"])
            filt = tu.get("/Filter") if hasattr(tu, "get") else None
            filt = str(filt) if filt is not None else None
            raw = tu.get_data() if hasattr(tu, "get_data") else None
            print(f"  ToUnicode /Filter={filt} 解压后字节数={len(raw) if raw else 0}")
            # 原始(压缩)字节数: 通过 stream 的 _data 或 encoded_data
            rawbytes = getattr(tu, "_data", None)
            if rawbytes is not None:
                print(f"  ToUnicode 原始(压缩)字节数={len(rawbytes)}")
            data = raw
            if data:
                txt = data.decode("latin-1", "replace")
                # 打印前若干 bfchar 映射, 看是否指向 CJK
                import re
                head = txt[:1500]
                print("  ToUnicode 前 1500 字节:")
                print("   " + head.replace("\n", "\n   "))
                # 统计 CJK 命中
                cjk = 0
                for m in re.finditer(r"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>", txt):
                    u = int(m.group(2), 16)
                    if 0x3400 <= u <= 0x9FFF or 0xF900 <= u <= 0xFAFF or 0x20000 <= u <= 0x2FA1F:
                        cjk += 1
                print(f"  ToUnicode 中 CJK 映射命中: {cjk}")

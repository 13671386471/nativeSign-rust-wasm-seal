#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成 unicode -> cid 映射表 (二进制), 供 Rust 运行时嵌字体使用。

关键: NotoSansSC 是 CID-keyed CFF 字体, ttf-parser 的 glyph_index 返回的是
字体内部的 GID (如 中=8805), 而用于 Identity-H 编码的正确 CID 必须从 CFF 字形名
'cidNNNNN' 解析得到 (中=9544)。本脚本用 fonttools 正确解析, 输出二进制表。

输出格式 (小端):
  u32 count
  随后 count 条记录, 每条 8 字节:
    u32 unicode_codepoint
    u32 cid
"""
import struct
import sys
from fontTools.ttLib import TTFont

FONT = r"D:/workspace/self/rust-wasm-seal/fonts/NotoSansSC-Regular.otf"
OUT = r"D:/workspace/self/rust-wasm-seal/fonts/uni2cid.bin"


def build_unicode_to_cid(font_path):
    f = TTFont(font_path)
    if 'CFF ' not in f:
        raise RuntimeError('字体不是 CFF/OTF: %s' % font_path)
    cmap = f.getBestCmap()  # {unicode: glyphName}
    go = f.getGlyphOrder()
    mapping = {}
    for u, gname in cmap.items():
        if gname.startswith('cid'):
            try:
                cid = int(gname[3:])
            except ValueError:
                continue
        else:
            try:
                cid = go.index(gname)
            except ValueError:
                continue
        mapping[u] = cid
    f.close()
    return mapping


def main():
    m = build_unicode_to_cid(FONT)
    print("映射条目数: %d" % len(m))
    for u in (0x4E2D, 0x6587, 0x5408, 0x540C):
        print("  U+%04X -> CID %d" % (u, m.get(u, -1)))

    with open(OUT, "wb") as fh:
        fh.write(struct.pack("<I", len(m)))
        for u in sorted(m.keys()):
            fh.write(struct.pack("<II", u, m[u]))
    print("已写出: %s" % OUT)


if __name__ == "__main__":
    main()

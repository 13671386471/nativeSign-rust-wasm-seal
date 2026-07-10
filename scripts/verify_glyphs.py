#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
验证嵌入后的 PDF 是否真的渲染出中文字形 (而非豆腐块 □)。
无法肉眼看图, 用"字形形状多样性"做程序化判定:
  - 真实中文: 每页有成百上千个不同字形 -> distinct 哈希数很大
  - 豆腐块:   所有汉字都渲染成同一个 □ 框 -> distinct 哈希数≈1~3
通过投影法把页面墨迹分割成"字形级"小方块, 归一化后做哈希, 统计 distinct 数。
"""
import sys
import numpy as np
from PIL import Image
import pypdfium2 as pp


def load_page_bitmap(pdf_path, page_index):
    doc = pp.PdfDocument(pdf_path)
    page = doc[page_index]
    bitmap = page.render(scale=1.5)
    arr = np.array(bitmap.to_pil().convert('L'))  # 灰度
    doc.close()
    return arr


def segment_glyphs(gray):
    """返回每个字形小块的归一化 16x16 二值哈希集合。"""
    h, w = gray.shape
    # 二值化: 墨迹 = 暗 (<200)
    ink = (gray < 200).astype(np.uint8)
    # 行投影
    row_proj = ink.sum(axis=1)
    # 找文本行 (连续有墨的行)
    lines = []
    in_line = False
    start = 0
    for r in range(h):
        if row_proj[r] > 0 and not in_line:
            in_line = True; start = r
        elif row_proj[r] == 0 and in_line:
            in_line = False; lines.append((start, r))
    if in_line:
        lines.append((start, h))

    hashes = set()
    glyph_count = 0
    for (r0, r1) in lines:
        line_h = r1 - r0
        if line_h < 4:  # 太矮, 忽略 (可能是噪声)
            continue
        line_ink = ink[r0:r1, :]
        col_proj = line_ink.sum(axis=0)
        # 列投影找字形间隔 (gap)
        in_g = False
        cstart = 0
        for c in range(w):
            if col_proj[c] > 0 and not in_g:
                in_g = True; cstart = c
            elif col_proj[c] == 0 and in_g:
                in_g = False
                # 处理一个字形段 [cstart, c)
                seg = line_ink[:, cstart:c]
                # 取墨迹包围盒
                ys, xs = np.where(seg > 0)
                if len(xs) == 0:
                    continue
                bh = ys.max() - ys.min() + 1
                bw = xs.max() - xs.min() + 1
                # 过滤: 太宽/太高 (表格横线竖线) 或太小 (噪声点)
                if bh < 5 or bh > 60 or bw < 2 or bw > 60:
                    continue
                crop = seg[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
                # 归一化到 16x16
                im = Image.fromarray((crop * 255).astype(np.uint8))
                im = im.resize((16, 16))
                binar = (np.array(im) > 127).astype(np.uint8)
                hkey = binar.tobytes()
                hashes.add(hkey)
                glyph_count += 1
    return glyph_count, len(hashes)


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else r"D:/workspace/self/rust-wasm-seal/sample.embedded.pdf"
    doc = pp.PdfDocument(path)
    n = len(doc)
    print("文件:", path, "页数:", n)
    doc.close()
    for i in range(n):
        gray = load_page_bitmap(path, i)
        gc, distinct = segment_glyphs(gray)
        ratio = (distinct / gc) if gc else 0
        verdict = "REAL_TEXT(many distinct shapes)" if distinct >= 15 else "SUSPECT_TOFU(few shapes)"
        print("  page[%d]: 字形块=%d, 不同形状=%d, 多样性=%.2f -> %s" % (i, gc, distinct, ratio, verdict))


if __name__ == "__main__":
    main()

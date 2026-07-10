#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
把未嵌入字体的 PDF (sample.pdf) 中引用的中文(STSong-Light/CID)与西文(Helvetica)
替换为嵌入式 NotoSansSC 字体, 产出可在 PDFium (含 WASM 构建) 中正确渲染中文的 PDF。

关键修正 (前两次失败的根因):
  NotoSansSC-Regular.otf 是 CFF 字体, CID-keyed, ROS=(Adobe,Identity,0),
  其字形顺序为 cid00001, cid00002, ... 即 **CID == GID** (顺序编号)。
  cmap: U+4E2D(中) -> GID cid09544, 即 CID 9544。
  PDF 内容流里用的是 UCS-2BE 的 *Unicode* 码点, 若直接当作 Identity-H 的 CID,
  CID=0x4E2D 会映射到字体里 GID=20013 的字形 (错误字形 / .notdef -> 豆腐块)。
  因此必须: 把内容流里的 Unicode 码点改写为字体实际的 CID(=GID), 再配合
  /Subtype /CIDFontType0 + /FontFile3 /Subtype /CIDFontType0C + /Encoding /Identity-H。

方案 (CIDFontType0 + CFF 嵌入, 标准做法):
  - 构建 unicode -> cid(GID) 映射 (从 cmap 与字形名 cidNNNNN 解析)
  - /F2 (中文, UCS-2BE 2字节码): 每个码点 u -> 2字节 cid
  - /F1 (西文, 1字节 WinAnsi): 每个字节 b -> WinAnsi->Unicode -> 2字节 cid
  - 字体字典: Type0 / Identity-H, 后代 CIDFontType0 /CIDFontType0C, 嵌入 OTF 字节
"""
import pikepdf
from fontTools.ttLib import TTFont


# ---------- 字体解析: 建立 unicode -> cid(GID) 映射 ----------

def build_unicode_to_cid(font_path):
    f = TTFont(font_path)
    if 'CFF ' not in f:
        raise RuntimeError('字体不是 CFF/OTF: %s' % font_path)
    cmap = f.getBestCmap()  # {unicode: glyphName}
    mapping = {}
    for u, gname in cmap.items():
        # 字形名为 'cidNNNNN' -> CID = NNNNN (此字体 CID==GID 顺序)
        if gname.startswith('cid'):
            try:
                cid = int(gname[3:])
            except ValueError:
                continue
        else:
            # 非 CID 字形 (理论上不会在 CID-keyed 字体出现): 用字形序作为 CID
            go = f.getGlyphOrder()
            try:
                cid = go.index(gname)
            except ValueError:
                continue
        mapping[u] = cid
    f.close()
    return mapping


def winansi_to_unicode(b):
    # WinAnsiEncoding -> Unicode (cp1252 覆盖大部分可见字符)
    # 控制区 0x80-0x9F 在 WinAnsi 有定义, cp1252 中部分未定义, 用替换字符兜底
    try:
        return bytes([b]).decode('cp1252')
    except Exception:
        return '\ufffd'


def parse_hex(b):
    h = b.decode('latin-1').replace(' ', '').replace('\t', '').replace('\r', '').replace('\n', '')
    if len(h) % 2 == 1:
        h += '0'
    return bytes.fromhex(h)


def unescape_literal(b):
    out = bytearray()
    i = 0
    n = len(b)
    while i < n:
        c = b[i]
        if c == ord('\\'):
            i += 1
            if i >= n:
                break
            d = b[i]
            if d == ord('n'): out.append(0x0A)
            elif d == ord('r'): out.append(0x0D)
            elif d == ord('t'): out.append(0x09)
            elif d == ord('b'): out.append(0x08)
            elif d == ord('f'): out.append(0x0C)
            elif d == ord('('): out.append(ord('('))
            elif d == ord(')'): out.append(ord(')'))
            elif d == ord('\\'): out.append(ord('\\'))
            elif 0x30 <= d <= 0x37:
                val = 0; cnt = 0
                while i < n and cnt < 3 and 0x30 <= b[i] <= 0x37:
                    val = val * 8 + (b[i] - 0x30); i += 1; cnt += 1
                out.append(val & 0xFF)
                continue
            else:
                out.append(d)
            i += 1
        else:
            out.append(c)
            i += 1
    return bytes(out)


def tokenize(data):
    toks = []
    i = 0
    n = len(data)
    while i < n:
        c = data[i]
        if c in b' \t\r\n\f':
            j = i
            while j < n and data[j] in b' \t\r\n\f':
                j += 1
            toks.append(('ws', i, j, data[i:j])); i = j
        elif c == ord('('):
            depth = 1; j = i + 1; buf = bytearray()
            while j < n and depth > 0:
                d = data[j]
                if d == ord('\\'):
                    buf.append(d)
                    if j + 1 < n:
                        buf.append(data[j + 1])
                    j += 2; continue
                if d == ord('('): depth += 1
                elif d == ord(')'):
                    depth -= 1
                    if depth == 0: break
                buf.append(d); j += 1
            toks.append(('str_lit', i, j + 1, bytes(buf))); i = j + 1
        elif c == ord('<'):
            if i + 1 < n and data[i + 1] == ord('<'):
                toks.append(('other', i, i + 2, data[i:i + 2])); i += 2; continue
            j = i + 1
            while j < n and data[j] != ord('>'): j += 1
            toks.append(('str_hex', i, j + 1, data[i + 1:j])); i = j + 1
        elif c == ord('/'):
            j = i + 1
            while j < n and not (data[j] in b' \t\r\n\f()<>[]/'): j += 1
            toks.append(('name', i, j, data[i:j])); i = j
        elif c in b'0123456789+-.':
            j = i
            while j < n and (data[j] in b'0123456789+-.eE'): j += 1
            toks.append(('num', i, j, data[i:j])); i = j
        elif c in b'[]':
            toks.append(('other', i, i + 1, data[i:i + 1])); i += 1
        elif c in b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz':
            j = i
            while j < n and data[j] in b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz': j += 1
            toks.append(('op', i, j, data[i:j])); i = j
        else:
            toks.append(('op', i, i + 1, data[i:i + 1])); i += 1
    return toks


def process_content(data, uni2cid):
    """返回 (rewritten_bytes, f2_count, f1_count, missing_count)。
    把 /F2 的 Unicode 码点、/F1 的 WinAnsi 字节, 都改写为 2字节 cid (Identity-H)。"""
    toks = tokenize(data)
    f2_count = 0
    f1_count = 0
    missing = 0
    rewritten = [None] * len(toks)
    last_str = None
    cur = None
    font_at = [None] * len(toks)
    for ti, (t, s, e, txt) in enumerate(toks):
        if t == 'name':
            ns = txt.decode('latin-1')
            if ns.startswith('/F'):
                for k in range(ti + 1, min(ti + 6, len(toks))):
                    if toks[k][0] == 'op' and toks[k][3] == b'Tf':
                        cur = ns; break
        font_at[ti] = cur
        if t in ('str_lit', 'str_hex'):
            last_str = ti
        if t == 'op' and txt == b'Tj':
            if last_str is not None:
                st, ss, se, stxt = toks[last_str]
                raw = parse_hex(stxt) if st == 'str_hex' else unescape_literal(stxt)
                fnt = font_at[last_str]
                out_codes = bytearray()
                if fnt == '/F2':
                    # UCS-2BE 2字节一码
                    if len(raw) % 2 != 0:
                        raw = raw + b'\x00'
                    for k in range(0, len(raw) - 1, 2):
                        u = int.from_bytes(raw[k:k + 2], 'big')
                        cid = uni2cid.get(u)
                        if cid is None:
                            cid = 0; missing += 1
                        out_codes += cid.to_bytes(2, 'big')
                        f2_count += 1
                elif fnt == '/F1':
                    for bb in raw:
                        ch = winansi_to_unicode(bb)
                        u = ord(ch)
                        cid = uni2cid.get(u)
                        if cid is None:
                            cid = 0; missing += 1
                        out_codes += cid.to_bytes(2, 'big')
                        f1_count += 1
                else:
                    out_codes = bytearray(raw)
                rewritten[last_str] = b'<' + out_codes.hex().encode('latin-1') + b'>'
    out = []
    pos = 0
    for ti, (t, s, e, txt) in enumerate(toks):
        out.append(data[pos:s])
        out.append(rewritten[ti] if rewritten[ti] is not None else data[s:e])
        pos = e
    out.append(data[pos:])
    return b''.join(out), f2_count, f1_count, missing


def build_to_unicode_cid(uni2cid):
    """构建 ToUnicode CMap 流数据: CID(Identity-H) -> Unicode。
    PDFium 需要 ToUnicode 才能把 CID 正确还原为可显示的 Unicode 字符。"""
    # 反转映射: cid -> unicode
    cid2uni = {}
    for u, cid in uni2cid.items():
        cid2uni[cid] = u

    lines = [
        b'/CIDInit /ProcSet findresource begin',
        b'12 dict begin',
        b'begincmap',
        b'/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def',
        b'/CMapName /Adobe-Identity-UCS def',
        b'/CMapType 2 def',
        b'1 begincodespacerange',
        b'<0000> <FFFF>',
        b'endcodespacerange',
    ]

    # 按 CID 排序输出 bfchar 条目
    sorted_cids = sorted(cid2uni.items())
    # 分段输出 (每段最多 100 条)
    chunk_size = 100
    for i in range(0, len(sorted_cids), chunk_size):
        chunk = sorted_cids[i:i + chunk_size]
        lines.append(('%d beginbfchar' % len(chunk)).encode())
        for cid, uni in chunk:
            lines.append(('<%04X> <%04X>' % (cid, uni)).encode())
        lines.append(b'endbfchar')

    lines.extend([
        b'endcmap',
        b'CMapName currentdict /CMap defineresource pop',
        b'end end',
    ])
    return b'\n'.join(lines)


def build_cidfont_type0(pdf, font_bytes, fontfile_obj, uni2cid=None, base_name="/NotoSansSC-Regular"):
    """构建 CIDFontType0 + Type0(Identity-H) 字体字典对 (CFF 嵌入)。"""
    descriptor = pikepdf.Dictionary({
        '/Type': pikepdf.Name('/FontDescriptor'),
        '/FontName': pikepdf.Name(base_name),
        '/Flags': 6,
        '/FontBBox': pikepdf.Array([-25, -254, 1000, 880]),
        '/ItalicAngle': 0,
        '/Ascent': 752,
        '/Descent': -271,
        '/CapHeight': 737,
        '/StemV': 58,
        '/FontFile3': fontfile_obj,
    })
    # 设置 FontFile3 子类型 (CFF)
    descriptor.FontFile3 = fontfile_obj
    fontfile_obj.Subtype = pikepdf.Name('/CIDFontType0C')

    cidfont = pikepdf.Dictionary({
        '/Type': pikepdf.Name('/Font'),
        '/Subtype': pikepdf.Name('/CIDFontType0'),
        '/BaseFont': pikepdf.Name(base_name),
        '/CIDSystemInfo': pikepdf.Dictionary({
            '/Registry': pikepdf.String('(Adobe)'),
            '/Ordering': pikepdf.String('(Identity)'),
            '/Supplement': 0,
        }),
        '/FontDescriptor': descriptor,
        '/DW': 1000,
    })

    type0 = pikepdf.Dictionary({
        '/Type': pikepdf.Name('/Font'),
        '/Subtype': pikepdf.Name('/Type0'),
        '/BaseFont': pikepdf.Name(base_name),
        '/Encoding': pikepdf.Name('/Identity-H'),
        '/DescendantFonts': pikepdf.Array([cidfont]),
    })

    # 关键修复: 添加 ToUnicode CMap (PDFium 需要它来正确显示中文)
    if uni2cid is not None:
        tou_data = build_to_unicode_cid(uni2cid)
        tou_stream = pdf.make_stream(tou_data)
        type0['/ToUnicode'] = tou_stream

    return type0


def main():
    import sys
    SRC = r"D:/工作文档/sample.pdf"
    FONT = r"D:/workspace/self/rust-wasm-seal/fonts/NotoSansSC-Regular.otf"
    OUT = r"D:/workspace/self/rust-wasm-seal/sample.embedded.pdf"

    print("[1] 解析字体, 建立 unicode->cid 映射:", FONT)
    uni2cid = build_unicode_to_cid(FONT)
    print("    映射条目数: %d" % len(uni2cid))

    print("[2] 打开 PDF:", SRC)
    pdf = pikepdf.open(SRC)
    with open(FONT, "rb") as fh:
        FONT_BYTES = fh.read()
    print("    字体大小: %d 字节" % len(FONT_BYTES))

    print("[3] 构建共享字体对象 (仅嵌入一次 8.3MB 字体):")
    fontfile_obj = pdf.make_stream(FONT_BYTES)
    fontfile_obj.Length1 = len(FONT_BYTES)
    f2_font = build_cidfont_type0(pdf, FONT_BYTES, fontfile_obj, uni2cid=uni2cid)
    f1_font = build_cidfont_type0(pdf, FONT_BYTES, fontfile_obj, uni2cid=uni2cid)

    print("[4] 逐页重写内容流 (Unicode码点 -> CID) 并挂接字体:")
    total_missing = 0
    total_f2 = 0
    total_f1 = 0
    for pi, page in enumerate(pdf.pages):
        c = page.Contents
        if isinstance(c, pikepdf.Array):
            streams = [bytes(s.get_stream_buffer()) for s in c]
        elif isinstance(c, pikepdf.Stream):
            streams = [bytes(c.get_stream_buffer())]
        else:
            streams = []
        if not streams:
            continue
        new_streams = []
        for sd in streams:
            nd, c2, c1, miss = process_content(sd, uni2cid)
            total_missing += miss; total_f2 += c2; total_f1 += c1
            new_streams.append(pdf.make_stream(nd))
        if len(new_streams) == 1:
            page.Contents = new_streams[0]
        else:
            page.Contents = pikepdf.Array(new_streams)

        # 替换字体字典 (引用共享字体对象, 保存时只存一份)
        res = page.get("/Resources")
        if res is None:
            continue
        fonts = res.get("/Font")
        if fonts is None:
            continue
        if "/F2" in fonts:
            fonts["/F2"] = f2_font
            print("    page[%d]: /F2 -> CIDFontType0+CIDFontType0C" % pi)
        if "/F1" in fonts:
            fonts["/F1"] = f1_font
            print("    page[%d]: /F1 -> CIDFontType0+CIDFontType0C" % pi)

    print("[4] 统计: /F2 字符=%d, /F1 字符=%d, 缺失映射=%d" % (total_f2, total_f1, total_missing))
    print("[5] 保存:", OUT)
    pdf.save(OUT)
    pdf.close()
    print("完成。")


if __name__ == "__main__":
    main()

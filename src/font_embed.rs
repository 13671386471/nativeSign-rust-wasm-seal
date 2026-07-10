//! 运行时自动嵌入中文字体 — 方案 A 核心实现
//!
//! PDFium WASM 对「未嵌入的 CID 中文字体」(如 STSong-Light) 不会回调系统字体
//! 提供器 (MapFont=0)，导致整页空白或乱码。根本修复是在加载 PDF 前, 把
//! NotoSansSC 字体真正嵌入 PDF, 并把内容流中的字符码改写为字体真实的 CID。
//!
//! 字符码 → Unicode → NotoSansSC CID 的正确推导:
//!   * 源字体是 Unicode 类编码 (UniGB-UCS2-H / UTF-16 等): 字符码即 Unicode 码点
//!   * 源字体是 Identity-H/V 且内嵌了 TrueType (CIDFontType2): 字符码=CID=GID,
//!     用内嵌字体 cmap 的 GID→Unicode 反查得到 Unicode
//!   * 源字体带有效 ToUnicode: 直接用其 CID→Unicode
//!   * 简单字体 (WinAnsi): 1 字节 cp1252 → Unicode
//! 最后 Unicode → NotoSansSC CID (来自预生成的 uni2cid.bin)。

use std::collections::{BTreeSet, HashMap};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use unicode_normalization::UnicodeNormalization;

/// 预生成的 unicode -> cid 映射表 (由 scripts/gen_uni2cid.py 生成)。
/// 注: NotoSansSC 是 CID-keyed CFF 字体, ttf-parser 的 glyph_index 返回的是
/// 字体内部 GID (如 中=8805), 而正确 CID 须从 CFF 字形名 cidNNNNN 解析 (中=9544)。
/// 故此处直接嵌入由 fonttools 正确生成的映射表, 避免 ttf-parser 的歧义。
const UNI2CID_BIN: &[u8] = include_bytes!("../fonts/uni2cid.bin");

/// 字符码编码方式
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CodeEncoding {
    /// 2 字节 Unicode (UCS-2 / UTF-16BE), 如 UniGB-UCS2-H / UniJIS-UTF16-H
    Unicode2Byte,
    /// 1 字节 WinAnsi (cp1252), 如 Helvetica 的 WinAnsiEncoding
    WinAnsi1Byte,
    /// Identity-H/V: 字符码即 CID (需经 cmap/ToUnicode 反查 Unicode)
    IdentityCid,
}

/// 从内嵌的二进制映射表加载 unicode -> cid
///
/// 表格式 (小端): u32 count, 随后 count 条 { u32 unicode, u32 cid }
fn build_unicode_to_cid() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    if UNI2CID_BIN.len() < 4 {
        return map;
    }
    let count =
        u32::from_le_bytes([UNI2CID_BIN[0], UNI2CID_BIN[1], UNI2CID_BIN[2], UNI2CID_BIN[3]])
            as usize;
    let mut off = 4;
    for _ in 0..count {
        if off + 8 > UNI2CID_BIN.len() {
            break;
        }
        let u = u32::from_le_bytes([
            UNI2CID_BIN[off],
            UNI2CID_BIN[off + 1],
            UNI2CID_BIN[off + 2],
            UNI2CID_BIN[off + 3],
        ]);
        let cid = u32::from_le_bytes([
            UNI2CID_BIN[off + 4],
            UNI2CID_BIN[off + 5],
            UNI2CID_BIN[off + 6],
            UNI2CID_BIN[off + 7],
        ]);
        map.insert(u, cid);
        off += 8;
    }
    map
}

/// 字体字典是否已嵌入 (含 FontFile / FontFile2 / FontFile3, 含后代)
fn is_font_embedded(doc: &Document, dict: &Dictionary) -> bool {
    if font_has_fontfile(dict) {
        return true;
    }
    if let Ok(df) = dict.get(b"DescendantFonts".as_ref()) {
        if let Object::Array(arr) = df {
            for d in arr.iter() {
                if let Object::Reference(rid) = d {
                    if let Ok(cdf) = doc.get_dictionary(*rid) {
                        if font_has_fontfile(&cdf) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn font_has_fontfile(dict: &Dictionary) -> bool {
    const KEYS: [&[u8]; 3] = [b"FontFile", b"FontFile2", b"FontFile3"];
    for k in KEYS {
        if dict.get(k).is_ok() {
            return true;
        }
    }
    false
}

/// 取字体字典内嵌字体字节 (优先 TrueType 的 FontFile2, 其次 CFF 的 FontFile3)
fn get_embedded_font_bytes(doc: &Document, dict: &Dictionary) -> Option<Vec<u8>> {
    for key in [b"FontFile2".as_ref(), b"FontFile3".as_ref(), b"FontFile".as_ref()] {
        if let Ok(ff) = dict.get(key) {
            let stream_ref: &Object = match ff {
                Object::Reference(rid) => doc.get_object(*rid).ok()?,
                _ => ff,
            };
            if let Ok(s) = stream_ref.as_stream() {
                return Some(s.content.clone());
            }
        }
    }
    if let Ok(df) = dict.get(b"DescendantFonts".as_ref()) {
        if let Object::Array(arr) = df {
            for d in arr.iter() {
                if let Object::Reference(rid) = d {
                    if let Ok(cdf) = doc.get_dictionary(*rid) {
                        if let Some(b) = get_embedded_font_bytes(doc, &cdf) {
                            return Some(b);
                        }
                    }
                }
            }
        }
    }
    None
}

/// 取字体字典的 ToUnicode 流字节
fn get_font_tounicode(doc: &Document, dict: &Dictionary) -> Option<Vec<u8>> {
    if let Ok(tu) = dict.get(b"ToUnicode".as_ref()) {
        let stream_ref: &Object = match tu {
            Object::Reference(rid) => doc.get_object(*rid).ok()?,
            _ => tu,
        };
        if let Ok(s) = stream_ref.as_stream() {
            return s.decompressed_content().ok().or_else(|| Some(s.content.clone()));
        }
    }
    None
}

/// 根据字体字典推断内容流字符码编码方式
fn detect_encoding(font_dict: &Dictionary) -> Option<CodeEncoding> {
    let subtype = font_dict.get(b"Subtype".as_ref()).ok();
    let is_type0 = matches!(subtype, Some(Object::Name(s)) if s.as_slice() == b"Type0");

    let encoding = font_dict.get(b"Encoding".as_ref()).ok();
    let enc_lower = match encoding {
        Some(Object::Name(n)) => String::from_utf8_lossy(n).to_lowercase(),
        _ => String::new(),
    };

    if is_type0 {
        if enc_lower.contains("identity") {
            return Some(CodeEncoding::IdentityCid);
        }
        if enc_lower.contains("ucs2")
            || enc_lower.contains("utf16")
            || enc_lower.contains("ucs")
        {
            return Some(CodeEncoding::Unicode2Byte);
        }
        // 非 Unicode 的 Adobe 排序 (GBK/GB/CNS/B5 等) 需要额外 CMap, 暂不处理
        if let Some(Object::Dictionary(cm)) = encoding {
            if let Ok(ordering) = cm.get(b"Ordering".as_ref()) {
                if let Object::String(o, _) = ordering {
                    let ostr = String::from_utf8_lossy(o).to_lowercase();
                    if ostr.contains("gb")
                        || ostr.contains("cns")
                        || ostr.contains("japan")
                        || ostr.contains("korea")
                    {
                        return None;
                    }
                }
            }
        }
        // 兜底: 当作 2 字节 Unicode
        Some(CodeEncoding::Unicode2Byte)
    } else {
        Some(CodeEncoding::WinAnsi1Byte)
    }
}

// ============================================================
// TrueType cmap 解析 — GID -> Unicode
// ============================================================

fn parse_truetype_gid_to_unicode(ttf: &[u8]) -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    if ttf.len() < 12 {
        return map;
    }
    let num_tables = u16::from_be_bytes([ttf[4], ttf[5]]) as usize;
    let mut cmap_off: Option<usize> = None;
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if rec + 16 > ttf.len() {
            break;
        }
        if &ttf[rec..rec + 4] == b"cmap" {
            cmap_off = Some(u32::from_be_bytes([ttf[rec + 8], ttf[rec + 9], ttf[rec + 10], ttf[rec + 11]]) as usize);
            break;
        }
    }
    let cmap_off = match cmap_off {
        Some(o) => o,
        None => return map,
    };
    if cmap_off + 4 > ttf.len() {
        return map;
    }
    let num_sub = u16::from_be_bytes([ttf[cmap_off + 2], ttf[cmap_off + 3]]) as usize;
    // 选 Unicode 子表: 优先 platform=0, 其次 (3,1)/(3,10)
    let mut chosen: Option<usize> = None;
    for i in 0..num_sub {
        let e = cmap_off + 4 + i * 8;
        if e + 8 > ttf.len() {
            break;
        }
        let plat = u16::from_be_bytes([ttf[e], ttf[e + 1]]);
        let enc = u16::from_be_bytes([ttf[e + 2], ttf[e + 3]]);
        let off = u32::from_be_bytes([ttf[e + 4], ttf[e + 5], ttf[e + 6], ttf[e + 7]]) as usize;
        let is_unicode = plat == 0 || (plat == 3 && (enc == 1 || enc == 10));
        if is_unicode {
            if chosen.is_none() || plat == 0 {
                chosen = Some(cmap_off + off);
            }
        }
    }
    let sub = match chosen {
        Some(s) => s,
        None => return map,
    };
    if sub + 2 > ttf.len() {
        return map;
    }
    let fmt = u16::from_be_bytes([ttf[sub], ttf[sub + 1]]);
    match fmt {
        4 => parse_cmap4(ttf, sub, &mut map),
        12 => parse_cmap12(ttf, sub, &mut map),
        _ => {}
    }
    map
}

fn parse_cmap4(ttf: &[u8], base: usize, map: &mut HashMap<u32, u32>) {
    if base + 14 > ttf.len() {
        return;
    }
    let segcount = u16::from_be_bytes([ttf[base + 6], ttf[base + 7]]) as usize / 2;
    if segcount == 0 {
        return;
    }
    let e = base + 14;
    let mut endcode = Vec::with_capacity(segcount);
    for i in 0..segcount {
        endcode.push(u16::from_be_bytes([ttf[e + i * 2], ttf[e + i * 2 + 1]]));
    }
    let s = e + segcount * 2 + 2; // 跳过保留 u16
    let mut startcode = Vec::with_capacity(segcount);
    for i in 0..segcount {
        startcode.push(u16::from_be_bytes([ttf[s + i * 2], ttf[s + i * 2 + 1]]));
    }
    let id = s + segcount * 2;
    let mut iddelta = Vec::with_capacity(segcount);
    for i in 0..segcount {
        iddelta.push(i16::from_be_bytes([ttf[id + i * 2], ttf[id + i * 2 + 1]]));
    }
    let ir = id + segcount * 2;
    let mut idrangeoff = Vec::with_capacity(segcount);
    for i in 0..segcount {
        idrangeoff.push(u16::from_be_bytes([ttf[ir + i * 2], ttf[ir + i * 2 + 1]]));
    }
    for i in 0..segcount {
        let start = startcode[i];
        let end = endcode[i];
        let delta = iddelta[i];
        let ro = idrangeoff[i];
        if start == 0xFFFF && end == 0xFFFF {
            continue;
        }
        let mut c = start;
        loop {
            let glyph: u16 = if ro == 0 {
                ((c as i32 + delta as i32) & 0xFFFF) as u16
            } else {
                let idx = ir + i * 2 + (c - start) as usize * 2 + ro as usize;
                if idx + 2 > ttf.len() {
                    break;
                }
                let g = u16::from_be_bytes([ttf[idx], ttf[idx + 1]]);
                if g == 0 {
                    0
                } else {
                    ((g as i32 + delta as i32) & 0xFFFF) as u16
                }
            };
            if glyph != 0 {
                map.insert(glyph as u32, c as u32);
            }
            if c == 0xFFFF {
                break;
            }
            c = c.wrapping_add(1);
        }
    }
}

fn parse_cmap12(ttf: &[u8], base: usize, map: &mut HashMap<u32, u32>) {
    if base + 12 > ttf.len() {
        return;
    }
    let ngroups = u32::from_be_bytes([ttf[base + 4], ttf[base + 5], ttf[base + 6], ttf[base + 7]]) as usize;
    let mut p = base + 12;
    for _ in 0..ngroups {
        if p + 12 > ttf.len() {
            break;
        }
        let startc = u32::from_be_bytes([ttf[p], ttf[p + 1], ttf[p + 2], ttf[p + 3]]);
        let endc = u32::from_be_bytes([ttf[p + 4], ttf[p + 5], ttf[p + 6], ttf[p + 7]]);
        let startg = u32::from_be_bytes([ttf[p + 8], ttf[p + 9], ttf[p + 10], ttf[p + 11]]);
        let mut c = startc;
        let mut g = startg;
        loop {
            map.insert(g, c);
            if c == endc {
                break;
            }
            c = c.wrapping_add(1);
            g = g.wrapping_add(1);
        }
        p += 12;
    }
}

// ============================================================
// 源 ToUnicode 解析 — CID -> Unicode
// ============================================================

/// CJK  radicals supplement (U+2E80~2EFF) 没有 NFKC 分解, 用硬编码表把常见字根归一到
/// 对应标准汉字, 避免渲染成字根形 (如 ⺠→民, ⻓→长)。
/// 仅覆盖实测遇到的少量字根, 其余字根保持原值(视觉差异极小)。
const RADICAL_SUPPLEMENT_MAP: &[(u32, u32)] = &[
    (0x2EA0, 0x6C11), // ⺠ → 民
    (0x2ED3, 0x957F), // ⻓ → 长
    (0x2E96, 0x624B), // ⺖ → 手
    (0x2E9F, 0x6708), // ⺟ → 月
    (0x2EA1, 0x6C34), // ⺡ → 水
    (0x2EB3, 0x706B), // ⺳ → 火
    (0x2EC8, 0x6728), // ⺨ → 木
    (0x2EE0, 0x91D1), // ⻠ → 金
    (0x2EE3, 0x571F), // ⻣ → 土
    (0x2EAA, 0x65E5), // ⺪ → 日
];

/// 仅对 CJK 兼容区码点 / Kangxi 字根 (U+F900~FAFF, U+2F800~2FA1F, U+2F00~2FDF) 做
/// NFKC 归一, 使其落到标准统一汉字, 从而命中 uni2cid.bin 中已有的标准 CID,
/// 避免渲染成豆腐块/字根形。全角标点等不在这些区间的码点保持不变, 避免提取文本
/// 与原文出现无谓差异。仅当归一化结果恰好是单个字符时才采用, 否则保留原值。
fn normalize_unicode(u: u32) -> u32 {
    let nfkc_range = (0x2F00..=0x2FDF).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0x2F800..=0x2FA1F).contains(&u);
    if nfkc_range {
        if let Some(c) = char::from_u32(u) {
            let s: String = c.nfkc().collect();
            if s.chars().count() == 1 {
                if let Some(nc) = s.chars().next() {
                    return nc as u32;
                }
            }
        }
    }
    if (0x2E80..=0x2EFF).contains(&u) {
        for (k, v) in RADICAL_SUPPLEMENT_MAP {
            if *k == u {
                return *v;
            }
        }
    }
    u
}

fn parse_tounicode_cid_to_unicode(cm: &[u8]) -> HashMap<u32, u32> {
    let mut map: HashMap<u32, u32> = HashMap::new();
    let data = cm;
    let mut i = 0usize;
    const MAX_ENTRIES: u32 = 500_000; // 防止 0xFFFFFFFF 全量范围导致 OOM
    // 最长关键字 (beginbfchar / beginbfrange / endbfchar / endbfrange) 均为 11 字节,
    // 故循环条件必须保证 i+11 不越界, 否则 data[i..i+11] 会 panic。
    while i + 11 <= data.len() {
        if map.len() as u32 >= MAX_ENTRIES {
            break;
        }
        // 查找 "beginbfchar" / "beginbfrange"
        if &data[i..i + 11] == b"beginbfchar" {
            i += 11;
            // 读取到 endbfchar (9 字节); 用 i+11<=len 同时覆盖 endbfrange 的安全性
            while i + 11 <= data.len() && &data[i..i + 9] != b"endbfchar" {
                if map.len() as u32 >= MAX_ENTRIES {
                    break;
                }
                if let (Some(cid), ni) = read_hex(data, i) {
                    if let (Some(uni), ni2) = read_hex(data, ni) {
                        map.insert(cid, normalize_unicode(uni));
                        i = ni2;
                        continue;
                    }
                }
                i += 1;
            }
        } else if &data[i..i + 11] == b"beginbfrange" {
            i += 11;
            while i + 11 <= data.len() && &data[i..i + 11] != b"endbfrange" {
                if map.len() as u32 >= MAX_ENTRIES {
                    break;
                }
                if let (Some(sc), ni) = read_hex(data, i) {
                    if let (Some(ec), ni2) = read_hex(data, ni) {
                        // 下一个要么是 <startunic> 要么是 [ ... ]
                        if let (Some(su), ni3) = read_hex(data, ni2) {
                            let mut c = sc;
                            let mut u = su;
                            let mut steps = 0u32;
                                while c <= ec {
                                map.insert(c, normalize_unicode(u));
                                if c == 0xFFFF_FFFF || steps >= 200_000 {
                                    break;
                                }
                                c = c.wrapping_add(1);
                                u = u.wrapping_add(1);
                                steps += 1;
                            }
                            i = ni3;
                            continue;
                        } else if data[ni2..].starts_with(b"[") {
                            // 逐字映射数组
                            let mut j = ni2 + 1;
                            let mut c = sc;
                            let mut steps = 0u32;
                            while j < data.len() && data[j] != b']' {
                                if steps >= 200_000 {
                                    break;
                                }
                                if let (Some(u), nj) = read_hex(data, j) {
                                    map.insert(c, normalize_unicode(u));
                                    if c == ec {
                                        break;
                                    }
                                    c = c.wrapping_add(1);
                                    j = nj;
                                    steps += 1;
                                    continue;
                                }
                                j += 1;
                            }
                            i = j + 1;
                            continue;
                        }
                    }
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    map
}

/// 从 data[start..] 读取下一个 <hex> 十六进制串, 返回 (value, 下一个绝对索引)
///
/// 注意: 返回的是绝对索引 (相对于 data 起始), 调用方必须直接赋值回游标 i,
/// 不能再叠加子切片偏移, 否则会回到文件头部反复解析同一区域。
fn read_hex(data: &[u8], start: usize) -> (Option<u32>, usize) {
    let mut i = start;
    while i < data.len() && data[i] != b'<' {
        i += 1;
    }
    if i >= data.len() {
        return (None, data.len());
    }
    i += 1; // 跳过 <
    let s = i;
    while i < data.len() && data[i] != b'>' {
        i += 1;
    }
    if i >= data.len() {
        return (None, data.len());
    }
    let hex = &data[s..i];
    i += 1; // 跳过 >
    let v = std::str::from_utf8(hex)
        .ok()
        .and_then(|h| u32::from_str_radix(h, 16).ok());
    match v {
        Some(v) => (Some(v), i),
        None => (None, i),
    }
}

// ============================================================
// ToUnicode CMap 生成 (NotoSansSC CID -> Unicode)
// ============================================================

/// 给 unicode 码点打分: 值越小越"标准"/越优先写入 ToUnicode。
/// 字体 cmap 中同一个 CID 常对应多个码点: 标准统一汉字 / CJK 兼容区 /
/// Kangxi 字根 / CJK 字根补充。文本提取必须映射到最标准的那个, 否则会导出
/// ⼀(Kangxi) 而非 一(统一汉字)。注意 Kangxi 字根 (U+2F00~) 数值小于统一汉字
/// (U+4E00~), 故不能简单按数值最小去重, 必须按"标准度"去重。
fn unicode_priority(u: u32) -> u8 {
    if (0x3400..=0x9FFF).contains(&u) {
        0 // 标准 CJK 统一汉字 (含扩展A): 最优先
    } else if (0xF900..=0xFAFF).contains(&u) {
        1 // CJK 兼容表意文字: 次优先
    } else if (0x2E80..=0x2EFF).contains(&u) {
        2 // CJK 字根补充: 低优先
    } else if (0x2F00..=0x2FDF).contains(&u) {
        3 // Kangxi 字根: 最低优先 (数值虽小, 但非标准)
    } else if (0x2F800..=0x2FA1F).contains(&u) {
        4 // CJK 兼容表意文字补充: 最低优先
    } else {
        0 // 其余 (标点/拉丁/片假名等) 视为标准, 保留
    }
}

fn build_to_unicode_cmap(uni2cid: &HashMap<u32, u32>) -> Vec<u8> {
    let mut cid2uni: Vec<(u32, u32)> = uni2cid.iter().map(|(u, c)| (*c, *u)).collect();
    // 同一 CID 可能同时对应标准码点与 CJK 兼容区/Kangxi 字根码点 (字体 cmap 同时含
    // 两者, 如 U+2F00 与 U+4E00 都指向"一"的同一 CID)。排序时先按 cid, 再按标准度
    // (unicode_priority 越小越标准), 再按 unicode 升序; 然后对每个 cid 保留第一条
    // (=优先级最高=标准统一汉字) 的条目, 使生成的 ToUnicode 文本提取结果与原文一致,
    // 且不影响渲染 (CID 不变, 字形不变)。
    cid2uni.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| unicode_priority(a.1).cmp(&unicode_priority(b.1)))
            .then_with(|| a.1.cmp(&b.1))
    });
    cid2uni.dedup_by_key(|(cid, _)| *cid);

    let mut out = String::new();
    out.push_str("/CIDInit /ProcSet findresource begin\n");
    out.push_str("12 dict begin\n");
    out.push_str("begincmap\n");
    out.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    out.push_str("/CMapName /Adobe-Identity-UCS def\n");
    out.push_str("/CMapType 2 def\n");
    out.push_str("1 begincodespacerange\n");
    out.push_str("<0000> <FFFF>\n");
    out.push_str("endcodespacerange\n");
    for chunk in cid2uni.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (cid, uni) in chunk {
            out.push_str(&format!("<{:04X}> <{:04X}>\n", cid, uni));
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\n");
    out.push_str("CMapName currentdict /CMap defineresource pop\n");
    out.push_str("end end\n");
    out.into_bytes()
}

/// 构建内嵌字体对象 (Type0 / Identity-H + CIDFontType0 / CIDFontType0C + ToUnicode)
/// 字体数据只嵌入一次, 返回的 Type0 对象 ID 可被多个页面/字体引用。
fn build_embedded_font(
    doc: &mut Document,
    font_data: &[u8],
    uni2cid: &HashMap<u32, u32>,
) -> ObjectId {
    // FontFile3 流 (CFF)
    let mut ff_dict = Dictionary::new();
    ff_dict.set(b"Length1".to_vec(), Object::Integer(font_data.len() as i64));
    let fontfile_id = doc.add_object(Object::Stream(Stream::new(ff_dict, font_data.to_vec())));
    if let Ok(Object::Stream(s)) = doc.get_object_mut(fontfile_id) {
        s.dict
            .set(b"Subtype".to_vec(), Object::Name(b"CIDFontType0C".to_vec()));
    }

    // FontDescriptor
    let mut desc = Dictionary::new();
    desc.set(b"Type".to_vec(), Object::Name(b"FontDescriptor".to_vec()));
    desc.set(b"FontName".to_vec(), Object::Name(b"NotoSansSC-Regular".to_vec()));
    desc.set(b"Flags".to_vec(), Object::Integer(6));
    desc.set(
        b"FontBBox".to_vec(),
        Object::Array(vec![
            Object::Integer(-25),
            Object::Integer(-254),
            Object::Integer(1000),
            Object::Integer(880),
        ]),
    );
    desc.set(b"ItalicAngle".to_vec(), Object::Integer(0));
    desc.set(b"Ascent".to_vec(), Object::Integer(752));
    desc.set(b"Descent".to_vec(), Object::Integer(-271));
    desc.set(b"CapHeight".to_vec(), Object::Integer(737));
    desc.set(b"StemV".to_vec(), Object::Integer(58));
    desc.set(b"FontFile3".to_vec(), Object::Reference(fontfile_id));
    let desc_id = doc.add_object(Object::Dictionary(desc));

    // CIDFontType0
    let mut cidfont = Dictionary::new();
    cidfont.set(b"Type".to_vec(), Object::Name(b"Font".to_vec()));
    cidfont.set(b"Subtype".to_vec(), Object::Name(b"CIDFontType0".to_vec()));
    cidfont.set(b"BaseFont".to_vec(), Object::Name(b"NotoSansSC-Regular".to_vec()));
    let mut csi = Dictionary::new();
    csi.set(b"Registry".to_vec(), Object::String(b"(Adobe)".to_vec(), StringFormat::Literal));
    csi.set(b"Ordering".to_vec(), Object::String(b"(Identity)".to_vec(), StringFormat::Literal));
    csi.set(b"Supplement".to_vec(), Object::Integer(0));
    cidfont.set(b"CIDSystemInfo".to_vec(), Object::Dictionary(csi));
    cidfont.set(b"FontDescriptor".to_vec(), Object::Reference(desc_id));
    cidfont.set(b"DW".to_vec(), Object::Integer(1000));
    let cidfont_id = doc.add_object(Object::Dictionary(cidfont));

    // ToUnicode
    let tou_data = build_to_unicode_cmap(uni2cid);
    let tou_id = doc.add_object(Object::Stream(Stream::new(Dictionary::new(), tou_data)));

    // Type0
    let mut type0 = Dictionary::new();
    type0.set(b"Type".to_vec(), Object::Name(b"Font".to_vec()));
    type0.set(b"Subtype".to_vec(), Object::Name(b"Type0".to_vec()));
    type0.set(b"BaseFont".to_vec(), Object::Name(b"NotoSansSC-Regular".to_vec()));
    type0.set(b"Encoding".to_vec(), Object::Name(b"Identity-H".to_vec()));
    type0.set(b"DescendantFonts".to_vec(), Object::Array(vec![Object::Reference(cidfont_id)]));
    type0.set(b"ToUnicode".to_vec(), Object::Reference(tou_id));
    doc.add_object(Object::Dictionary(type0))
}

/// 取页面字体名 -> 字体对象 ID 的映射 (含继承的资源)
fn get_page_font_ids(doc: &Document, page_id: ObjectId) -> Vec<(String, ObjectId)> {
    let mut result = Vec::new();
    if let Ok((resources, inherited)) = doc.get_page_resources(page_id) {
        let mut dicts: Vec<&Dictionary> = Vec::new();
        if let Some(r) = resources {
            dicts.push(r);
        }
        for id in &inherited {
            if let Ok(d) = doc.get_dictionary(*id) {
                dicts.push(d);
            }
        }
        for dict in dicts {
            if let Ok(fonts) = dict.get(b"Font".as_ref()) {
                if let Object::Dictionary(fd) = fonts {
                    for (name, val) in fd.iter() {
                        if let Object::Reference(rid) = val {
                            result.push((String::from_utf8_lossy(name).to_string(), *rid));
                        }
                    }
                }
            }
        }
    }
    result
}

/// WinAnsi (cp1252) 单字节 -> Unicode
fn winansi_byte_to_unicode(b: u8) -> u32 {
    match b {
        0x00..=0x7F => b as u32,
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        0xA0..=0xFF => 0x00A0 + (b as u32 - 0xA0),
        _ => 0xFFFD,
    }
}

/// 该字体是否需要中文替换 (避免对纯拉丁/符号字体无谓嵌入)
fn is_cjk_font(name: &str, code_to_unicode: &HashMap<u32, u32>, enc: CodeEncoding) -> bool {
    let lower = name.to_lowercase();
    if lower.contains("song")
        || lower.contains("hei")
        || lower.contains("kai")
        || lower.contains("fang")
        || lower.contains("ming")
        || lower.contains("noto")
        || lower.contains("stsong")
        || lower.contains("simsun")
        || lower.contains("cjk")
        || lower.contains("gb")
        || lower.contains("sc")
        || lower.contains("tc")
        || lower.contains("jp")
        || lower.contains("kr")
        || lower.contains("chinese")
    {
        return true;
    }
    // 编码层面判断: Unicode 类 CJK CMap
    if matches!(enc, CodeEncoding::Unicode2Byte) {
        return true;
    }
    // 映射中包含 CJK 统一汉字
    code_to_unicode
        .values()
        .any(|&u| (0x3400..=0x9FFF).contains(&u) || (0xF900..=0xFAFF).contains(&u) || (0x20000..=0x2FA1F).contains(&u))
}

/// 把内容流中的一段字符码按编码方式改写为 2 字节 CID (NotoSansSC)
///
/// `code_to_unicode`: 源字符码 -> Unicode (IdentityCid / 源ToUnicode 时非空;
///                    Unicode2Byte / WinAnsi 时为空, 由 enc 直接推出 Unicode)
fn remap_string(
    s: &[u8],
    enc: CodeEncoding,
    code_to_unicode: &HashMap<u32, u32>,
    uni2cid: &HashMap<u32, u32>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2);
    match enc {
        CodeEncoding::Unicode2Byte => {
            let mut i = 0;
            while i + 1 < s.len() {
                let u = ((s[i] as u32) << 8) | (s[i + 1] as u32);
                let cid = uni2cid.get(&u).copied().unwrap_or(0);
                out.push((cid >> 8) as u8);
                out.push((cid & 0xFF) as u8);
                i += 2;
            }
            if i < s.len() {
                let u = (s[i] as u32) << 8;
                let cid = uni2cid.get(&u).copied().unwrap_or(0);
                out.push((cid >> 8) as u8);
                out.push((cid & 0xFF) as u8);
            }
        }
        CodeEncoding::WinAnsi1Byte => {
            for &b in s {
                let u = winansi_byte_to_unicode(b);
                let cid = uni2cid.get(&u).copied().unwrap_or(0);
                out.push((cid >> 8) as u8);
                out.push((cid & 0xFF) as u8);
            }
        }
        CodeEncoding::IdentityCid => {
            let mut i = 0;
            while i + 1 < s.len() {
                let code = ((s[i] as u32) << 8) | (s[i + 1] as u32);
                let u = code_to_unicode.get(&code).copied().unwrap_or(0);
                let cid = uni2cid.get(&u).copied().unwrap_or(0);
                out.push((cid >> 8) as u8);
                out.push((cid & 0xFF) as u8);
                i += 2;
            }
            if i < s.len() {
                let code = (s[i] as u32) << 8;
                let u = code_to_unicode.get(&code).copied().unwrap_or(0);
                let cid = uni2cid.get(&u).copied().unwrap_or(0);
                out.push((cid >> 8) as u8);
                out.push((cid & 0xFF) as u8);
            }
        }
    }
    out
}

/// 重写内容流操作: 跟踪当前字体, 对需要替换的字体改写字符码
fn rewrite_operations(
    ops: Vec<Operation>,
    font_map: &HashMap<String, (CodeEncoding, HashMap<u32, u32>)>,
    uni2cid: &HashMap<u32, u32>,
) -> Vec<Operation> {
    let mut out = Vec::with_capacity(ops.len());
    let mut current_font: Option<String> = None;

    for op in ops {
        if op.operator == "Tf" {
            if let Some(Object::Name(name)) = op.operands.get(0) {
                current_font = Some(String::from_utf8_lossy(name).to_string());
            }
            out.push(op);
            continue;
        }

        let info = current_font.as_ref().and_then(|f| font_map.get(f).cloned());

        match op.operator.as_str() {
            "Tj" => {
                if let Some((enc, c2u)) = info {
                    if let Some(Object::String(s, _)) = op.operands.get(0) {
                        let new_s = remap_string(s, enc, &c2u, uni2cid);
                        let mut new_args = op.operands.clone();
                        new_args[0] = Object::String(new_s, StringFormat::Hexadecimal);
                        out.push(Operation::new(&op.operator, new_args));
                        continue;
                    }
                }
                out.push(op);
            }
            "TJ" => {
                if let Some((enc, c2u)) = info {
                    let mut new_args = op.operands.clone();
                    if let Some(Object::Array(arr)) = new_args.get_mut(0) {
                        for item in arr.iter_mut() {
                            if let Object::String(s, _) = item {
                                *s = remap_string(s, enc, &c2u, uni2cid);
                            }
                        }
                    }
                    out.push(Operation::new(&op.operator, new_args));
                    continue;
                }
                out.push(op);
            }
            "'" => {
                if let Some((enc, c2u)) = info {
                    if let Some(Object::String(s, _)) = op.operands.get(0) {
                        let new_s = remap_string(s, enc, &c2u, uni2cid);
                        let mut new_args = op.operands.clone();
                        new_args[0] = Object::String(new_s, StringFormat::Hexadecimal);
                        out.push(Operation::new(&op.operator, new_args));
                        continue;
                    }
                }
                out.push(op);
            }
            "\"" => {
                if let Some((enc, c2u)) = info {
                    let mut new_args = op.operands.clone();
                    if let Some(Object::String(s, _)) = new_args.last_mut() {
                        *s = remap_string(s, enc, &c2u, uni2cid);
                    }
                    out.push(Operation::new(&op.operator, new_args));
                    continue;
                }
                out.push(op);
            }
            _ => out.push(op),
        }
    }
    out
}

/// 安装一次性 panic hook, 把 wasm 的 abort 改为可读的 console 错误 (报告 file:line)
fn install_panic_hook() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "未知位置".to_string());
        let msg = if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else {
            "非字符串 panic 载荷".to_string()
        };
        web_sys::console::error_1(&format!("[font_embed][PANIC] {} @ {}", msg, loc).into());
    }));
}

/// 核心: 把未嵌入/不可渲染的中文字体替换为内嵌的 NotoSansSC
pub fn embed_cjk_fonts(pdf_bytes: &[u8], font_bytes: &[u8]) -> Result<Vec<u8>, String> {
    install_panic_hook();
    let uni2cid = build_unicode_to_cid();
    let mut doc = Document::load_mem(pdf_bytes).map_err(|e| format!("PDF 解析失败: {:?}", e))?;

    let type0_id = build_embedded_font(&mut doc, font_bytes, &uni2cid);
    let pages: Vec<ObjectId> = doc.page_iter().collect();

    // Pass 1: 分析每个页面字体, 决定哪些需要替换, 并准备 code->unicode 映射
    let mut analysis: Vec<(ObjectId, HashMap<String, (CodeEncoding, HashMap<u32, u32>)>)> = Vec::new();
    let mut to_replace: BTreeSet<ObjectId> = BTreeSet::new();

    for &page_id in &pages {
        let font_entries = get_page_font_ids(&doc, page_id);
        let mut font_map = HashMap::new();
        for (name, fid) in font_entries {
            let fdict = match doc.get_dictionary(fid) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let enc = match detect_encoding(&fdict) {
                Some(e) => e,
                None => {
                    web_sys::console::warn_1(
                        &format!("[font_embed] 跳过非Unicode CID 字体: {}", name).into(),
                    );
                    continue;
                }
            };

            // 推导 code -> unicode
            let mut code_to_unicode: HashMap<u32, u32> = HashMap::new();
            if let CodeEncoding::IdentityCid = enc {
                // 优先: 内嵌 TrueType 的 cmap (GID=Unicode)
                if let Some(emb) = get_embedded_font_bytes(&doc, &fdict) {
                    let g2u = parse_truetype_gid_to_unicode(&emb);
                    if !g2u.is_empty() {
                        code_to_unicode = g2u;
                    }
                }
                // 退而求其次: 源 ToUnicode
                if code_to_unicode.is_empty() {
                    if let Some(tu) = get_font_tounicode(&doc, &fdict) {
                        code_to_unicode = parse_tounicode_cid_to_unicode(&tu);
                    }
                }
            }

            // 是否值得替换 (仅 CJK) — 用字体的真实 BaseFont 名称判断
            // (资源键如 "FT8" 不含语义, 而 BaseFont 如 "ABSEKN+FangSong" 含 "fang"/"song")
            let base_font = fdict
                .get(b"BaseFont".as_ref())
                .ok()
                .and_then(|o| {
                    if let Object::Name(n) = o {
                        Some(String::from_utf8_lossy(n).to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let detect_name = if base_font.is_empty() {
                name.clone()
            } else {
                base_font
            };
            let cjk = is_cjk_font(&detect_name, &code_to_unicode, enc);
            if !cjk {
                continue;
            }
            // IdentityCid 必须能推导出 Unicode, 否则无法安全替换
            if let CodeEncoding::IdentityCid = enc {
                if code_to_unicode.is_empty() {
                    web_sys::console::warn_1(
                        &format!("[font_embed] Identity-H 字体无法推导 Unicode, 跳过: {}", name).into(),
                    );
                    continue;
                }
            }

            font_map.insert(name.clone(), (enc, code_to_unicode));
            to_replace.insert(fid);
        }
        analysis.push((page_id, font_map));
    }

    if to_replace.is_empty() {
        return Ok(pdf_bytes.to_vec());
    }

    // Pass 2: 重写各页内容流 (可变借用 doc)
    for (page_id, font_map) in &analysis {
        for content_id in doc.get_page_contents(*page_id) {
            let content_data = {
                let obj = doc.get_object(content_id).map_err(|e| e.to_string())?;
                let stream = obj.as_stream().map_err(|_| "内容流不是 Stream".to_string())?;
                stream
                    .decompressed_content()
                    .unwrap_or_else(|_| stream.content.clone())
            };
            let content = Content::decode(&content_data)
                .map_err(|e| format!("内容流解码失败: {:?}", e))?;
            let rewritten = rewrite_operations(content.operations, font_map, &uni2cid);
            let encoded = Content {
                operations: rewritten,
            }
            .encode()
            .map_err(|e| e.to_string())?;
            // 关键: 必须用 set_plain_content 去掉原流上的 /Filter (如 FlateDecode),
            // 否则写入的未压缩内容会被当成压缩数据解压, 导致内容流为空/损坏。
            if let Ok(Object::Stream(s)) = doc.get_object_mut(content_id) {
                s.set_plain_content(encoded);
            }
        }
    }

    // Pass 3: 替换字体对象为内嵌 Type0 字体 (共享同一 type0_id 的子对象)
    let type0_template = match doc.get_object(type0_id) {
        Ok(Object::Dictionary(d)) => Some(d.clone()),
        _ => None,
    };
    if let Some(template) = type0_template {
        for &font_id in &to_replace {
            if let Ok(mut obj) = doc.get_object_mut(font_id) {
                *obj = Object::Dictionary(template.clone());
            }
        }
    }

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| format!("PDF 保存失败: {:?}", e))?;
    Ok(out)
}

/// WASM 对外接口: 自动嵌入中文字体
///
/// 从 JS 已注册的字体中取 NotoSansSC (回退 song 别名)。若 PDF 无需处理或出错,
/// 返回原始字节, 不影响正常渲染。
pub fn preprocess_pdf_for_cjk(pdf_bytes: &[u8]) -> Vec<u8> {
    let font_bytes = crate::font_provider::get_registered_font("notosanssc")
        .or_else(|| crate::font_provider::get_registered_font("song"))
        .or_else(|| crate::font_provider::get_registered_font("stsong-light"))
        .or_else(|| crate::font_provider::get_registered_font("default"));

    let font_bytes = match font_bytes {
        Some(f) if !f.is_empty() => f,
        _ => {
            web_sys::console::warn_1(&"[font_embed] 未注册中文字体, 跳过自动嵌入".into());
            return pdf_bytes.to_vec();
        }
    };

    match embed_cjk_fonts(pdf_bytes, &font_bytes) {
        Ok(out) => {
            web_sys::console::log_1(
                &format!(
                    "[font_embed] 自动嵌入中文字体完成: {} -> {} 字节",
                    pdf_bytes.len(),
                    out.len()
                )
                .into(),
            );
            out
        }
        Err(e) => {
            web_sys::console::warn_1(&format!("[font_embed] 嵌入失败, 使用原字节: {}", e).into());
            pdf_bytes.to_vec()
        }
    }
}

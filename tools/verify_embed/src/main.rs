//! 独立验证工具: 用与 font_embed.rs 相同的算法在 native 端嵌入中文字体,
//! 输出 PDF 后用 pypdfium2 验证中文渲染。验证通过后, wasm 版 (同算法) 即可直接用。

use std::collections::{BTreeSet, HashMap};

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use ttf_parser::Face;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CodeEncoding {
    Unicode2Byte,
    WinAnsi1Byte,
}

fn build_unicode_to_cid(font_data: &[u8]) -> Result<HashMap<u32, u32>, String> {
    let face = Face::parse(font_data, 0).map_err(|e| format!("字体解析失败: {:?}", e))?;
    let mut map = HashMap::new();
    for cp in 0x20u32..=0xFFFF {
        if let Some(ch) = char::from_u32(cp) {
            if let Some(gid) = face.glyph_index(ch) {
                map.insert(cp, gid.0 as u32);
            }
        }
    }
    Ok(map)
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

fn is_font_embedded(doc: &Document, dict: &Dictionary) -> bool {
    if font_has_fontfile(dict) {
        return true;
    }
    if let Ok(df) = dict.get(b"DescendantFonts".as_ref()) {
        if let Object::Array(arr) = df {
            for d in arr {
                if let Object::Reference(rid) = d {
                    if let Ok(cdf) = doc.get_dictionary(*rid) {
                        if font_has_fontfile(cdf) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn detect_encoding(font_dict: &Dictionary) -> Option<CodeEncoding> {
    let subtype = font_dict.get(b"Subtype".as_ref()).ok();
    let is_type0 = matches!(subtype, Some(Object::Name(s)) if s.as_slice() == b"Type0");

    let encoding = font_dict.get(b"Encoding".as_ref()).ok();
    let enc_lower = match encoding {
        Some(Object::Name(n)) => String::from_utf8_lossy(n).to_lowercase(),
        _ => String::new(),
    };

    if is_type0 {
        if enc_lower.contains("ucs2")
            || enc_lower.contains("utf16")
            || enc_lower.contains("ucs")
            || enc_lower.contains("identity")
        {
            return Some(CodeEncoding::Unicode2Byte);
        }
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
        Some(CodeEncoding::Unicode2Byte)
    } else {
        Some(CodeEncoding::WinAnsi1Byte)
    }
}

fn build_to_unicode_cmap(uni2cid: &HashMap<u32, u32>) -> Vec<u8> {
    let mut cid2uni: Vec<(u32, u32)> = uni2cid.iter().map(|(u, c)| (*c, *u)).collect();
    cid2uni.sort();
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

fn build_embedded_font(
    doc: &mut Document,
    font_data: &[u8],
    uni2cid: &HashMap<u32, u32>,
) -> ObjectId {
    let mut ff_dict = Dictionary::new();
    ff_dict.set(b"Length1".to_vec(), Object::Integer(font_data.len() as i64));
    let fontfile_id = doc.add_object(Object::Stream(Stream::new(ff_dict, font_data.to_vec())));
    if let Ok(Object::Stream(s)) = doc.get_object_mut(fontfile_id) {
        s.dict.set(b"Subtype".to_vec(), Object::Name(b"CIDFontType0C".to_vec()));
    }

    let mut desc = Dictionary::new();
    desc.set(b"Type".to_vec(), Object::Name(b"FontDescriptor".to_vec()));
    desc.set(b"FontName".to_vec(), Object::Name(b"NotoSansSC-Regular".to_vec()));
    desc.set(b"Flags".to_vec(), Object::Integer(6));
    desc.set(b"FontBBox".to_vec(), Object::Array(vec![
        Object::Integer(-25), Object::Integer(-254), Object::Integer(1000), Object::Integer(880),
    ]));
    desc.set(b"ItalicAngle".to_vec(), Object::Integer(0));
    desc.set(b"Ascent".to_vec(), Object::Integer(752));
    desc.set(b"Descent".to_vec(), Object::Integer(-271));
    desc.set(b"CapHeight".to_vec(), Object::Integer(737));
    desc.set(b"StemV".to_vec(), Object::Integer(58));
    desc.set(b"FontFile3".to_vec(), Object::Reference(fontfile_id));
    let desc_id = doc.add_object(Object::Dictionary(desc));

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

    let tou_data = build_to_unicode_cmap(uni2cid);
    let tou_id = doc.add_object(Object::Stream(Stream::new(Dictionary::new(), tou_data)));

    let mut type0 = Dictionary::new();
    type0.set(b"Type".to_vec(), Object::Name(b"Font".to_vec()));
    type0.set(b"Subtype".to_vec(), Object::Name(b"Type0".to_vec()));
    type0.set(b"BaseFont".to_vec(), Object::Name(b"NotoSansSC-Regular".to_vec()));
    type0.set(b"Encoding".to_vec(), Object::Name(b"Identity-H".to_vec()));
    type0.set(b"DescendantFonts".to_vec(), Object::Array(vec![Object::Reference(cidfont_id)]));
    type0.set(b"ToUnicode".to_vec(), Object::Reference(tou_id));
    doc.add_object(Object::Dictionary(type0))
}

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

fn winansi_byte_to_unicode(b: u8) -> u32 {
    match b {
        0x00..=0x7F => b as u32,
        0x80 => 0x20AC, 0x82 => 0x201A, 0x83 => 0x0192, 0x84 => 0x201E, 0x85 => 0x2026,
        0x86 => 0x2020, 0x87 => 0x2021, 0x88 => 0x02C6, 0x89 => 0x2030, 0x8A => 0x0160,
        0x8B => 0x2039, 0x8C => 0x0152, 0x8E => 0x017D, 0x91 => 0x2018, 0x92 => 0x2019,
        0x93 => 0x201C, 0x94 => 0x201D, 0x95 => 0x2022, 0x96 => 0x2013, 0x97 => 0x2014,
        0x98 => 0x02DC, 0x99 => 0x2122, 0x9A => 0x0161, 0x9B => 0x203A, 0x9C => 0x0153,
        0x9E => 0x017E, 0x9F => 0x0178, 0xA0..=0xFF => 0x00A0 + (b as u32 - 0xA0),
        _ => 0xFFFD,
    }
}

fn remap_string(s: &[u8], enc: CodeEncoding, uni2cid: &HashMap<u32, u32>) -> Vec<u8> {
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
    }
    out
}

fn rewrite_operations(
    ops: Vec<Operation>,
    font_map: &HashMap<String, CodeEncoding>,
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
        let enc = current_font.as_ref().and_then(|f| font_map.get(f).copied());
        match op.operator.as_str() {
            "Tj" => {
                if let Some(enc) = enc {
                    if let Some(Object::String(s, _)) = op.operands.get(0) {
                        let new_s = remap_string(s, enc, uni2cid);
                        let mut new_args = op.operands.clone();
                        new_args[0] = Object::String(new_s, StringFormat::Hexadecimal);
                        out.push(Operation::new(&op.operator, new_args));
                        continue;
                    }
                }
                out.push(op);
            }
            "TJ" => {
                if let Some(enc) = enc {
                    let mut new_args = op.operands.clone();
                    if let Some(Object::Array(arr)) = new_args.get_mut(0) {
                        for item in arr.iter_mut() {
                            if let Object::String(s, _) = item {
                                *s = remap_string(s, enc, uni2cid);
                            }
                        }
                    }
                    out.push(Operation::new(&op.operator, new_args));
                    continue;
                }
                out.push(op);
            }
            "'" => {
                if let Some(enc) = enc {
                    if let Some(Object::String(s, _)) = op.operands.get(0) {
                        let new_s = remap_string(s, enc, uni2cid);
                        let mut new_args = op.operands.clone();
                        new_args[0] = Object::String(new_s, StringFormat::Hexadecimal);
                        out.push(Operation::new(&op.operator, new_args));
                        continue;
                    }
                }
                out.push(op);
            }
            "\"" => {
                if let Some(enc) = enc {
                    let mut new_args = op.operands.clone();
                    if let Some(Object::String(s, _)) = new_args.last_mut() {
                        *s = remap_string(s, enc, uni2cid);
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

fn embed_cjk_fonts(pdf_bytes: &[u8], font_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let uni2cid = build_unicode_to_cid(font_bytes)?;
    let mut doc = Document::load_mem(pdf_bytes).map_err(|e| format!("PDF 解析失败: {:?}", e))?;
    let type0_id = build_embedded_font(&mut doc, font_bytes, &uni2cid);
    let pages: Vec<ObjectId> = doc.page_iter().collect();

    let mut analysis: Vec<(ObjectId, HashMap<String, CodeEncoding>)> = Vec::new();
    let mut to_replace: BTreeSet<ObjectId> = BTreeSet::new();

    for &page_id in &pages {
        let font_entries = get_page_font_ids(&doc, page_id);
        let mut font_map = HashMap::new();
        for (name, fid) in font_entries {
            if let Ok(fdict) = doc.get_dictionary(fid) {
                if !is_font_embedded(&doc, fdict) {
                    if let Some(enc) = detect_encoding(fdict) {
                        font_map.insert(name.clone(), enc);
                        to_replace.insert(fid);
                    } else {
                        eprintln!("[warn] 跳过无法处理的字体: {}", name);
                    }
                }
            }
        }
        analysis.push((page_id, font_map));
    }

    if to_replace.is_empty() {
        return Ok(pdf_bytes.to_vec());
    }

    for (page_id, font_map) in &analysis {
        for content_id in doc.get_page_contents(*page_id) {
            let content_data = {
                let obj = doc.get_object(content_id).map_err(|e| e.to_string())?;
                let stream = obj.as_stream().map_err(|_| "内容流不是 Stream".to_string())?;
                stream.decompressed_content().unwrap_or_else(|_| stream.content.clone())
            };
            let content = Content::decode(&content_data)
                .map_err(|e| format!("内容流解码失败: {:?}", e))?;
            let rewritten = rewrite_operations(content.operations, font_map, &uni2cid);
            let encoded = Content { operations: rewritten }.encode().map_err(|e| e.to_string())?;
            if let Ok(Object::Stream(s)) = doc.get_object_mut(content_id) {
                s.set_content(encoded);
            }
        }
    }

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
    doc.save_to(&mut out).map_err(|e| format!("PDF 保存失败: {:?}", e))?;
    Ok(out)
}

fn main() {
    // 1) 验证字体映射假设
    let font_bytes = std::fs::read(r"D:/workspace/self/rust-wasm-seal/fonts/NotoSansSC-Regular.otf")
        .expect("读取字体失败");
    let face = Face::parse(&font_bytes, 0).expect("字体解析失败");
    println!("=== 字体映射验证 (ttf-parser glyph_index) ===");
    for (label, cp) in [("中", 0x4E2D), ("文", 0x6587), ("合", 0x5408), ("同", 0x540C)] {
        let ch = char::from_u32(cp).unwrap();
        let gid = face.glyph_index(ch).map(|g| g.0).unwrap_or(0);
        println!("  {} (U+{:04X}) -> GID/CID = {}", label, cp, gid);
    }

    // 2) 端到端嵌入
    let pdf_bytes = std::fs::read(r"D:/工作文档/sample.pdf").expect("读取 PDF 失败");
    println!("\n=== 嵌入中文字体 ===");
    println!("  原始 PDF: {} 字节", pdf_bytes.len());
    match embed_cjk_fonts(&pdf_bytes, &font_bytes) {
        Ok(out) => {
            let out_path = r"D:/workspace/self/rust-wasm-seal/sample.rust_embedded.pdf";
            std::fs::write(out_path, &out).expect("写入失败");
            println!("  嵌入后 PDF: {} 字节 -> {}", out.len(), out_path);
        }
        Err(e) => {
            eprintln!("  嵌入失败: {}", e);
            std::process::exit(1);
        }
    }
}

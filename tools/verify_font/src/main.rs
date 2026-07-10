use ttf_parser::Face;
fn main() {
    let font_bytes = std::fs::read(r"D:/workspace/self/rust-wasm-seal/fonts/NotoSansSC-Regular.otf").expect("read font");
    let face = Face::parse(&font_bytes, 0).expect("parse");
    println!("=== ttf-parser glyph_index (应为 CID, 因 NotoSansSC CID==GID) ===");
    for (label, cp) in [("中",0x4E2D),("文",0x6587),("合",0x5408),("同",0x540C),("A",0x0041),("1",0x0031)] {
        let ch = char::from_u32(cp).unwrap();
        let gid = face.glyph_index(ch).map(|g| g.0).unwrap_or(0);
        println!("  {} U+{:04X} -> GID/CID = {}", label, cp, gid);
    }
    // 统计映射条目
    let mut n = 0u32;
    for cp in 0x20u32..=0xFFFF {
        if let Some(ch) = char::from_u32(cp) {
            if face.glyph_index(ch).is_some() { n += 1; }
        }
    }
    println!("  unicode->cid 映射条目数: {}", n);
}

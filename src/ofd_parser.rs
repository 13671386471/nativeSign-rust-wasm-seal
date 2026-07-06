//! OFD 文档解析器
//!
//! 解析 OFD (Open Fixed-layout Document) ZIP 文件，提取页面内容对象。
//! 支持: 文本(TextObject)、路径(PathObject)、图片(ImageObject)。
//!
//! 解析流程:
//!   1. 打开 ZIP → 读取 OFD.xml 找到 DocRoot
//!   2. 读取 Document.xml → 获取 Page 列表和公共资源路径
//!   3. 逐页读取 Page_N/Content.xml → 提取对象树
//!   4. 可选: 读取公共资源中的字体信息

use std::collections::HashMap;
use std::io::{Cursor, Read};
use quick_xml::events::Event;
use quick_xml::Reader;

// ============================================================
// 数据模型
// ============================================================

#[derive(Debug, Clone)]
pub struct OfdDocument {
    pub doc_info: OfdDocInfo,
    pub pages: Vec<OfdPage>,
    pub fonts: HashMap<String, OfdFont>,
    pub public_images: HashMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Default)]
pub struct OfdDocInfo {
    pub title: String,
    pub author: String,
    pub creator: String,
}

#[derive(Debug, Clone)]
pub struct OfdPage {
    pub index: u32,
    /// 物理区域 (单位: mm), 默认 A4: (0, 0, 210, 297)
    pub physical_box: OfdRect,
    /// 页面中的渲染对象 (Z 序排列)
    pub objects: Vec<OfdObject>,
}

#[derive(Debug, Clone, Copy)]
pub struct OfdRect {
    pub x: f64, pub y: f64, pub w: f64, pub h: f64,
}

impl OfdRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self { Self { x, y, w, h } }
}

// ---- 对象类型 ----

#[derive(Debug, Clone)]
pub enum OfdObject {
    Text(OfdTextObject),
    Image(OfdImageObject),
    Path(OfdPathObject),
}

#[derive(Debug, Clone)]
pub struct OfdTextObject {
    pub boundary: Option<OfdRect>,
    pub ctm: [f64; 6],       // [a, b, c, d, e, f]
    pub font_family: String,
    pub font_size: f64,        // 单位: mm
    pub fill_color: OfdColor,
    pub stroke_color: Option<OfdColor>,
    pub text_items: Vec<OfdTextItem>,
}

#[derive(Debug, Clone)]
pub struct OfdTextItem {
    pub x: f64,   // mm
    pub y: f64,   // mm
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct OfdImageObject {
    pub ctm: [f64; 6],
    pub boundary: Option<OfdRect>,
    pub image_data: Vec<u8>,
    pub img_width: f64,   // pixel
    pub img_height: f64,  // pixel
}

#[derive(Debug, Clone)]
pub struct OfdPathObject {
    pub ctm: [f64; 6],
    pub boundary: Option<OfdRect>,
    pub stroke_color: Option<OfdColor>,
    pub fill_color: Option<OfdColor>,
    pub line_width: f64,     // mm
    /// SVG 风格缩略路径数据, 如 "M 10 10 L 100 100"
    pub path_data: String,
}

#[derive(Debug, Clone, Copy)]
pub struct OfdColor {
    pub r: u8, pub g: u8, pub b: u8, pub a: u8,
}

impl OfdColor {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    #[allow(dead_code)]
    pub const RED:   Self = Self { r: 255, g: 0, b: 0, a: 255 };

    pub fn to_css(&self) -> String {
        if self.a == 255 {
            format!("rgb({},{},{})", self.r, self.g, self.b)
        } else {
            format!("rgba({},{},{},{})", self.r, self.g, self.b, self.a as f64 / 255.0)
        }
    }
}

#[derive(Debug, Clone)]
pub struct OfdFont {
    pub id: String,
    pub family: String,
}

// ============================================================
// 主解析入口
// ============================================================

pub fn parse_ofd(data: &[u8]) -> Result<OfdDocument, String> {
    let cursor = Cursor::new(data.to_vec());
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("ZIP 解压失败: {}", e))?;

    // 1) OFD.xml → DocRoot 路径
    let doc_root = find_doc_root(&mut archive)?;
    let base_dir = parent_dir(&doc_root);

    // 2) Document.xml → Page 列表 + CommonData 路径
    let (page_refs, common_data_path) = read_document_manifest(&mut archive, &doc_root)?;

    // 3) 解析公共资源 (字体 + 图片)
    let mut fonts: HashMap<String, OfdFont> = HashMap::new();
    let mut public_images: HashMap<String, Vec<u8>> = HashMap::new();
    if let Some(ref cp) = common_data_path {
        parse_common_resources(&mut archive, cp, &mut fonts, &mut public_images)?;
    }
    // 也尝试默认 DocumentRes.xml
    let doc_res = format!("{}DocumentRes.xml", base_dir);
    if common_data_path.as_deref() != Some(&doc_res) {
        let _ = parse_common_resources(&mut archive, &doc_res, &mut fonts, &mut public_images);
    }

    // 4) 逐页解析 Content.xml
    let mut pages = Vec::new();
    for (idx, page_ref) in page_refs.iter().enumerate() {
        let page_file = resolve_page_path(&base_dir, page_ref);
        let page = parse_page_content(&mut archive, &page_file, idx as u32)?;
        pages.push(page);
    }

    Ok(OfdDocument {
        doc_info: OfdDocInfo::default(),
        pages,
        fonts,
        public_images,
    })
}

// ============================================================
// 步骤 1: OFD.xml
// ============================================================

fn find_doc_root(archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>) -> Result<String, String> {
    let xml = read_zip_str(archive, "OFD.xml")?;
    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "DocBody" {
                    if let Some(v) = attr_val(e, &r, "DocRoot") {
                        return Ok(v);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("OFD.xml 解析错误: {}", e)),
            _ => {}
        }
        buf.clear();
    }
    // 默认值
    Ok("Doc_0/Document.xml".into())
}

// ============================================================
// 步骤 2: Document.xml → 页面清单
// ============================================================

fn read_document_manifest(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    doc_root: &str,
) -> Result<(Vec<String>, Option<String>), String> {
    let xml = read_zip_str(archive, doc_root)?;
    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut pages = Vec::new();
    let mut common_data: Option<String> = None;
    let mut in_pages = false;
    let mut in_common = false;

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Pages" => in_pages = true,
                    "CommonData" => in_common = true,
                    "Page" if in_pages => {
                        if let Some(loc) = attr_val(e, &r, "BaseLoc") {
                            pages.push(loc);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "Page" && in_pages {
                    if let Some(loc) = attr_val(e, &r, "BaseLoc") {
                        pages.push(loc);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Pages" => in_pages = false,
                    "CommonData" => in_common = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    // CommonData: 优先取 Document.xml 中声明的 MaxOccur="1" 的 CommonData
    // 实际常位于 Doc_N/DocumentRes.xml
    let base = parent_dir(doc_root);
    let default_cd = format!("{}DocumentRes.xml", base);
    common_data = Some(common_data.unwrap_or(default_cd));

    Ok((pages, common_data))
}

// ============================================================
// 步骤 3: 公共资源
// ============================================================

fn parse_common_resources(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    path: &str,
    fonts: &mut HashMap<String, OfdFont>,
    images: &mut HashMap<String, Vec<u8>>,
) -> Result<(), String> {
    let xml = match read_zip_str(archive, path) {
        Ok(x) => x,
        Err(_) => return Ok(()), // 文件不存在,跳过
    };
    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Font" => {
                        let id = attr_val(e, &r, "ID").unwrap_or_default();
                        let family = attr_val(e, &r, "FamilyName")
                            .or_else(|| attr_val(e, &r, "FontName"))
                            .unwrap_or_else(|| "sans-serif".into());
                        fonts.insert(id, OfdFont { id: String::new(), family });
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

// ============================================================
// 步骤 4: 单页 Content.xml 解析
// ============================================================

fn parse_page_content(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    page_path: &str,
    page_idx: u32,
) -> Result<OfdPage, String> {
    let xml = match read_zip_str(archive, page_path) {
        Ok(x) => x,
        Err(_) => {
            return Ok(OfdPage {
                index: page_idx,
                physical_box: OfdRect::new(0.0, 0.0, 210.0, 297.0),
                objects: Vec::new(),
            });
        }
    };

    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut physical_box = OfdRect::new(0.0, 0.0, 210.0, 297.0);
    let mut objects: Vec<OfdObject> = Vec::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Area" | "PageArea" => {
                        if let Some(pb) = attr_val(e, &r, "PhysicalBox") {
                            physical_box = parse_rect(&pb);
                        }
                    }
                    "TextObject" => {
                        let mut sub_buf = Vec::new();
                        let obj = parse_text_object(&mut r, &mut sub_buf, e)?;
                        objects.push(OfdObject::Text(obj));
                    }
                    "PathObject" => {
                        let mut sub_buf = Vec::new();
                        let obj = parse_path_object(&mut r, &mut sub_buf, e)?;
                        objects.push(OfdObject::Path(obj));
                    }
                    "ImageObject" => {
                        let mut sub_buf = Vec::new();
                        let obj = parse_image_object(&mut r, &mut sub_buf, e)?;
                        objects.push(OfdObject::Image(obj));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "Area" || tag == "PageArea" {
                    if let Some(pb) = attr_val(e, &r, "PhysicalBox") {
                        physical_box = parse_rect(&pb);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(OfdPage { index: page_idx, physical_box, objects })
}

// ============================================================
// 对象子解析器
// ============================================================

fn parse_text_object(
    r: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    start: &quick_xml::events::BytesStart,
) -> Result<OfdTextObject, String> {
    let boundary = attr_val(start, r, "Boundary").map(|s| parse_rect(&s));
    let ctm = attr_val(start, r, "CTM")
        .map(|s| parse_ctm(&s))
        .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    let mut font_family = String::from("sans-serif");
    let mut font_size = 4.23; // 默认 12pt → mm
    let mut fill_color = OfdColor::BLACK;
    let mut stroke_color: Option<OfdColor> = None;
    let mut text_items: Vec<OfdTextItem> = Vec::new();

    let mut in_text_code = false;
    let mut current_x = 0.0f64;
    let mut current_y = 0.0f64;
    let mut delta_x_arr: Vec<f64> = Vec::new();
    let mut delta_y_arr: Vec<f64> = Vec::new();
    let mut delta_x_idx = 0usize;
    let mut delta_y_idx = 0usize;

    loop {
        match r.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "TextCode" => {
                        in_text_code = true;
                        current_x = attr_val(e, r, "X")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(current_x);
                        current_y = attr_val(e, r, "Y")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(current_y);
                        if let Some(dx) = attr_val(e, r, "DeltaX") {
                            delta_x_arr = parse_num_array(&dx);
                            delta_x_idx = 0;
                        }
                        if let Some(dy) = attr_val(e, r, "DeltaY") {
                            delta_y_arr = parse_num_array(&dy);
                            delta_y_idx = 0;
                        }
                    }
                    "Font" => {
                        font_size = attr_val(e, r, "Size")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(font_size);
                    }
                    "FillColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            fill_color = parse_color(&v);
                        }
                    }
                    "StrokeColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            stroke_color = Some(parse_color(&v));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Font" => {
                        font_size = attr_val(e, r, "Size")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(font_size);
                    }
                    "FillColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            fill_color = parse_color(&v);
                        }
                    }
                    "StrokeColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            stroke_color = Some(parse_color(&v));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_text_code {
                    let raw = e.unescape().unwrap_or_default();
                    let text = raw.to_string();
                    if !text.trim().is_empty() {
                        let mut x = current_x;
                        let _y = current_y;
                        let char_count = text.chars().count();

                        // 逐字偏移 (DeltaX 数组)
                        for gi in 0..char_count {
                            if gi > 0 {
                                x += delta_x_arr.get(gi - 1).copied().unwrap_or(0.0);
                                // y += delta_y_arr.get(gi - 1).copied().unwrap_or(0.0);
                            }
                        }

                        text_items.push(OfdTextItem { x, y: current_y, text });
                        current_x += char_count as f64
                            * delta_x_arr.last().copied().unwrap_or(0.0);
                        current_y += char_count as f64
                            * delta_y_arr.last().copied().unwrap_or(0.0);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "TextCode" {
                    in_text_code = false;
                } else if tag == "TextObject" {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(OfdTextObject {
        boundary,
        ctm,
        font_family,
        font_size,
        fill_color,
        stroke_color,
        text_items,
    })
}

fn parse_path_object(
    r: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    start: &quick_xml::events::BytesStart,
) -> Result<OfdPathObject, String> {
    let boundary = attr_val(start, r, "Boundary").map(|s| parse_rect(&s));
    let ctm = attr_val(start, r, "CTM")
        .map(|s| parse_ctm(&s))
        .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    let mut stroke_color: Option<OfdColor> = Some(OfdColor::BLACK);
    let mut fill_color: Option<OfdColor> = None;
    let line_width = 0.353; // ~0.5pt → mm
    let mut path_data = String::new();

    loop {
        match r.read_event_into(buf) {
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "StrokeColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            stroke_color = Some(parse_color(&v));
                        }
                    }
                    "FillColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            fill_color = Some(parse_color(&v));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "StrokeColor" {
                    if let Some(v) = attr_val(e, r, "Value") {
                        stroke_color = Some(parse_color(&v));
                    }
                } else if tag == "FillColor" {
                    if let Some(v) = attr_val(e, r, "Value") {
                        fill_color = Some(parse_color(&v));
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default();
                path_data.push_str(&text);
            }
            Ok(Event::End(ref e)) => {
                if local_name(e.name().as_ref()) == "PathObject" {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(OfdPathObject {
        ctm,
        boundary,
        stroke_color,
        fill_color,
        line_width,
        path_data,
    })
}

fn parse_image_object(
    r: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    start: &quick_xml::events::BytesStart,
) -> Result<OfdImageObject, String> {
    let boundary = attr_val(start, r, "Boundary").map(|s| parse_rect(&s));
    let ctm = attr_val(start, r, "CTM")
        .map(|s| parse_ctm(&s))
        .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    let mut image_data: Vec<u8> = Vec::new();
    let img_width = 0.0f64;
    let img_height = 0.0f64;

    loop {
        match r.read_event_into(buf) {
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "MediaFile" {
                    // 内嵌 base64 图片数据
                    if let Some(data) = attr_val(e, r, "Data") {
                        image_data = base64_decode(&data);
                    }
                }
            }
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "MediaFile" {
                    if let Some(data) = attr_val(e, r, "Data") {
                        image_data = base64_decode(&data);
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                if local_name(e.name().as_ref()) == "ImageObject" {
                    break;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(OfdImageObject { ctm, boundary, image_data, img_width, img_height })
}

// ============================================================
// 工具函数
// ============================================================

fn read_zip_str(archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>, path: &str) -> Result<String, String> {
    let mut file = archive
        .by_name(path)
        .map_err(|e| format!("条目 {} 不存在: {}", path, e))?;
    let mut s = String::new();
    file.read_to_string(&mut s)
        .map_err(|e| format!("读取 {} 失败: {}", path, e))?;
    Ok(s)
}

#[allow(dead_code)]
fn read_zip_bytes(archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>, path: &str) -> Result<Vec<u8>, String> {
    let mut file = archive.by_name(path)
        .map_err(|e| format!("条目 {} 不存在: {}", path, e))?;
    let mut v = Vec::new();
    file.read_to_end(&mut v)
        .map_err(|e| format!("读取 {} 失败: {}", path, e))?;
    Ok(v)
}

/// 获取 XML 元素的本地名称 (去掉命名空间前缀)
fn local_name(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

/// 从元素提取属性值
fn attr_val(e: &quick_xml::events::BytesStart, reader: &Reader<&[u8]>, name: &str) -> Option<String> {
    let decoder = reader.decoder();
    for a in e.attributes().flatten() {
        if local_name(a.key.as_ref()) == name {
            return a.decode_and_unescape_value(decoder).ok().map(|c| c.to_string());
        }
    }
    None
}

fn parse_rect(s: &str) -> OfdRect {
    let v: Vec<f64> = s.split_whitespace().filter_map(|p| p.parse().ok()).collect();
    if v.len() >= 4 { OfdRect::new(v[0], v[1], v[2], v[3]) }
    else { OfdRect::new(0.0, 0.0, 210.0, 297.0) }
}

fn parse_ctm(s: &str) -> [f64; 6] {
    let v: Vec<f64> = s.split_whitespace().filter_map(|p| p.parse().ok()).collect();
    if v.len() >= 6 { [v[0], v[1], v[2], v[3], v[4], v[5]] }
    else { [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] }
}

fn parse_color(s: &str) -> OfdColor {
    let v: Vec<u8> = s.split_whitespace()
        .filter_map(|p| p.parse::<f64>().ok().map(|n| (n.clamp(0.0, 255.0)) as u8))
        .collect();
    match v.len() {
        1 => OfdColor { r: v[0], g: v[0], b: v[0], a: 255 },
        3 => OfdColor { r: v[0], g: v[1], b: v[2], a: 255 },
        4 => OfdColor { r: v[0], g: v[1], b: v[2], a: v[3] },
        _ => OfdColor::BLACK,
    }
}

/// 解析空白分隔数字数组
fn parse_num_array(s: &str) -> Vec<f64> {
    s.split_whitespace().filter_map(|p| p.parse().ok()).collect()
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(pos) => format!("{}/", &path[..=pos]),
        None => String::new(),
    }
}

fn resolve_page_path(base_dir: &str, page_ref: &str) -> String {
    // page_ref 格式通常是 "Pages/Page_0/Content.xml"
    if page_ref.starts_with('/') {
        page_ref[1..].to_string()
    } else {
        format!("{}{}", base_dir, page_ref)
    }
}

fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .unwrap_or_default()
}

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
    /// 页面注释 (如印章/水印), 渲染在普通对象之上
    pub annotations: Vec<OfdAnnotation>,
}

#[derive(Debug, Clone)]
pub struct OfdAnnotation {
    pub annot_type: String,   // "Watermark", "Stamp" 等
    pub boundary: OfdRect,    // Annotation 在页面上的位置
    pub objects: Vec<OfdObject>, // Appearance 内的对象 (Text/Image/Path)
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
    pub resource_id: String,   // ResourceID 引用公共资源中的图片
    pub image_data: Vec<u8>,   // 实际图片数据（内嵌 base64 或从公共资源加载）
    pub img_width: f64,        // pixel
    pub img_height: f64,       // pixel
}

// ============================================================
// 字体名称映射 — OFD 字体名 → CSS 字体栈
// ============================================================

pub fn map_font_family(ofd_font: &str) -> String {
    let mapped = match ofd_font {
        "SimSun" | "宋体" | "simsun" => r#""SimSun", "Songti SC", "Noto Serif CJK SC", serif"#,
        "SimHei" | "黑体" | "simhei" => r#""SimHei", "Heiti SC", "Noto Sans CJK SC", sans-serif"#,
        "KaiTi" | "楷体" | "kaiti" => r#""KaiTi", "Kaiti SC", "STKaiti", serif"#,
        "FangSong" | "仿宋" | "fangsong" => r#""FangSong", "Fangsong SC", "STFangsong", serif"#,
        "SimLi" | "隶书" => r#""SimLi", "LiSu", "STLiti", serif"#,
        "SimYou" | "幼圆" => r#""SimYou", "YouYuan", cursive"#,
        "Microsoft YaHei" | "微软雅黑" => r#""Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif"#,
        "Arial" => r#""Arial", sans-serif"#,
        "Times New Roman" => r#""Times New Roman", serif"#,
        "Courier New" => r#""Courier New", monospace"#,
        _ => ofd_font,
    };
    mapped.to_string()
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

    // 2) Document.xml → Page 列表 + CommonData 路径 + Annotations 路径
    let (page_refs, common_data_path, annotations_path) = read_document_manifest(&mut archive, &doc_root)?;

    web_sys::console::log_1(&format!("[parse_ofd] doc_root={}, pages_found={}", doc_root, page_refs.len()).into());

    // 3) 解析公共资源 (字体 + 图片)
    let mut fonts: HashMap<String, OfdFont> = HashMap::new();
    let mut public_images: HashMap<String, Vec<u8>> = HashMap::new();
    if let Some(ref cp) = common_data_path {
        parse_common_resources(&mut archive, cp, &mut fonts, &mut public_images)?;
    }
    // 也尝试默认 DocumentRes.xml
    let doc_res = if base_dir.is_empty() {
        "DocumentRes.xml".to_string()
    } else {
        format!("{}/DocumentRes.xml", base_dir)
    };
    if common_data_path.as_deref() != Some(&doc_res) {
        let _ = parse_common_resources(&mut archive, &doc_res, &mut fonts, &mut public_images);
    }

    // 4) 解析 Annotations.xml (页面注释索引)
    let page_annotations_map = if let Some(ref ap) = annotations_path {
        let resolved = resolve_page_path(&base_dir, ap);
        parse_annotations_index(&mut archive, &resolved).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    // 建立 PageID → 数组索引的映射
    let page_id_to_index: HashMap<u32, usize> = page_refs
        .iter()
        .enumerate()
        .map(|(idx, (page_id, _))| (*page_id, idx))
        .collect();

    // 5) 逐页解析 Content.xml
    let mut pages = Vec::new();
    for (idx, (page_id, page_ref)) in page_refs.iter().enumerate() {
        let page_file = resolve_page_path(&base_dir, page_ref);
        // Annotations.xml 中的 PageID 对应 Document.xml 中 Page 的 ID
        // 把 Annotation 文件路径解析为相对于 OFD 根目录的完整路径
        let page_annot = page_annotations_map.get(page_id)
            .map(|ap| resolve_page_path(&base_dir, ap));
        web_sys::console::log_1(&format!("[parse_ofd] page[{}] id={} annotation_path={:?}", idx, page_id, page_annot).into());
        let page = parse_page_content(&mut archive, &page_file, idx as u32, &fonts, page_annot.as_deref())?;
        web_sys::console::log_1(&format!("[parse_ofd] page[{}] id={} objects={} annotations={} physical_box={:.0}x{:.0}",
            idx, page_id, page.objects.len(), page.annotations.len(), page.physical_box.w, page.physical_box.h).into());
        pages.push(page);
    }

    web_sys::console::log_1(&format!("[parse_ofd] TOTAL: {} pages parsed", pages.len()).into());

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
) -> Result<(Vec<(u32, String)>, Option<String>, Option<String>), String> {
    let xml = read_zip_str(archive, doc_root)?;
    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut pages = Vec::new();
    let mut common_data: Option<String> = None;
    let mut annotations: Option<String> = None;
    let mut public_res: Option<String> = None;
    let mut document_res: Option<String> = None;
    let mut in_pages = false;
    let mut in_common = false;
    let mut current_elem = String::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Pages" => in_pages = true,
                    "CommonData" => in_common = true,
                    "Page" if in_pages => {
                        let page_id = attr_val(e, &r, "ID")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                        if let Some(loc) = attr_val(e, &r, "BaseLoc") {
                            pages.push((page_id, loc));
                        }
                    }
                    "PublicRes" if in_common => {
                        current_elem = "PublicRes".to_string();
                    }
                    "DocumentRes" if in_common => {
                        current_elem = "DocumentRes".to_string();
                    }
                    "Annotations" => {
                        current_elem = "Annotations".to_string();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "Page" && in_pages {
                    let page_id = attr_val(e, &r, "ID")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    if let Some(loc) = attr_val(e, &r, "BaseLoc") {
                        pages.push((page_id, loc));
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string().trim().to_string();
                if current_elem == "PublicRes" {
                    public_res = Some(text);
                } else if current_elem == "DocumentRes" {
                    document_res = Some(text);
                } else if current_elem == "Annotations" {
                    annotations = Some(text);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Pages" => in_pages = false,
                    "CommonData" => in_common = false,
                    "PublicRes" | "DocumentRes" | "Annotations" => current_elem.clear(),
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
    let document_res = document_res.unwrap_or_else(|| {
        if base.is_empty() {
            "DocumentRes.xml".to_string()
        } else {
            format!("{}/DocumentRes.xml", base)
        }
    });
    let public_res = public_res.unwrap_or_else(|| {
        if base.is_empty() {
            "PublicRes.xml".to_string()
        } else {
            format!("{}/PublicRes.xml", base)
        }
    });

    // 返回 DocumentRes.xml 作为 CommonData 路径；同时 PublicRes.xml 也作为 CommonData 解析
    // 因为 parse_common_resources 会同时处理字体和图片
    common_data = Some(document_res);

    Ok((pages, common_data, annotations))
}

// ============================================================
// 步骤 3: 公共资源
// ============================================================

/// 资源条目：ResourceID → 文件路径
#[derive(Debug, Clone)]
struct ResEntry {
    id: String,
    media_type: String, // "Image", "Audio", "Video"
    file_path: String,
}

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

    let file_dir = parent_dir(path);
    let mut base_loc = String::new(); // 资源文件基本目录
    let mut resource_entries: Vec<ResEntry> = Vec::new();
    let mut in_multimedia = false;
    let mut current_entry: Option<ResEntry> = None;

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Res" => {
                        // 读取 BaseLoc 属性
                        if let Some(bl) = attr_val(e, &r, "BaseLoc") {
                            base_loc = bl;
                        }
                    }
                    "Font" => {
                        let id = attr_val(e, &r, "ID").unwrap_or_default();
                        let family = attr_val(e, &r, "FamilyName")
                            .or_else(|| attr_val(e, &r, "FontName"))
                            .unwrap_or_else(|| "sans-serif".into());
                        fonts.insert(id, OfdFont { id: String::new(), family });
                    }
                    "MultiMedia" => {
                        in_multimedia = true;
                        let id = attr_val(e, &r, "ID").unwrap_or_default();
                        let media_type = attr_val(e, &r, "Type").unwrap_or_else(|| "Image".into());
                        current_entry = Some(ResEntry { id, media_type, file_path: String::new() });
                    }
                    "MediaFile" if in_multimedia => {
                        if let Some(ref mut entry) = current_entry {
                            // MediaFile 可能包含文件路径作为文本内容
                            // 在 End 事件中捕获
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Res" => {
                        if let Some(bl) = attr_val(e, &r, "BaseLoc") {
                            base_loc = bl;
                        }
                    }
                    "Font" => {
                        let id = attr_val(e, &r, "ID").unwrap_or_default();
                        let family = attr_val(e, &r, "FamilyName")
                            .or_else(|| attr_val(e, &r, "FontName"))
                            .unwrap_or_else(|| "sans-serif".into());
                        fonts.insert(id, OfdFont { id: String::new(), family });
                    }
                    "MultiMedia" => {
                        let id = attr_val(e, &r, "ID").unwrap_or_default();
                        let media_type = attr_val(e, &r, "Type").unwrap_or_else(|| "Image".into());
                        // 空的 MultiMedia，跳过
                        if id.is_empty() { continue; }
                        current_entry = Some(ResEntry { id, media_type, file_path: String::new() });
                    }
                    "MediaFile" if current_entry.is_some() => {
                        // MediaFile 可能有文件路径属性
                        if let Some(ref mut entry) = current_entry {
                            if let Some(fp) = attr_val(e, &r, "FilePath") {
                                entry.file_path = fp;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                if in_multimedia && current_entry.is_some() {
                    let text = e.unescape().unwrap_or_default().to_string().trim().to_string();
                    if let Some(ref mut entry) = current_entry {
                        if entry.file_path.is_empty() && !text.is_empty() {
                            entry.file_path = text;
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "MultiMedia" => {
                        in_multimedia = false;
                        if let Some(entry) = current_entry.take() {
                            if !entry.id.is_empty() && !entry.file_path.is_empty() {
                                resource_entries.push(entry);
                            }
                        }
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

    // 确定资源文件的基本目录
    let res_base_dir = if base_loc.is_empty() {
        if file_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", file_dir)
        }
    } else if file_dir.is_empty() {
        format!("{}/", base_loc)
    } else {
        format!("{}/{}/", file_dir, base_loc)
    };

    // 读取所有引用的图片文件
    for entry in &resource_entries {
        if entry.media_type.eq_ignore_ascii_case("image") {
            let img_path = if entry.file_path.starts_with('/') {
                entry.file_path[1..].to_string()
            } else {
                format!("{}{}", res_base_dir, entry.file_path)
            };
            if let Ok(data) = read_zip_bytes(archive, &img_path) {
                images.insert(entry.id.clone(), data);
            }
        }
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
    fonts: &HashMap<String, OfdFont>,
    annotations_path: Option<&str>,
) -> Result<OfdPage, String> {
    web_sys::console::log_1(&format!("[parse_page_content] try read page_path={}", page_path).into());
    let xml = match read_zip_str(archive, page_path) {
        Ok(x) => x,
        Err(e) => {
            web_sys::console::error_1(&format!("[parse_page_content] failed to read {}: {}", page_path, e).into());
            return Ok(OfdPage {
                index: page_idx,
                physical_box: OfdRect::new(0.0, 0.0, 210.0, 297.0),
                objects: Vec::new(),
                annotations: Vec::new(),
            });
        }
    };

    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut physical_box = OfdRect::new(0.0, 0.0, 210.0, 297.0);
    let mut objects: Vec<OfdObject> = Vec::new();
    let mut current_elem = String::new();

    // 打印原始事件，便于调试
    web_sys::console::log_1(&format!("[parse_page_content] parsing page {} xml_len={}", page_path, xml.len()).into());

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                web_sys::console::log_1(&format!("[parse_page_content] Start tag={}", tag).into());
                match tag.as_str() {
                    "Area" | "PageArea" => {
                        // PhysicalBox 可能作为属性
                        if let Some(pb) = attr_val(e, &r, "PhysicalBox") {
                            physical_box = parse_rect(&pb);
                        }
                        current_elem = tag.clone();
                    }
                    "PhysicalBox" => {
                        current_elem = "PhysicalBox".to_string();
                    }
                    "TextObject" => {
                        let mut sub_buf = Vec::new();
                        let obj = parse_text_object(&mut r, &mut sub_buf, e, fonts)?;
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
                    "Layer" => {
                        // 解析 <Layer> 内嵌对象，加到当前页面
                        let mut sub_buf = Vec::new();
                        if let Ok(layer_objs) = parse_layer_objects(&mut r, &mut sub_buf, fonts) {
                            objects.extend(layer_objs);
                        }
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
            Ok(Event::Text(ref e)) => {
                if current_elem == "PhysicalBox" {
                    let text = e.unescape().unwrap_or_default().to_string();
                    physical_box = parse_rect(text.trim());
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                web_sys::console::log_1(&format!("[parse_page_content] End tag={}", tag).into());
                if tag == "Area" || tag == "PageArea" || tag == "PhysicalBox" {
                    current_elem.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    // 解析页面注释 (Annotation)
    // annotations_path 已是由 parse_ofd 解析好的、相对于 OFD 根目录的完整路径
    let annotations = if let Some(annot_path) = annotations_path {
        parse_page_annotations(archive, annot_path, page_idx, fonts).unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(OfdPage { index: page_idx, physical_box, objects, annotations })
}

/// 解析 <Layer> 元素内部的所有对象，直到遇到 </Layer>
fn parse_layer_objects(
    r: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    fonts: &HashMap<String, OfdFont>,
) -> Result<Vec<OfdObject>, String> {
    let mut objects: Vec<OfdObject> = Vec::new();
    let mut depth: u32 = 1;

    web_sys::console::log_1(&"[parse_layer_objects] start".into());

    loop {
        match r.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                web_sys::console::log_1(&format!("[parse_layer_objects] Start tag={} depth={}", tag, depth).into());
                match tag.as_str() {
                    "Layer" => depth += 1,
                    "TextObject" => {
                        web_sys::console::log_1(&format!("[parse_layer_objects] parsing TextObject depth={}", depth).into());
                        let mut sub_buf = Vec::new();
                        let obj = parse_text_object(r, &mut sub_buf, e, fonts)?;
                        objects.push(OfdObject::Text(obj));
                    }
                    "PathObject" => {
                        let mut sub_buf = Vec::new();
                        let obj = parse_path_object(r, &mut sub_buf, e)?;
                        objects.push(OfdObject::Path(obj));
                    }
                    "ImageObject" => {
                        let mut sub_buf = Vec::new();
                        let obj = parse_image_object(r, &mut sub_buf, e)?;
                        objects.push(OfdObject::Image(obj));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "Layer" {
                    // 空 Layer 不需要调整 depth
                } else {
                    match tag.as_str() {
                        "TextObject" => {
                            let mut sub_buf = Vec::new();
                            let obj = parse_text_object(r, &mut sub_buf, e, fonts)?;
                            objects.push(OfdObject::Text(obj));
                        }
                        "PathObject" => {
                            let mut sub_buf = Vec::new();
                            let obj = parse_path_object(r, &mut sub_buf, e)?;
                            objects.push(OfdObject::Path(obj));
                        }
                        "ImageObject" => {
                            let mut sub_buf = Vec::new();
                            let obj = parse_image_object(r, &mut sub_buf, e)?;
                            objects.push(OfdObject::Image(obj));
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                web_sys::console::log_1(&format!("[parse_layer_objects] End tag={} depth={}", tag, depth).into());
                if tag == "Layer" {
                    if depth > 0 {
                        depth -= 1;
                    }
                    if depth == 0 {
                        break;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Layer 解析错误: {}", e)),
            _ => {}
        }
        buf.clear();
    }

    Ok(objects)
}

// ============================================================
// 对象子解析器
// ============================================================

fn parse_text_object(
    r: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
    start: &quick_xml::events::BytesStart,
    fonts: &HashMap<String, OfdFont>,
) -> Result<OfdTextObject, String> {
    let boundary = attr_val(start, r, "Boundary").map(|s| parse_rect(&s));
    // 标准 OFD: 当 TextObject 未显式提供 CTM 时，对象坐标系原点在 (Boundary.x, Boundary.y)。
    // 因此 TextCode 的 X/Y 应相对于 Boundary 左上角解释。
    let has_ctm = attr_val(start, r, "CTM").is_some();
    let ctm = if has_ctm {
        attr_val(start, r, "CTM").map(|s| parse_ctm(&s)).unwrap()
    } else if let Some(ref b) = boundary {
        [1.0, 0.0, 0.0, 1.0, b.x, b.y]
    } else {
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    };

    let mut font_family = String::from("sans-serif");
    let mut font_size = 4.23; // 默认 ~12pt (mm)
    let mut fill_color = OfdColor::BLACK;
    let mut stroke_color: Option<OfdColor> = None;
    let mut text_items: Vec<OfdTextItem> = Vec::new();

    let mut in_text_code = false;
    let mut current_x = 0.0f64;
    let mut current_y = 0.0f64;
    let mut delta_x_arr: Vec<f64> = Vec::new();
    let mut delta_y_arr: Vec<f64> = Vec::new();

    // 状态追踪：当前进入的直接子元素标签名，用于捕获其文本内容
    // 可能值: "Size", "FontName", "X", "Y", ""(不在目标元素中)
    let mut current_text_elem: String = String::new();

    // 从 TextObject 属性中读取 Font（可能为 ID 或名称）
    if let Some(font_val) = attr_val(start, r, "Font") {
        // 尝试将 Font 值解析为数字 ID → 从 fonts 映射表中查询
        if let Ok(id) = font_val.parse::<u32>() {
            let id_str = id.to_string();
            if let Some(ofd_font) = fonts.get(&id_str) {
                let name = if ofd_font.family.is_empty() { &ofd_font.id } else { &ofd_font.family };
                if !name.is_empty() {
                    font_family = name.clone();
                }
            }
        } else if !font_val.is_empty() {
            font_family = font_val;
        }
    }
    // Size 属性
    if let Some(size_val) = attr_val(start, r, "Size") {
        if let Ok(sz) = size_val.parse::<f64>() {
            font_size = sz;
        }
    }

    loop {
        match r.read_event_into(buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "TextCode" => {
                        in_text_code = true;
                        current_text_elem.clear();
                        current_x = attr_val(e, r, "X")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(current_x);
                        current_y = attr_val(e, r, "Y")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(current_y);
                        if let Some(dx) = attr_val(e, r, "DeltaX") {
                            delta_x_arr = parse_num_array(&dx);
                        }
                        if let Some(dy) = attr_val(e, r, "DeltaY") {
                            delta_y_arr = parse_num_array(&dy);
                        }
                    }
                    "Font" => {
                        current_text_elem.clear();
                        // 兼容格式A: <Font Size="12" FamilyName="..." />
                        if let Some(size) = attr_val(e, r, "Size") {
                            if let Ok(sz) = size.parse::<f64>() {
                                font_size = sz;
                            }
                        }
                        if let Some(family) = attr_val(e, r, "FamilyName")
                            .or_else(|| attr_val(e, r, "FontName"))
                        {
                            if !family.is_empty() {
                                font_family = family;
                            }
                        }
                    }
                    "FillColor" => {
                        current_text_elem.clear();
                        if let Some(v) = attr_val(e, r, "Value") {
                            fill_color = parse_color(&v);
                        }
                    }
                    "StrokeColor" => {
                        current_text_elem.clear();
                        if let Some(v) = attr_val(e, r, "Value") {
                            stroke_color = Some(parse_color(&v));
                        }
                    }
                    // 实际 OFD 文件中 TextObject 的直接子元素（需要捕获文本内容）
                    "Size" | "FontName" | "X" | "Y" => {
                        current_text_elem = tag.clone();
                    }
                    _ => {
                        current_text_elem.clear();
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Font" => {
                        if let Some(size) = attr_val(e, r, "Size") {
                            if let Ok(sz) = size.parse::<f64>() {
                                font_size = sz;
                            }
                        }
                        if let Some(family) = attr_val(e, r, "FamilyName")
                            .or_else(|| attr_val(e, r, "FontName"))
                        {
                            if !family.is_empty() {
                                font_family = family;
                            }
                        }
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
                current_text_elem.clear();
            }
            Ok(Event::Text(ref e)) => {
                let raw = e.unescape().unwrap_or_default();
                let text = raw.to_string();

                if in_text_code {
                    // TextCode 内的文本内容 → 实际显示文字
                    let trimmed_text = text.trim();
                    if !trimmed_text.is_empty() {
                        let char_count = trimmed_text.chars().count();

                        if char_count > 1 && delta_x_arr.len() >= char_count.saturating_sub(1) {
                            // 有完整 DeltaX：按字符拆分，精确控制每个字符位置
                            let mut x = current_x;
                            let y = current_y;
                            for (gi, ch) in trimmed_text.chars().enumerate() {
                                text_items.push(OfdTextItem { x, y, text: ch.to_string() });
                                if gi + 1 < char_count {
                                    x += delta_x_arr.get(gi).copied().unwrap_or(0.0);
                                }
                            }
                            current_x = x;
                        } else {
                            // DeltaX 缺失或不足：把整个字符串交给 Canvas 自动排版
                            text_items.push(OfdTextItem { x: current_x, y: current_y, text: trimmed_text.to_string() });
                        }

                        // 更新 current_y，供后续可能的 TextCode 使用
                        current_y += delta_y_arr.last().copied().unwrap_or(0.0);
                    }
                } else if !current_text_elem.is_empty() {
                    // TextObject 直接子元素的文本内容
                    let trimmed = text.trim();
                    match current_text_elem.as_str() {
                        "Size" => {
                            if let Ok(sz) = trimmed.parse::<f64>() {
                                font_size = sz;
                            }
                        }
                        "FontName" => {
                            if !trimmed.is_empty() {
                                font_family = trimmed.to_string();
                            }
                        }
                        "X" => {
                            if let Ok(val) = trimmed.parse::<f64>() {
                                current_x = val;
                            }
                        }
                        "Y" => {
                            if let Ok(val) = trimmed.parse::<f64>() {
                                current_y = val;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "TextCode" => {
                        in_text_code = false;
                        current_text_elem.clear();
                    }
                    "TextObject" => break,
                    _ => {
                        // 退出任何子元素时清除状态
                        if tag == current_text_elem.as_str()
                            || tag == "Size" || tag == "FontName"
                            || tag == "X" || tag == "Y"
                        {
                            current_text_elem.clear();
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    // 字体大小单位兼容性处理
    // 部分 OFD 实现使用 pt 而非 mm 作为 Size 单位
    // 启发式判断：> 10 的值可能是 pt（因为正文通常不会用 > 10mm 的字体）
    if font_size > 10.0 {
        font_size = font_size * 0.352778; // pt → mm (1pt = 1/72 inch ≈ 0.3528mm)
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
    let has_ctm = attr_val(start, r, "CTM").is_some();
    let ctm = if has_ctm {
        attr_val(start, r, "CTM")
            .map(|s| parse_ctm(&s))
            .unwrap()
    } else if let Some(ref b) = boundary {
        [1.0, 0.0, 0.0, 1.0, b.x, b.y]
    } else {
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    };

    let mut stroke_color: Option<OfdColor> = Some(OfdColor::BLACK);
    let mut fill_color: Option<OfdColor> = None;
    // LineWidth 可能作为 XML 属性或子元素出现；先读属性, 子元素可覆盖
    let mut line_width = attr_val(start, r, "LineWidth")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.353); // ~0.5pt → mm
    let mut path_data = String::new();
    let mut current_path_elem = String::new();

    // PathObject 自身的 Fill/Stroke 属性
    let stroke_attr = attr_val(start, r, "Stroke").unwrap_or_else(|| "true".into());
    let fill_attr = attr_val(start, r, "Fill").unwrap_or_else(|| "false".into());
    let mut should_stroke = stroke_attr.eq_ignore_ascii_case("true");
    let mut should_fill = fill_attr.eq_ignore_ascii_case("true");

    // 如果未指定 FillColor/StrokeColor 但属性为 true，使用默认黑色
    let mut stroke_color_explicit = false;
    let mut fill_color_explicit = false;

    loop {
        match r.read_event_into(buf) {
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "StrokeColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            stroke_color = Some(parse_color(&v));
                            stroke_color_explicit = true;
                        }
                    }
                    "FillColor" => {
                        if let Some(v) = attr_val(e, r, "Value") {
                            fill_color = Some(parse_color(&v));
                            fill_color_explicit = true;
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
                        stroke_color_explicit = true;
                    }
                } else if tag == "FillColor" {
                    if let Some(v) = attr_val(e, r, "Value") {
                        fill_color = Some(parse_color(&v));
                        fill_color_explicit = true;
                    }
                } else if tag == "LineWidth" {
                    current_path_elem = "LineWidth".to_string();
                }
            }
            Ok(Event::Text(ref e)) => {
                if current_path_elem == "LineWidth" {
                    if let Ok(v) = e.unescape().unwrap_or_default().trim().parse::<f64>() {
                        line_width = v;
                    }
                } else {
                    let text = e.unescape().unwrap_or_default();
                    path_data.push_str(&text);
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "PathObject" {
                    break;
                } else if tag == "LineWidth" {
                    current_path_elem.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    // 根据属性确定最终是否描边/填充
    if should_stroke && !stroke_color_explicit {
        stroke_color = Some(OfdColor::BLACK);
    } else if !should_stroke {
        stroke_color = None;
    }
    if should_fill && !fill_color_explicit {
        fill_color = Some(OfdColor::BLACK);
    } else if !should_fill {
        fill_color = None;
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
    let has_ctm = attr_val(start, r, "CTM").is_some();
    let ctm = if has_ctm {
        attr_val(start, r, "CTM")
            .map(|s| parse_ctm(&s))
            .unwrap()
    } else if let Some(ref b) = boundary {
        [1.0, 0.0, 0.0, 1.0, b.x, b.y]
    } else {
        [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    };
    let resource_id = attr_val(start, r, "ResourceID").unwrap_or_default();

    let mut image_data: Vec<u8> = Vec::new();
    let mut img_width = 0.0f64;
    let mut img_height = 0.0f64;

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

    Ok(OfdImageObject { ctm, boundary, resource_id, image_data, img_width, img_height })
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
        Some(pos) => {
            // 不包含末尾的 '/', 返回的目录本身也不带尾部 '/'
            let dir = &path[..pos];
            if dir.is_empty() {
                String::new()
            } else {
                dir.to_string()
            }
        }
        None => String::new(),
    }
}

fn resolve_page_path(base_dir: &str, page_ref: &str) -> String {
    // page_ref 格式通常是 "Pages/Page_0/Content.xml"
    if page_ref.starts_with('/') {
        page_ref[1..].to_string()
    } else if base_dir.is_empty() {
        page_ref.to_string()
    } else {
        format!("{}/{}", base_dir, page_ref)
    }
}

fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .unwrap_or_default()
}

/// 解析 Annotations.xml (Document 级别), 返回 PageID → 该页 Annotation 文件路径的映射
fn parse_annotations_index(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    path: &str,
) -> Result<HashMap<u32, String>, String> {
    let xml = read_zip_str(archive, path)?;
    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut result = HashMap::new();
    let mut current_page: Option<u32> = None;
    let mut current_elem = String::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "Page" {
                    current_page = attr_val(e, &r, "PageID")
                        .and_then(|s| s.parse().ok());
                    current_elem = "Page".to_string();
                } else if tag == "FileLoc" {
                    current_elem = "FileLoc".to_string();
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "Page" {
                    current_page = attr_val(e, &r, "PageID")
                        .and_then(|s| s.parse().ok());
                }
            }
            Ok(Event::Text(ref e)) => {
                if current_elem == "FileLoc" {
                    if let Some(page_id) = current_page {
                        let text = e.unescape().unwrap_or_default().to_string().trim().to_string();
                        if !text.is_empty() {
                            result.insert(page_id, text);
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "Page" {
                    current_page = None;
                    current_elem.clear();
                } else if tag == "FileLoc" {
                    current_elem.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    web_sys::console::log_1(&format!("[parse_annotations_index] path={} entries={:?}", path, result).into());
    Ok(result)
}

/// 解析单个页面的 Annotation.xml, 返回 OfdAnnotation 列表
fn parse_page_annotations(
    archive: &mut zip::ZipArchive<Cursor<Vec<u8>>>,
    path: &str,
    page_idx: u32,
    fonts: &HashMap<String, OfdFont>,
) -> Result<Vec<OfdAnnotation>, String> {
    web_sys::console::log_1(&format!("[parse_page_annotations] try read path={}", path).into());
    let xml = match read_zip_str(archive, path) {
        Ok(x) => x,
        Err(e) => {
            web_sys::console::error_1(&format!("[parse_page_annotations] failed to read {}: {}", path, e).into());
            return Ok(Vec::new());
        }
    };

    let mut r = Reader::from_str(&xml);
    r.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut annotations = Vec::new();
    let mut current_annot_type = String::new();
    let mut current_boundary = OfdRect::new(0.0, 0.0, 0.0, 0.0);
    let mut in_appearance = false;
    let mut appearance_objects: Vec<OfdObject> = Vec::new();
    let mut current_elem = String::new();

    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Annot" => {
                        current_annot_type = attr_val(e, &r, "Type").unwrap_or_default();
                        appearance_objects.clear();
                    }
                    "Appearance" => {
                        in_appearance = true;
                        // 某些 OFD 文件把 Boundary 放在 Appearance 上而非 Annot 上
                        current_boundary = attr_val(e, &r, "Boundary")
                            .map(|s| parse_rect(&s))
                            .unwrap_or(current_boundary);
                    }
                    "Layer" if in_appearance => {
                        // Appearance 内的 Layer: 解析其中的对象
                        if let Ok(mut objs) = parse_layer_objects(&mut r, &mut Vec::new(), fonts) {
                            appearance_objects.append(&mut objs);
                        }
                    }
                    "TextObject" if in_appearance => {
                        let mut sub_buf = Vec::new();
                        if let Ok(obj) = parse_text_object(&mut r, &mut sub_buf, e, fonts) {
                            appearance_objects.push(OfdObject::Text(obj));
                        }
                    }
                    "PathObject" if in_appearance => {
                        let mut sub_buf = Vec::new();
                        if let Ok(obj) = parse_path_object(&mut r, &mut sub_buf, e) {
                            appearance_objects.push(OfdObject::Path(obj));
                        }
                    }
                    "ImageObject" if in_appearance => {
                        let mut sub_buf = Vec::new();
                        if let Ok(obj) = parse_image_object(&mut r, &mut sub_buf, e) {
                            appearance_objects.push(OfdObject::Image(obj));
                        }
                    }
                    _ if in_appearance => {
                        current_elem = tag;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "Appearance" => {
                        in_appearance = false;
                    }
                    "Annot" => {
                        annotations.push(OfdAnnotation {
                            annot_type: current_annot_type.clone(),
                            boundary: current_boundary,
                            objects: appearance_objects.clone(),
                        });
                        appearance_objects.clear();
                    }
                    _ if in_appearance => {
                        current_elem.clear();
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

    web_sys::console::log_1(&format!("[parse_page_annotations] page={} path={} annotations={}", page_idx, path, annotations.len()).into());
    for (i, a) in annotations.iter().enumerate() {
        web_sys::console::log_1(&format!("[annotation {}] type={} boundary={:.1},{:.1},{:.1},{:.1} objects={}",
            i, a.annot_type, a.boundary.x, a.boundary.y, a.boundary.w, a.boundary.h, a.objects.len()).into());
    }

    Ok(annotations)
}

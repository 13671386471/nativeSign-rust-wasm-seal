//! 文档渲染模块 — 基于 PDFium 的真实 PDF 渲染 + Canvas 2D
//!
//! 替代原 Qt WASM 渲染管线:
//!   - PDF 文档: 使用 pdfium-render 在 WASM 内部渲染为位图,绘制到 Canvas
//!   - 全部页面一次性渲染, 垂直堆叠在滚动容器中, 实现丝滑连续滚动
//!   - OFD 文档: 使用 Canvas 2D 占位渲染(待实现完整 OFD 解析)
//!   - 印章叠加: Canvas 覆盖层绘制

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use pdfium_render::prelude::*;
use crate::types::*;
use crate::ofd_parser;

// ============================================================
// PDFium 全局单例 — 只初始化一次,避免重复绑定报错
// ============================================================

static mut PDFIUM: Option<Pdfium> = None;
static PDFIUM_INIT: std::sync::Once = std::sync::Once::new();

fn get_pdfium() -> Result<&'static Pdfium, JsValue> {
    unsafe {
        PDFIUM_INIT.call_once(|| {
            // 注意: 中文字体提供器不再通过 pdfium-render 的 set_custom_font_provider() 注册。
            // 原因: 该 API 会把 Rust wasm 模块的 extern "C" 函数指针(即本模块的函数表索引)
            // 直接传给 PDFium 的 FPDF_SetSystemFontInfo, 但 PDFium 运行在独立的 wasm 模块
            // (pdfium.js) 中, 无法跨模块调用这些指针, 导致字体映射时触发 wasm trap → panic。
            // 正确的做法由 JS 端 installChineseFontProvider() 完成: 它把字体回调打补丁到
            // PDFium 自身的函数表中(与 pdfium-render 处理文件回调的机制一致), 从而支持跨模块。
            // 此处仅初始化 PDFium 实例即可。
            web_sys::console::log_1(&"[render] PDFium 实例初始化 (字体提供器由 JS 端安装)".into());
            let pdfium = Pdfium::default();
            PDFIUM = Some(pdfium);
        });
        PDFIUM.as_ref()
            .ok_or_else(|| JsValue::from_str("PDFium 初始化失败"))
    }
}

// ============================================================
// 渲染引擎配置
// ============================================================

const PAGE_GAP: i32 = 8; // 页面之间的间距(px)

#[derive(Debug, Clone)]
pub struct RenderConfig {
    pub zoom: f64,
    pub page_mode: PageMode,
    pub columns: u32,
    pub rotation: i32,
    pub show_toolbar: bool,
    pub show_menu: bool,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            page_mode: PageMode::FitWidth,
            columns: 1,
            rotation: 0,
            show_toolbar: true,
            show_menu: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageMode {
    SinglePage = 1,
    FitWidth = 2,
    FitPage = 4,
    MultiColumn = 8,
    Continuous = 16,
    Original = 32,
    TwoPage = 64,
}

// ============================================================
// 渲染引擎
// ============================================================

pub struct RenderEngine {
    config: RenderConfig,
    container_id: String,
    current_page: u32,
}

impl RenderEngine {
    pub fn new(container_id: &str) -> Self {
        Self {
            config: RenderConfig::default(),
            container_id: container_id.to_string(),
            current_page: 0,
        }
    }

    // ===== 页面模式 =====

    pub fn set_page_mode(&mut self, mode: i32, param: i32) {
        match mode {
            1 => {
                self.config.page_mode = PageMode::Original;
                self.config.zoom = param as f64 / 100.0;
            }
            2 => self.config.page_mode = PageMode::FitWidth,
            4 => self.config.page_mode = PageMode::FitPage,
            8 => {
                self.config.page_mode = PageMode::MultiColumn;
                self.config.columns = param as u32;
            }
            32 => {
                self.config.page_mode = PageMode::Original;
                self.config.zoom = 1.0;
            }
            _ => {}
        }
    }

    // ===== 指针事件控制 =====

    pub fn set_pointer_events(&self, enabled: bool) -> Result<(), JsValue> {
        // 对全部页面 canvas 设置 pointer-events
        let window = web_sys::window().ok_or(JsValue::from_str("无 window"))?;
        let document = window.document().ok_or(JsValue::from_str("无 document"))?;
        let value = if enabled { "auto" } else { "none" };

        let container = document
            .get_element_by_id(&self.container_id)
            .ok_or_else(|| JsValue::from_str("找不到容器"))?;

        // 遍历全部子元素, 处理 canvas 元素
        let mut child = container.first_element_child();
        while let Some(el) = child {
            if el.tag_name().to_lowercase() == "canvas" {
                let w = el.get_attribute("width").unwrap_or_default();
                let h = el.get_attribute("height").unwrap_or_default();
                el.set_attribute("style", &format!(
                    "display: block; margin: 0 auto; width: {}px; height: {}px; pointer-events: {};",
                    w, h, value
                )).ok();
            }
            child = el.next_element_sibling();
        }
        Ok(())
    }

    // ===== 页面导航 (兼容旧 API, 现在主要由 JS 滚动控制) =====

    pub fn set_current_page(&mut self, page: u32, max_page: u32) {
        self.current_page = page.min(max_page.saturating_sub(1));
    }

    pub fn get_current_page(&self) -> u32 {
        self.current_page
    }

    pub fn next_page(&mut self, max_page: u32) {
        if self.current_page + 1 < max_page {
            self.current_page += 1;
        }
    }

    pub fn prev_page(&mut self) {
        if self.current_page > 0 {
            self.current_page -= 1;
        }
    }

    // ===== 清除全部页面 canvas =====

    fn clear_all_canvases(&self) {
        let window = match web_sys::window() {
            Some(w) => w,
            None => return,
        };
        let document = match window.document() {
            Some(d) => d,
            None => return,
        };
        let container = match document.get_element_by_id(&self.container_id) {
            Some(c) => c,
            None => return,
        };

        // 移除以 __pg_ 开头的 canvas 子元素
        let mut to_remove: Vec<web_sys::Element> = Vec::new();
        let mut child = container.first_element_child();
        while let Some(el) = child {
            if el.tag_name().to_lowercase() == "canvas" {
                if el.id().starts_with("__pg_") {
                    to_remove.push(el.clone());
                }
            }
            child = el.next_element_sibling();
        }
        for el in to_remove {
            container.remove_child(&el).ok();
        }
    }

    // ===== 主渲染入口 =====

    /// 刷新渲染 — 渲染全部页面为连续滚动画布
    pub fn refresh(&self, doc_state: &DocState) -> Result<(), JsValue> {
        // 先清除旧页面
        self.clear_all_canvases();

        match doc_state.doc_type {
            DocType::Pdf => self.render_all_pdf_pages(doc_state)?,
            DocType::Ofd => self.render_all_ofd_pages(doc_state)?,
        }

        // 通知 JS 重新绑定滚动监听
        let js = r#"
        (function() {
            if (typeof window.__wasm_on_pages_rendered === 'function') {
                window.__wasm_on_pages_rendered();
            }
        })()
        "#;
        js_sys::eval(js).ok();

        Ok(())
    }

    /// 一次性渲染全部 PDF 页面 — 每个页面一个 canvas, 垂直堆叠
    fn render_all_pdf_pages(&self, doc_state: &DocState) -> Result<(), JsValue> {
        web_sys::console::log_1(&format!("[render_pdf] START raw_data_len={}", doc_state.raw_data.len()).into());

        let pdfium = get_pdfium()?;
        web_sys::console::log_1(&format!("[render_pdf] PDFium instance obtained").into());

        // 只加载一次 PDF
        let document = pdfium
            .load_pdf_from_byte_vec(doc_state.raw_data.clone(), None)
            .map_err(|e| {
                web_sys::console::log_1(&format!("[render_pdf] PDF 加载失败: {}", e).into());
                JsValue::from_str(&format!("PDF 加载失败: {}", e))
            })?;
        web_sys::console::log_1(&format!("[render_pdf] PDF loaded successfully").into());

        let page_count = document.pages().len() as u32;
        web_sys::console::log_1(&format!("[render_pdf] page_count={}", page_count).into());
        if page_count == 0 {
            web_sys::console::log_1(&"[render_pdf] page_count=0, returning early".into());
            return Ok(());
        }

        // 获取容器宽度作为 FitWidth/FitPage 的参考
        let window = web_sys::window().ok_or(JsValue::from_str("无 window"))?;
        let document_js = window.document().ok_or(JsValue::from_str("无 document"))?;
        let container = document_js
            .get_element_by_id(&self.container_id)
            .ok_or_else(|| JsValue::from_str(&format!("找不到容器 #{}", self.container_id)))?;
        let container_w = container.client_width() as f64;
        let container_h_for_fit = (container.client_height() as f64).max(600.0);
        web_sys::console::log_1(&format!("[render_pdf] container_w={} container_h={}", container_w, container_h_for_fit).into());

        // 预计算每页的渲染尺寸
        let mut page_sizes: Vec<(f64, f64, i32, i32)> = Vec::new();
        for i in 0..page_count {
            if let Ok(page) = document.pages().get(i as i32) {
                let pw = page.width().value as f64;
                let ph = page.height().value as f64;
                let (tw, th) = self.calc_render_size(pw, ph, container_w, container_h_for_fit);
                web_sys::console::log_1(&format!("[render_pdf] page[{}] pdf_size={:.1}x{:.1} → target={}x{}", i, pw, ph, tw, th).into());
                page_sizes.push((pw, ph, tw, th));
            }
        }

        // 逐页渲染
        for i in 0..page_count {
            let (_pw, _ph, target_w, target_h) = page_sizes[i as usize];
            let canvas = self.create_page_canvas(i, target_w, target_h, page_count)?;

            let page = document.pages().get(i as i32)
                .map_err(|e| JsValue::from_str(&format!("页面 {} 不存在: {}", i, e)))?;

            web_sys::console::log_1(&format!("[render_pdf] rendering page[{}] target={}x{}", i, target_w, target_h).into());
            self.render_page_to_canvas(&page, &canvas, target_w, target_h, i, doc_state)?;
            web_sys::console::log_1(&format!("[render_pdf] page[{}] done", i).into());
        }

        web_sys::console::log_1(&format!("[render_pdf] ALL {} pages rendered", page_count).into());
        Ok(())
    }

    /// 为一页创建 canvas 元素
    fn create_page_canvas(&self, page_idx: u32, w: i32, h: i32, total_pages: u32) -> Result<HtmlCanvasElement, JsValue> {
        let window = web_sys::window().ok_or(JsValue::from_str("无 window"))?;
        let document = window.document().ok_or(JsValue::from_str("无 document"))?;

        let container = document
            .get_element_by_id(&self.container_id)
            .ok_or_else(|| JsValue::from_str(&format!("找不到容器 #{}", self.container_id)))?;

        let canvas_id = format!("__pg_{}_p{}", self.container_id, page_idx);

        // 如果已存在则复用
        if let Some(existing) = document.get_element_by_id(&canvas_id) {
            let canvas: HtmlCanvasElement = existing
                .dyn_into()
                .map_err(|_| JsValue::from_str("元素不是 canvas"))?;
            canvas.set_width(w as u32);
            canvas.set_height(h as u32);
            let margin_bottom = if page_idx + 1 < total_pages { PAGE_GAP } else { 0 };
            canvas.set_attribute("style", &format!(
                "display: block; margin: 0 auto {}px auto; width: {}px; height: {}px;",
                margin_bottom, w, h
            ))?;
            return Ok(canvas);
        }

        let canvas: HtmlCanvasElement = document
            .create_element("canvas")
            .map_err(|_| JsValue::from_str("创建 canvas 失败"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("类型转换失败"))?;

        canvas.set_id(&canvas_id);
        canvas.set_width(w as u32);
        canvas.set_height(h as u32);

        let margin_bottom = if page_idx + 1 < total_pages { PAGE_GAP } else { 0 };
        canvas.set_attribute("style", &format!(
            "display: block; margin: 0 auto {}px auto; width: {}px; height: {}px;",
            margin_bottom, w, h
        ))?;

        // 设置 data-page 属性方便 JS 识别
        canvas.set_attribute("data-page", &page_idx.to_string())?;

        container.append_child(&canvas)
            .map_err(|_| JsValue::from_str("添加 canvas 到容器失败"))?;

        Ok(canvas)
    }

    /// 将单个 PDF 页面渲染到 canvas
    fn render_page_to_canvas(
        &self,
        page: &PdfPage,
        canvas: &HtmlCanvasElement,
        target_w: i32,
        target_h: i32,
        page_idx: u32,
        doc_state: &DocState,
    ) -> Result<(), JsValue> {
        web_sys::console::log_1(&format!("[render_page] page[{}] target={}x{}", page_idx, target_w, target_h).into());

        let render_cfg = PdfRenderConfig::new()
            .set_target_width(target_w)
            .set_maximum_height(target_h * 2)
            .render_form_data(true);

        let bitmap = page
            .render_with_config(&render_cfg)
            .map_err(|e| {
                web_sys::console::log_1(&format!("[render_page] page[{}] render_with_config FAILED: {:?}", page_idx, e).into());
                JsValue::from_str(&format!("页面 {} 渲染失败: {:?}", page_idx, e))
            })?;

        let bmp_w = bitmap.width() as u32;
        let bmp_h = bitmap.height() as u32;
        web_sys::console::log_1(&format!("[render_page] page[{}] bitmap rendered, size={}x{}", page_idx, bmp_w, bmp_h).into());

        // 诊断: 统计非白色像素, 用于确认文字是否真的被渲染 (字体是否生效)
        let rgba = bitmap.as_rgba_bytes();
        let expected = bmp_w as usize * bmp_h as usize * 4;
        let non_white = rgba
            .chunks(4)
            .filter(|px| px.len() == 4 && (px[0] != 255 || px[1] != 255 || px[2] != 255))
            .count();
        web_sys::console::log_1(&format!(
            "[render_page] page[{}] non_white_pixels={} (rgba_len={}, expected={})",
            page_idx, non_white, rgba.len(), expected
        ).into());
        if non_white == 0 {
            web_sys::console::warn_1(&format!(
                "[render_page] page[{}] 位图全白! 若文档含文字, 请检查字体提供器是否生效",
                page_idx
            ).into());
        }

        // 转换为 ImageData 并直接绘制到 canvas
        // (pdfium-render 的 as_image_data 内部已做 BGRA→RGBA 转换与 stride 处理)
        let image_data = bitmap
            .as_image_data()
            .map_err(|e| {
                web_sys::console::log_1(&format!("[render_page] page[{}] as_image_data FAILED: {:?}", page_idx, e).into());
                JsValue::from_str(&format!("ImageData 转换失败: {:?}", e))
            })?;

        // 将 canvas 尺寸调整为 bitmap 实际尺寸, 避免 put_image_data 因尺寸不匹配而裁剪/报错
        canvas.set_width(bmp_w);
        canvas.set_height(bmp_h);
        let margin_bottom = if page_idx + 1 < doc_state.page_count { PAGE_GAP } else { 0 };
        canvas.set_attribute(
            "style",
            &format!(
                "display: block; margin: 0 auto {}px auto; width: {}px; height: {}px;",
                margin_bottom, bmp_w, bmp_h
            ),
        )?;

        let ctx = canvas
            .get_context("2d")
            .map_err(|_| JsValue::from_str("无法获取 Canvas 2D 上下文"))?
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsValue::from_str("无法转换为 CanvasRenderingContext2d"))?;

        ctx.put_image_data(&image_data, 0.0, 0.0)
            .map_err(|e| {
                web_sys::console::log_1(&format!("[render_page] page[{}] put_image_data FAILED: {:?}", page_idx, e).into());
                JsValue::from_str(&format!("ImageData 绘制到 Canvas 失败: {:?}", e))
            })?;
        web_sys::console::log_1(&format!("[render_page] page[{}] put_image_data OK", page_idx).into());

        // 印章叠加
        let pdf_pw = page.width().value as f64;
        let pdf_ph = page.height().value as f64;
        for seal in &doc_state.seals {
            if seal.page_index == page_idx {
                self.render_seal(&ctx, seal, pdf_pw, pdf_ph, bmp_w as f64, bmp_h as f64)?;
            }
        }

        Ok(())
    }

    /// 计算页面的渲染目标尺寸
    fn calc_render_size(&self, page_w: f64, page_h: f64, container_w: f64, container_h: f64) -> (i32, i32) {
        match self.config.page_mode {
            PageMode::FitWidth => {
                let w = (container_w * self.config.zoom) as i32;
                let h = ((page_h / page_w) * container_w * self.config.zoom) as i32;
                (w.max(1), h.max(1))
            }
            PageMode::FitPage => {
                let scale_w = container_w / page_w;
                let scale_h = container_h / page_h;
                let scale = scale_w.min(scale_h) * self.config.zoom;
                ((page_w * scale) as i32, (page_h * scale) as i32)
            }
            _ => {
                ((page_w * self.config.zoom) as i32, (page_h * self.config.zoom) as i32)
            }
        }
    }

    // ============================================================
    // OFD 全部页面渲染 — 基于 quick-xml + zip 真实解析
    // ============================================================

    fn render_all_ofd_pages(&self, doc_state: &DocState) -> Result<(), JsValue> {
        web_sys::console::log_1(&format!("[render] render_all_ofd_pages called, doc_type={:?}, raw_data_len={}",
            doc_state.doc_type, doc_state.raw_data.len()).into());

        // 解析 OFD 文档
        let ofd = ofd_parser::parse_ofd(&doc_state.raw_data)
            .map_err(|e| {
                web_sys::console::log_1(&format!("[render] OFD 解析失败: {}", e).into());
                JsValue::from_str(&format!("OFD 解析失败: {}", e))
            })?;

        let page_count = ofd.pages.len() as u32;
        web_sys::console::log_1(&format!("[render] OFD parsed: {} pages", page_count).into());
        if page_count == 0 {
            return Ok(());
        }

        // 获取容器宽度
        let window = web_sys::window().ok_or(JsValue::from_str("无 window"))?;
        let document_js = window.document().ok_or(JsValue::from_str("无 document"))?;
        let container = document_js
            .get_element_by_id(&self.container_id)
            .ok_or_else(|| JsValue::from_str("找不到容器"))?;
        let container_w = container.client_width() as f64;

        // 缩放: mm → px (1mm = 72/25.4 ≈ 2.835px, 即 A4=210×297mm → 595×842px)
        let base_scale = 595.0 / 210.0; // ~2.833 px/mm

        for page in &ofd.pages {
            let pb = &page.physical_box;

            // 计算页面像素尺寸
            let page_w_px = (pb.w * base_scale * self.config.zoom) as i32;
            let page_h_px = (pb.h * base_scale * self.config.zoom) as i32;
            let pw = page_w_px.max(1);
            let ph = page_h_px.max(1);

        let canvas = self.create_page_canvas(page.index, pw, ph, page_count)?;
        web_sys::console::log_1(&format!("[render] rendering page[{}] canvas={}x{} scale={:.3}",
            page.index, pw, ph, base_scale * self.config.zoom).into());
        let _ctx = self.render_ofd_canvas(&canvas, page, pb, base_scale, doc_state)?;
        }

        Ok(())
    }

    /// 将 OFD 页面渲染到 canvas
    fn render_ofd_canvas(
        &self,
        canvas: &HtmlCanvasElement,
        page: &ofd_parser::OfdPage,
        physical_box: &ofd_parser::OfdRect,
        base_scale: f64,
        doc_state: &DocState,
    ) -> Result<CanvasRenderingContext2d, JsValue> {
        let ctx = canvas
            .get_context("2d")
            .map_err(|_| JsValue::from_str("无法获取 Canvas 2D 上下文"))?
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsValue::from_str("无法转换为 CanvasRenderingContext2d"))?;

        let w = canvas.width() as f64;
        let h = canvas.height() as f64;

        // 白色背景
        ctx.set_fill_style_str("#FFFFFF");
        ctx.fill_rect(0.0, 0.0, w, h);

        let scale = base_scale * self.config.zoom;

        // OFD 坐标系: 原点在左上角, X 向右, Y 向下
        // Canvas 2D 坐标系相同, 直接应用 scale
        ctx.save();
        ctx.scale(scale, scale)?;

        for obj in &page.objects {
            match obj {
                ofd_parser::OfdObject::Text(text_obj) => {
                    self.render_ofd_text(&ctx, text_obj, scale)?;
                }
                ofd_parser::OfdObject::Path(path_obj) => {
                    self.render_ofd_path(&ctx, path_obj)?;
                }
                ofd_parser::OfdObject::Image(img_obj) => {
                    // 图片渲染暂缓 (需要 image crate 解码)
                    let _ = img_obj;
                }
            }
        }

        ctx.restore();

        // 印章叠加 (坐标需从 mm 转为像素)
        for seal in &doc_state.seals {
            if seal.page_index == page.index {
                self.render_seal_ofd(&ctx, seal, physical_box, scale)?;
            }
        }

        Ok(ctx)
    }

    /// 渲染 OFD 文本对象
    fn render_ofd_text(
        &self,
        ctx: &CanvasRenderingContext2d,
        obj: &ofd_parser::OfdTextObject,
        scale: f64,
    ) -> Result<(), JsValue> {
        // 只记录前3个文本对象的调试信息
        static mut LOG_COUNT: u32 = 0;
        unsafe {
            if LOG_COUNT < 3 {
                web_sys::console::log_1(&format!("[render_text] font={} size={}mm→{}px items={} text[0]={:?}",
                    obj.font_family, obj.font_size, obj.font_size * scale,
                    obj.text_items.len(),
                    obj.text_items.get(0).map(|t| t.text.clone()).unwrap_or_default()
                ).into());
                LOG_COUNT += 1;
            }
        }

        ctx.save();

        // 应用 CTM 变换
        // OFD CTM [a b c d e f] ↔ Canvas setTransform(a, b, c, d, e, f)
        let ctm = obj.ctm;
        ctx.transform(ctm[0], ctm[1], ctm[2], ctm[3], ctm[4], ctm[5])?;

        // 字体: OFD 字体大小单位为 mm
        // Canvas set_font 的 font-size 单位是 CSS 像素, 不受 ctx.scale() 变换影响
        // 因此需要手动乘以 scale 将 mm 转为屏幕像素: screen_px = font_size_mm * scale
        let font_family = if obj.font_family.is_empty() { "sans-serif" } else { &obj.font_family };
        let screen_font_size = obj.font_size * scale;
        ctx.set_font(&format!("{}px {}", screen_font_size, font_family));

        // 文字颜色
        ctx.set_fill_style_str(&obj.fill_color.to_css());

        // 绘制各文本段
        for item in &obj.text_items {
            // OFD 默认使用 baseline 对齐
            ctx.set_text_baseline("alphabetic");
            ctx.set_text_align("start");
            ctx.fill_text(&item.text, item.x, item.y)
                .map_err(|_| JsValue::from_str("文本绘制失败"))?;
        }

        ctx.restore();
        Ok(())
    }

    /// 渲染 OFD 路径对象 (SVG 风格缩略路径)
    fn render_ofd_path(
        &self,
        ctx: &CanvasRenderingContext2d,
        obj: &ofd_parser::OfdPathObject,
    ) -> Result<(), JsValue> {
        ctx.save();

        let ctm = obj.ctm;
        ctx.transform(ctm[0], ctm[1], ctm[2], ctm[3], ctm[4], ctm[5])?;

        // 解析并执行路径命令
        exec_path_commands(ctx, &obj.path_data)?;

        // 填充
        if let Some(ref fill) = obj.fill_color {
            ctx.set_fill_style_str(&fill.to_css());
            ctx.fill();
        }

        // 描边
        if let Some(ref stroke) = obj.stroke_color {
            ctx.set_stroke_style_str(&stroke.to_css());
            ctx.set_line_width(obj.line_width.max(0.1));
            ctx.stroke();
        }

        ctx.restore();
        Ok(())
    }

    /// OFD 印章渲染 (坐标基于物理页面, 单位 mm)
    fn render_seal_ofd(
        &self,
        ctx: &CanvasRenderingContext2d,
        seal: &PlacedSeal,
        physical_box: &ofd_parser::OfdRect,
        scale: f64,
    ) -> Result<(), JsValue> {
        // 印章坐标 (归一化到页面物理尺寸)
        let sx = (seal.x / physical_box.w) * physical_box.w;
        let sy = (seal.y / physical_box.h) * physical_box.h;
        let sw = seal.width * self.config.zoom / scale;
        let sh = seal.height * self.config.zoom / scale;

        let cx = sx + sw / 2.0;
        let cy = sy + sh / 2.0;
        let radius = sw.min(sh) / 2.0;

        ctx.save();
        ctx.begin_path();
        ctx.arc(cx, cy, radius, 0.0, std::f64::consts::PI * 2.0)
            .map_err(|_| JsValue::from_str("印章路径绘制失败"))?;
        ctx.set_stroke_style_str("#D81E06");
        ctx.set_line_width(0.5);
        ctx.stroke();

        ctx.set_font("3px sans-serif");
        ctx.set_fill_style_str("#D81E06");
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        ctx.fill_text(&seal.seal_info.seal_name, cx, cy).ok();

        if seal.signed {
            ctx.set_fill_style_str("#52C41A");
            ctx.set_font("4px sans-serif");
            ctx.fill_text("✓", cx + radius * 0.7, cy - radius * 0.7).ok();
        }
        ctx.restore();

        Ok(())
    }

    // ============================================================
    // 印章渲染
    // ============================================================

    fn render_seal(
        &self,
        ctx: &CanvasRenderingContext2d,
        seal: &PlacedSeal,
        pdf_page_w: f64,
        pdf_page_h: f64,
        bmp_w: f64,
        bmp_h: f64,
    ) -> Result<(), JsValue> {
        // seal.x/y 是 PDF 点坐标 (0~595, 0~842), 原点在左上角 (由点击坐标转换而来)
        // 需要缩放到位图像素坐标
        let scale_x = bmp_w / pdf_page_w;
        let scale_y = bmp_h / pdf_page_h;
        let sx = seal.x * scale_x;
        let sy = seal.y * scale_y;
        let sw = seal.width * scale_x;
        let sh = seal.height * scale_y;

        let cx = sx + sw / 2.0;
        let cy = sy + sh / 2.0;
        let radius = sw.min(sh) / 2.0;

        // 如果有印章图片, 尝试绘制
        if !seal.seal_info.seal_image.is_empty() {
            // 解码 base64 PNG 并绘制
            use base64::Engine;
            if let Ok(img_bytes) = base64::engine::general_purpose::STANDARD.decode(&seal.seal_info.seal_image) {
                // 用 PNG 解码获取像素数据
                if let Some((iw, ih, rgba)) = decode_png_to_rgba(&img_bytes) {
                    // 创建临时 canvas 绘制图片 (带 alpha 通道)
                    let window = web_sys::window().ok_or(JsValue::from_str("无 window"))?;
                    let document = window.document().ok_or(JsValue::from_str("无 document"))?;
                    if let Ok(tmp_canvas) = document.create_element("canvas") {
                        let tmp: HtmlCanvasElement = tmp_canvas.dyn_into().map_err(|_| JsValue::from_str("canvas 转换失败"))?;
                        tmp.set_width(iw);
                        tmp.set_height(ih);
                        if let Ok(tmp_ctx_obj) = tmp.get_context("2d") {
                            if let Some(tmp_ctx_obj) = tmp_ctx_obj {
                                if let Ok(tmp_ctx) = tmp_ctx_obj.dyn_into::<CanvasRenderingContext2d>() {
                                    // 创建 ImageData 并绘制
                                    if let Ok(img_data) = web_sys::ImageData::new_with_u8_clamped_array_and_sh(
                                        wasm_bindgen::Clamped(&rgba), iw, ih,
                                    ) {
                                        let _ = tmp_ctx.put_image_data(&img_data, 0.0, 0.0);
                                        // 缩放绘制到主 canvas
                                        let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
                                            &tmp, sx, sy, sw, sh,
                                        );
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 回退: 绘制圆形+文字
        ctx.save();
        ctx.begin_path();
        ctx.arc(cx, cy, radius, 0.0, std::f64::consts::PI * 2.0)
            .map_err(|_| JsValue::from_str("绘制印章失败"))?;

        ctx.set_stroke_style_str("#D81E06");
        ctx.set_line_width(2.0);
        ctx.stroke();

        ctx.set_font(format!("{}px sans-serif", (radius * 0.3) as i32).as_str());
        ctx.set_fill_style_str("#D81E06");
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        ctx.fill_text(&seal.seal_info.seal_name, cx, cy).ok();

        if seal.signed {
            ctx.set_fill_style_str("#52C41A");
            ctx.set_font(format!("{}px sans-serif", (radius * 0.5) as i32).as_str());
            ctx.fill_text("\u{2713}", cx + radius * 0.7, cy - radius * 0.7).ok();
        }
        ctx.restore();

        Ok(())
    }

    // ============================================================
    // 缩放控制
    // ============================================================

    pub fn zoom_in(&mut self) {
        self.config.zoom = (self.config.zoom * 1.2).min(5.0);
    }

    pub fn zoom_out(&mut self) {
        self.config.zoom = (self.config.zoom / 1.2).max(0.1);
    }

    pub fn get_zoom(&self) -> f64 {
        self.config.zoom
    }

    // ============================================================
    // 重置
    // ============================================================

    pub fn reset(&mut self) {
        self.current_page = 0;
        self.config.zoom = 1.0;
        self.clear_all_canvases();
    }

    // ============================================================
    // 兼容原 API
    // ============================================================

    pub fn set_js_env(&mut self, _env: i32) {}

    pub fn set_show_toolbar(&mut self, show: i32) {
        self.config.show_toolbar = show != 0;
    }

    pub fn set_show_def_menu(&mut self, show: i32) {
        self.config.show_menu = show != 0;
    }
}

impl Default for RenderEngine {
    fn default() -> Self {
        Self::new("screen")
    }
}

// ============================================================
// SVG 风格路径命令执行器 (用于 OFD AbbreviatedData)
// ============================================================

/// 解析并执行 OFD 缩略路径数据 (兼容 SVG path 子集)
/// 支持: M/m, L/l, C/c, Q/q, A/a, Z/z, H/h, V/v
fn exec_path_commands(
    ctx: &CanvasRenderingContext2d,
    data: &str,
) -> Result<(), JsValue> {
    let tokens = tokenize_path(data);
    let mut i = 0usize;
    let (mut cx, mut cy) = (0.0f64, 0.0f64); // current point
    let (mut sx, mut sy) = (0.0f64, 0.0f64); // sub-path start

    while i < tokens.len() {
        let cmd = &tokens[i];
        i += 1;

        match cmd.as_str() {
            // ---- 绝对命令 ----
            "M" => {
                // 收集连续的坐标对
                while i + 1 < tokens.len() && is_num(&tokens[i]) {
                    let x: f64 = tokens[i].parse().unwrap_or(cx);
                    let y: f64 = tokens[i + 1].parse().unwrap_or(cy);
                    ctx.move_to(x, y);
                    cx = x; cy = y; sx = x; sy = y;
                    i += 2;
                }
            }
            "L" => {
                while i + 1 < tokens.len() && is_num(&tokens[i]) {
                    let x: f64 = tokens[i].parse().unwrap_or(cx);
                    let y: f64 = tokens[i + 1].parse().unwrap_or(cy);
                    ctx.line_to(x, y);
                    cx = x; cy = y;
                    i += 2;
                }
            }
            "C" => {
                while i + 5 < tokens.len() && is_num(&tokens[i]) {
                    let x1: f64 = tokens[i].parse().unwrap_or(0.0);
                    let y1: f64 = tokens[i + 1].parse().unwrap_or(0.0);
                    let x2: f64 = tokens[i + 2].parse().unwrap_or(0.0);
                    let y2: f64 = tokens[i + 3].parse().unwrap_or(0.0);
                    let x: f64 = tokens[i + 4].parse().unwrap_or(cx);
                    let y: f64 = tokens[i + 5].parse().unwrap_or(cy);
                    ctx.bezier_curve_to(x1, y1, x2, y2, x, y);
                    cx = x; cy = y;
                    i += 6;
                }
            }
            "Q" => {
                while i + 3 < tokens.len() && is_num(&tokens[i]) {
                    let x1: f64 = tokens[i].parse().unwrap_or(0.0);
                    let y1: f64 = tokens[i + 1].parse().unwrap_or(0.0);
                    let x: f64 = tokens[i + 2].parse().unwrap_or(cx);
                    let y: f64 = tokens[i + 3].parse().unwrap_or(cy);
                    ctx.quadratic_curve_to(x1, y1, x, y);
                    cx = x; cy = y;
                    i += 4;
                }
            }
            "A" => {
                // arc: rx ry x-axis-rotation large-arc-flag sweep-flag x y
                while i + 6 < tokens.len() && is_num(&tokens[i]) {
                    let rx: f64 = tokens[i].parse().unwrap_or(0.0);
                    let ry: f64 = tokens[i + 1].parse().unwrap_or(0.0);
                    let _rot: f64 = tokens[i + 2].parse().unwrap_or(0.0);
                    let _large: f64 = tokens[i + 3].parse().unwrap_or(0.0);
                    let sweep: f64 = tokens[i + 4].parse().unwrap_or(0.0);
                    let x: f64 = tokens[i + 5].parse().unwrap_or(cx);
                    let y: f64 = tokens[i + 6].parse().unwrap_or(cy);
                    // Canvas 2D 没有原生 arcTo 椭圆支持, 用简化的 ellipse
                    // 这里做近似: ignore rotation, 取平均半径
                    let r = (rx + ry) / 2.0;
                    if r > 0.0 {
                        ctx.arc(x, y, r, 0.0, std::f64::consts::PI * 2.0)
                            .ok();
                    } else {
                        ctx.line_to(x, y);
                    }
                    cx = x; cy = y;
                    i += 7;
                }
            }
            "Z" | "z" => {
                ctx.close_path();
                cx = sx; cy = sy;
            }
            // ---- 相对命令 ----
            "m" => {
                while i + 1 < tokens.len() && is_num(&tokens[i]) {
                    let x: f64 = cx + tokens[i].parse::<f64>().unwrap_or(0.0);
                    let y: f64 = cy + tokens[i + 1].parse::<f64>().unwrap_or(0.0);
                    ctx.move_to(x, y);
                    cx = x; cy = y; sx = x; sy = y;
                    i += 2;
                }
            }
            "l" => {
                while i + 1 < tokens.len() && is_num(&tokens[i]) {
                    let x: f64 = cx + tokens[i].parse::<f64>().unwrap_or(0.0);
                    let y: f64 = cy + tokens[i + 1].parse::<f64>().unwrap_or(0.0);
                    ctx.line_to(x, y);
                    cx = x; cy = y;
                    i += 2;
                }
            }
            "c" => {
                while i + 5 < tokens.len() && is_num(&tokens[i]) {
                    let x1 = cx + tokens[i].parse::<f64>().unwrap_or(0.0);
                    let y1 = cy + tokens[i + 1].parse::<f64>().unwrap_or(0.0);
                    let x2 = cx + tokens[i + 2].parse::<f64>().unwrap_or(0.0);
                    let y2 = cy + tokens[i + 3].parse::<f64>().unwrap_or(0.0);
                    let x = cx + tokens[i + 4].parse::<f64>().unwrap_or(0.0);
                    let y = cy + tokens[i + 5].parse::<f64>().unwrap_or(0.0);
                    ctx.bezier_curve_to(x1, y1, x2, y2, x, y);
                    cx = x; cy = y;
                    i += 6;
                }
            }
            "q" => {
                while i + 3 < tokens.len() && is_num(&tokens[i]) {
                    let x1 = cx + tokens[i].parse::<f64>().unwrap_or(0.0);
                    let y1 = cy + tokens[i + 1].parse::<f64>().unwrap_or(0.0);
                    let x = cx + tokens[i + 2].parse::<f64>().unwrap_or(0.0);
                    let y = cy + tokens[i + 3].parse::<f64>().unwrap_or(0.0);
                    ctx.quadratic_curve_to(x1, y1, x, y);
                    cx = x; cy = y;
                    i += 4;
                }
            }
            "h" => {
                while i < tokens.len() && is_num(&tokens[i]) {
                    let x: f64 = cx + tokens[i].parse::<f64>().unwrap_or(0.0);
                    ctx.line_to(x, cy);
                    cx = x;
                    i += 1;
                }
            }
            "v" => {
                while i < tokens.len() && is_num(&tokens[i]) {
                    let y: f64 = cy + tokens[i].parse::<f64>().unwrap_or(0.0);
                    ctx.line_to(cx, y);
                    cy = y;
                    i += 1;
                }
            }
            "H" => {
                while i < tokens.len() && is_num(&tokens[i]) {
                    let x: f64 = tokens[i].parse().unwrap_or(cx);
                    ctx.line_to(x, cy);
                    cx = x;
                    i += 1;
                }
            }
            "V" => {
                while i < tokens.len() && is_num(&tokens[i]) {
                    let y: f64 = tokens[i].parse().unwrap_or(cy);
                    ctx.line_to(cx, y);
                    cy = y;
                    i += 1;
                }
            }
            _ => {
                // 未知命令, 跳过
            }
        }
    }

    Ok(())
}

/// 将路径数据字符串拆分为 token 列表 (命令字母和数字分开)
fn tokenize_path(data: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in data.chars() {
        if ch.is_whitespace() || ch == ',' {
            // 分隔符
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else if ch.is_ascii_alphabetic() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push(ch.to_string());
        } else if ch == '-' && !current.is_empty() {
            // 负号 → 新数字的开始 (但要是前一个 token 是数字才行)
            tokens.push(std::mem::take(&mut current));
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// 判断 token 是否为数字
fn is_num(s: &str) -> bool {
    s.parse::<f64>().is_ok()
}

/// 解码 PNG 为 RGBA 像素数据 (用于印章渲染)
fn decode_png_to_rgba(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.len() < 8 || &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }

    let mut pos = 8;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut bit_depth = 0u8;
    let mut color_type = 0u8;
    let mut idat_data: Vec<u8> = Vec::new();

    while pos < data.len() {
        if pos + 8 > data.len() { break; }
        let length = u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
        let chunk_type = &data[pos+4..pos+8];
        let chunk_data_start = pos + 8;

        if chunk_data_start + length > data.len() { break; }

        match chunk_type {
            b"IHDR" => {
                width = u32::from_be_bytes([data[chunk_data_start], data[chunk_data_start+1], data[chunk_data_start+2], data[chunk_data_start+3]]);
                height = u32::from_be_bytes([data[chunk_data_start+4], data[chunk_data_start+5], data[chunk_data_start+6], data[chunk_data_start+7]]);
                bit_depth = data[chunk_data_start+8];
                color_type = data[chunk_data_start+9];
            }
            b"IDAT" => {
                idat_data.extend_from_slice(&data[chunk_data_start..chunk_data_start+length]);
            }
            b"IEND" => break,
            _ => {}
        }

        pos = chunk_data_start + length + 4; // skip CRC
    }

    if width == 0 || height == 0 || idat_data.is_empty() || bit_depth != 8 {
        return None;
    }

    // 解压 IDAT
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut decoder = ZlibDecoder::new(&idat_data[..]);
    let mut raw = Vec::new();
    decoder.read_to_end(&mut raw).ok()?;

    let bytes_per_pixel = match color_type {
        2 => 3, // RGB
        6 => 4, // RGBA
        0 => 1, // Grayscale
        4 => 2, // Grayscale + Alpha
        _ => return None,
    };

    let stride = width as usize * bytes_per_pixel;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    let mut raw_pos = 0;

    for _y in 0..height as usize {
        if raw_pos >= raw.len() { break; }
        raw_pos += 1; // skip filter byte (忽略 PNG 滤镜, 简化处理)
        let row_start = raw_pos;
        for _x in 0..width as usize {
            let px = row_start + _x * bytes_per_pixel;
            if px + bytes_per_pixel > raw.len() { break; }
            match color_type {
                2 => { // RGB → RGBA
                    rgba.extend_from_slice(&[raw[px], raw[px+1], raw[px+2], 255]);
                }
                6 => { // RGBA
                    rgba.extend_from_slice(&raw[px..px+4]);
                }
                0 => { // Gray → RGBA
                    let g = raw[px];
                    rgba.extend_from_slice(&[g, g, g, 255]);
                }
                4 => { // Gray+Alpha → RGBA
                    let g = raw[px];
                    rgba.extend_from_slice(&[g, g, g, raw[px+1]]);
                }
                _ => {}
            }
        }
        raw_pos = row_start + stride;
    }

    Some((width, height, rgba))
}

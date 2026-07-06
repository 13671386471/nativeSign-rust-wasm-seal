//! 文档渲染模块 — 基于 PDFium 的真实 PDF 渲染 + Canvas 2D
//!
//! 替代原 Qt WASM 渲染管线:
//!   - PDF 文档: 使用 pdfium-render 在 WASM 内部渲染为位图,绘制到 Canvas
//!   - 全部页面一次性渲染, 垂直堆叠在滚动容器中, 实现丝滑连续滚动
//!   - OFD 文档: 使用 Canvas 2D 占位渲染(待实现完整 OFD 解析)
//!   - 印章叠加: Canvas 覆盖层绘制

use wasm_bindgen::prelude::*;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};
use pdfium_render::prelude::*;
use crate::types::*;

// ============================================================
// PDFium 全局单例 — 只初始化一次,避免重复绑定报错
// ============================================================

static mut PDFIUM: Option<Pdfium> = None;
static PDFIUM_INIT: std::sync::Once = std::sync::Once::new();

fn get_pdfium() -> Result<&'static Pdfium, JsValue> {
    unsafe {
        PDFIUM_INIT.call_once(|| {
            PDFIUM = Some(Pdfium::default());
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
        let pdfium = get_pdfium()?;

        // 只加载一次 PDF
        let document = pdfium
            .load_pdf_from_byte_vec(doc_state.raw_data.clone(), None)
            .map_err(|e| JsValue::from_str(&format!("PDF 加载失败: {}", e)))?;

        let page_count = document.pages().len() as u32;
        if page_count == 0 {
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

        // 预计算每页的渲染尺寸
        let mut page_sizes: Vec<(f64, f64, i32, i32)> = Vec::new();
        for i in 0..page_count {
            if let Ok(page) = document.pages().get(i as i32) {
                let pw = page.width().value as f64;
                let ph = page.height().value as f64;
                let (tw, th) = self.calc_render_size(pw, ph, container_w, container_h_for_fit);
                page_sizes.push((pw, ph, tw, th));
            }
        }

        // 逐页渲染
        for i in 0..page_count {
            let (_pw, _ph, target_w, target_h) = page_sizes[i as usize];
            let canvas = self.create_page_canvas(i, target_w, target_h, page_count)?;

            let page = document.pages().get(i as i32)
                .map_err(|e| JsValue::from_str(&format!("页面 {} 不存在: {}", i, e)))?;

            self.render_page_to_canvas(&page, &canvas, target_w, target_h, i, doc_state)?;
        }

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
        let render_cfg = PdfRenderConfig::new()
            .set_target_width(target_w)
            .set_maximum_height(target_h * 2)
            .render_form_data(true);

        let bitmap = page
            .render_with_config(&render_cfg)
            .map_err(|e| JsValue::from_str(&format!("页面 {} 渲染失败: {:?}", page_idx, e)))?;

        let image_data: ImageData = bitmap
            .as_image_data()
            .map_err(|e| JsValue::from_str(&format!("ImageData 转换失败: {:?}", e)))?;

        let ctx = canvas
            .get_context("2d")
            .map_err(|_| JsValue::from_str("无法获取 Canvas 2D 上下文"))?
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsValue::from_str("无法转换为 CanvasRenderingContext2d"))?;

        // 白色背景
        ctx.set_fill_style_str("#FFFFFF");
        ctx.fill_rect(0.0, 0.0, target_w as f64, target_h as f64);

        // 绘制页面图像
        ctx.put_image_data(&image_data, 0.0, 0.0)
            .map_err(|_| JsValue::from_str("ImageData 绑定到 Canvas 失败"))?;

        // 印章叠加
        let img_w = image_data.width() as f64;
        let img_h = image_data.height() as f64;
        for seal in &doc_state.seals {
            if seal.page_index == page_idx {
                self.render_seal(&ctx, seal, 0.0, 0.0, img_w, img_h)?;
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
    // OFD 全部页面渲染
    // ============================================================

    fn render_all_ofd_pages(&self, doc_state: &DocState) -> Result<(), JsValue> {
        let page_count = doc_state.page_count;
        if page_count == 0 {
            return Ok(());
        }

        let window = web_sys::window().ok_or(JsValue::from_str("无 window"))?;
        let document_js = window.document().ok_or(JsValue::from_str("无 document"))?;
        let container = document_js
            .get_element_by_id(&self.container_id)
            .ok_or_else(|| JsValue::from_str("找不到容器"))?;
        let _container_w = container.client_width() as f64;

        for i in 0..page_count {
            let page_w = (595.0 * self.config.zoom) as i32;
            let page_h = (842.0 * self.config.zoom) as i32;
            let canvas = self.create_page_canvas(i, page_w, page_h, page_count)?;
            self.render_ofd_page_to_canvas(&canvas, i, doc_state)?;
        }

        Ok(())
    }

    fn render_ofd_page_to_canvas(
        &self,
        canvas: &HtmlCanvasElement,
        page_idx: u32,
        doc_state: &DocState,
    ) -> Result<(), JsValue> {
        let ctx = canvas
            .get_context("2d")
            .map_err(|_| JsValue::from_str("无法获取 Canvas 2D 上下文"))?
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| JsValue::from_str("无法转换为 CanvasRenderingContext2d"))?;

        let width = canvas.width() as f64;
        let height = canvas.height() as f64;

        let page_w = 595.0 * self.config.zoom;
        let page_h = 842.0 * self.config.zoom;
        let x = (width - page_w) / 2.0;
        let y = ((height - page_h) / 2.0).max(0.0);

        ctx.clear_rect(0.0, 0.0, width, height);
        ctx.set_fill_style_str("#FFFFFF");
        ctx.fill_rect(x, y, page_w, page_h);

        ctx.set_stroke_style_str("#CCCCCC");
        ctx.set_line_width(1.0);
        ctx.stroke_rect(x, y, page_w, page_h);

        ctx.set_fill_style_str("#999999");
        ctx.set_font("14px sans-serif");
        ctx.set_text_align("center");
        ctx.fill_text(
            &format!("OFD 第 {} 页 — 完整解析引擎待实现", page_idx + 1),
            x + page_w / 2.0,
            y + page_h / 2.0,
        ).ok();

        for seal in &doc_state.seals {
            if seal.page_index == page_idx {
                self.render_seal(&ctx, seal, x, y, page_w, page_h)?;
            }
        }

        Ok(())
    }

    // ============================================================
    // 印章渲染
    // ============================================================

    fn render_seal(
        &self,
        ctx: &CanvasRenderingContext2d,
        seal: &PlacedSeal,
        page_x: f64,
        page_y: f64,
        page_w: f64,
        page_h: f64,
    ) -> Result<(), JsValue> {
        let sx = page_x + (seal.x / page_w) * page_w;
        let sy = page_y + (seal.y / page_h) * page_h;
        let sw = seal.width * self.config.zoom;
        let sh = seal.height * self.config.zoom;

        let cx = sx + sw / 2.0;
        let cy = sy + sh / 2.0;
        let radius = sw.min(sh) / 2.0;

        ctx.begin_path();
        ctx.arc(cx, cy, radius, 0.0, std::f64::consts::PI * 2.0)
            .map_err(|_| JsValue::from_str("绘制印章失败"))?;

        ctx.set_stroke_style_str("#D81E06");
        ctx.set_line_width(2.0);
        ctx.stroke();

        ctx.set_font("10px sans-serif");
        ctx.set_fill_style_str("#D81E06");
        ctx.set_text_align("center");
        ctx.fill_text(&seal.seal_info.seal_name, cx, cy).ok();

        if seal.signed {
            ctx.set_fill_style_str("#52C41A");
            ctx.set_font("16px sans-serif");
            ctx.fill_text("✓", cx + radius * 0.7, cy - radius * 0.7).ok();
        }

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

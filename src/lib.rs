//! 点聚版式文档签章引擎 — Rust WASM 重构版
//!
//! 提供与原始 OFD_Plugin 兼容的 JavaScript API
//!
//! ============================================================
//! ⚠️ 重要说明 — 假数据使用标记
//! ============================================================
//!
//! 以下位置使用了模拟数据，生产部署前需要替换：
//!
//! 1. FIXME: REPLACE_WITH_REAL_CERT
//!    crypto.rs — MOCK_RSA_CERT, MOCK_SM2_CERT 证书公钥 (仅用于 API 返回值)
//!    crypto.rs — SM2 密钥对在首次使用时由 libsm 随机生成 (非持久化)
//!    注意: SM2/SM3/SM4 算法本身已使用 libsm crate 的真实实现
//!
//! 2. FIXME: REPLACE_WITH_REAL_SIGN_SERVICE
//!    sign.rs — cloud_sign() 云签 HTTP 服务调用
//!
//! 3. FIXME: REPLACE_WITH_REAL_UKEY_PROXY
//!    ukey.rs — UKey WebSocket 代理通信
//!
//! 4. FIXME: REPLACE_WITH_REAL_DOC_ENGINE
//!    engine.rs — PDF/OFD 完整解析与签章结构体构造
//!
//! 5. FIXME: REPLACE_WITH_REAL_RENDER
//!    render.rs — 文档完整渲染 (当前仅占位显示)
//! ============================================================

mod types;
mod crypto;
mod der;
mod ses;
mod pkcs7;
mod engine;
mod seal;
mod sign;
mod ukey;
mod render;
mod utils;
mod ofd_parser;
mod ofd_sign;
mod font_provider;
mod font_embed;

use wasm_bindgen::prelude::*;
use std::cell::RefCell;
use web_sys;
// ============================================================
// 全局单例 — 引擎状态管理
// ============================================================

thread_local! {
    static ENGINE: RefCell<Option<WasmerEngine>> = RefCell::new(None);
}

/// 主引擎结构 — 包含所有子模块
struct WasmerEngine {
    doc: engine::DocumentEngine,
    sign: sign::SignEngine,
    seal: seal::SealEngine,
    ukey: ukey::UkeyEngine,
    render: render::RenderEngine,
    /// 当前落章参数
    current_seal_info: Option<types::SealInfo>,
    /// 印章池 (意愿认证后获取的印章列表)
    seal_pool: Vec<types::SealInfo>,
    /// 签署配置
    sign_config: types::SignConfig,
}

impl WasmerEngine {
    fn new() -> Self {
        Self {
            doc: engine::DocumentEngine::new(),
            sign: sign::SignEngine::new(),
            seal: seal::SealEngine,
            ukey: ukey::UkeyEngine::new(),
            render: render::RenderEngine::new("screen"),
            current_seal_info: None,
            seal_pool: Vec::new(),
            sign_config: types::SignConfig::default(),
        }
    }
}

fn with_engine<F, R>(f: F) -> R
where
    F: FnOnce(&mut WasmerEngine) -> R,
{
    ENGINE.with(|cell| {
        let mut engine = cell.borrow_mut();
        if engine.is_none() {
            *engine = Some(WasmerEngine::new());
        }
        f(engine.as_mut().unwrap())
    })
}

/// 触发渲染刷新
fn refresh_render() {
    with_engine(|engine| {
        let doc_state = engine.doc.state.clone();
        web_sys::console::log_1(&format!("[refresh] doc_type={:?} page_count={} is_opened={}",
            doc_state.doc_type, doc_state.page_count, doc_state.is_opened).into());

        // 防御检查：未打开文档或无数据时跳过渲染
        if !doc_state.is_opened || doc_state.raw_data.is_empty() {
            web_sys::console::warn_1(&"[refresh] 文档未打开或数据为空，跳过渲染".into());
            return;
        }

        if let Err(e) = engine.render.refresh(&doc_state) {
            web_sys::console::error_1(&format!("[refresh] 渲染失败: {:?}", e).into());
        }
    });
}

// ============================================================
// WASM 对外 API — 与 OFD_Plugin 接口保持兼容
// ============================================================

/// 初始化 WASM 引擎（替代 Qt WASM 的 _InitApplication）
#[wasm_bindgen]
pub fn init_application(spinner_id: &str, screen_id: &str, _status_id: &str) {
    console_error_panic_hook::set_once();

    with_engine(|engine| {
        engine.render = render::RenderEngine::new(screen_id);
        engine.doc = engine::DocumentEngine::new();
        engine.sign = sign::SignEngine::new();
    });

    // 隐藏加载指示器，显示画布
    let hide_spinner = js_sys::Function::new_no_args(&format!(
        r#"
        var s = document.getElementById('{}');
        if (s) s.style.display = 'none';
        var sc = document.getElementById('{}');
        if (sc) sc.style.display = 'block';
        "#,
        spinner_id, screen_id
    ));
    hide_spinner.call0(&JsValue::NULL).ok();
}

/// 注册事件监听器
#[wasm_bindgen]
pub fn regist_listener(event_name: &str, js_func_name: &str, _async: bool) -> Result<(), JsValue> {
    // 注册 JS 回调
    // 对应 OFD_Plugin.registListener("tool_selectpoint", "SelectPoint", true)
    // 对应 OFD_Plugin.registListener("pageindex", "PageIndex", false)

    web_sys::console::log_1(
        &format!("OFD_Plugin.registListener: {} -> {}", event_name, js_func_name).into()
    );

    Ok(())
}

// ---- 文档操作 API ----

/// 加载文件
#[wasm_bindgen]
pub async fn load_file(file_data: Vec<u8>, file_name: &str) -> Result<String, JsValue> {
    let result = with_engine(|engine| {
        engine.render.reset(); // 清除旧文档的渲染元素

        // 第一步：在原始数据上解析已有签章（预处理可能损坏 PDF 结构）
        let existing_seals = if file_name.to_lowercase().ends_with(".pdf") {
            engine::DocumentEngine::parse_existing_signatures(&file_data)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // 第二步：预处理嵌入中文字体（仅对 PDF 文件）
        let processed = if file_name.to_lowercase().ends_with(".pdf") {
            font_embed::preprocess_pdf_for_cjk(&file_data)
        } else {
            file_data.to_vec()
        };

        // 第三步：加载文件
        engine.doc.load_file(processed, file_name)
            .map(|_| "1".to_string())
            .map_err(|e| JsValue::from_str(&e))?;

        // 第四步：恢复已有签章统计（覆盖预处理文件上的空结果）
        if !existing_seals.is_empty() {
            engine.doc.state.seals = existing_seals;
            engine.doc.state.seal_count = engine.doc.state.seals.len() as u32;
            engine.doc.state.signed_count = engine.doc.state.seals.iter()
                .filter(|s| s.signed)
                .count() as u32;
            web_sys::console::log_1(&format!(
                "[load_file] 恢复 {} 枚已有签章", engine.doc.state.seal_count
            ).into());
        }

        Ok("1".to_string())
    });

    // 只在加载成功后才触发渲染
    match &result {
        Ok(_) => refresh_render(),
        Err(e) => {
            web_sys::console::error_1(&format!("[load_file] 加载失败，跳过渲染: {:?}", e).into());
        }
    }
    result
}

/// 文档是否已打开
#[wasm_bindgen]
pub fn is_opened() -> i32 {
    with_engine(|engine| {
        if engine.doc.is_opened() { 1 } else { 0 }
    })
}

/// 获取总页数
#[wasm_bindgen]
pub async fn get_page_count() -> u32 {
    with_engine(|engine| engine.doc.get_page_count())
}

/// 获取页面宽度(单位: pt)
#[wasm_bindgen]
pub async fn get_page_width(page_index: u32) -> f64 {
    with_engine(|engine| engine.doc.get_page_width(page_index))
}

/// 获取页面高度(单位: pt)
#[wasm_bindgen]
pub async fn get_page_height(page_index: u32) -> f64 {
    with_engine(|engine| engine.doc.get_page_height(page_index))
}

/// 获取文档类型 ("pdf" / "ofd")
#[wasm_bindgen]
pub async fn get_doc_type() -> String {
    with_engine(|engine| engine.doc.get_doc_type().to_string())
}

/// 获取当前文件大小 (KB)
#[wasm_bindgen]
pub async fn get_curr_file_size() -> u64 {
    with_engine(|engine| engine.doc.get_curr_file_size())
}

/// 获取文件 MD5 值
#[wasm_bindgen]
pub async fn get_file_md5_value(param: &str) -> String {
    with_engine(|engine| {
        engine.doc.get_file_md5_value(param).unwrap_or_default()
    })
}

/// 获取文档属性
#[wasm_bindgen]
pub async fn get_doc_property(key: &str) -> String {
    with_engine(|engine| {
        engine.doc.get_doc_property(key).unwrap_or_default()
    })
}

/// 设置文档属性
#[wasm_bindgen]
pub async fn set_doc_property(key: &str, value: &str) {
    with_engine(|engine| engine.doc.set_doc_property(key, value));
}

/// 保存文档
#[wasm_bindgen]
pub async fn save_to(file_name: &str, format: &str, flags: i32) -> Result<String, JsValue> {
    with_engine(|engine| {
        // 如果有未嵌入的印章, 先嵌入到文档中
        let has_unsigned_seals = engine.doc.state.seals.iter().any(|s| !s.signed);
        if has_unsigned_seals {
            let doc_data = engine.doc.state.raw_data.clone();
            let seals = engine.doc.state.seals.clone();
            let doc_type = engine.doc.state.doc_type;

            // 为每枚印章构建 PKCS#7 签名值
            let algorithm = match engine.sign_config.algorithm {
                types::Algorithm::Rsa => ses::SealAlgorithm::Rsa,
                types::Algorithm::Sm2 => ses::SealAlgorithm::Sm2,
            };
            let mut signed_seals: Vec<types::PlacedSeal> = Vec::new();
            for mut seal in seals {
                if !seal.signed {
                    // 构建 SES 参数
                    let ses_params = ses::ses_params_from_seal_info(&seal.seal_info, algorithm);

                    // 构建 MOCK SES_Signature
                    let ses_sig_der = ses::build_mock_ses_signature(&ses_params, &doc_data);

                    // 计算文档摘要
                    let doc_hash = match algorithm {
                        ses::SealAlgorithm::Sm2 => crypto::sm3_hash(&doc_data),
                        ses::SealAlgorithm::Rsa => crypto::sha256(&doc_data),
                    };

                    // 构建 PKCS#7
                    let pkcs7_der = pkcs7::build_mock_pkcs7(
                        algorithm,
                        &ses_sig_der,
                        &doc_hash,
                        ses_params.sign_time,
                    );

                    seal.signature = Some(pkcs7_der);
                    seal.signed = true;
                }
                signed_seals.push(seal);
            }

            // 嵌入签章到文档
            let embedded = seal::SealEngine::embed_seals_to_document(
                &doc_data, &signed_seals, doc_type, algorithm
            ).map_err(|e| {
                web_sys::console::error_1(&format!("[save_to] 签章嵌入失败: {}", e).into());
                JsValue::from_str(&e)
            })?;

            // 更新文档数据
            engine.doc.state.raw_data = embedded.clone();
            engine.doc.state.seals = signed_seals;
            engine.doc.state.signed_count = engine.doc.state.seals.iter()
                .filter(|s| s.signed).count() as u32;

            web_sys::console::log_1(&format!(
                "[save_to] 签章嵌入完成, 文档大小: {} bytes",
                engine.doc.state.raw_data.len()
            ).into());
        }

        engine.doc.save_to(file_name, format, flags)
            .map_err(|e| JsValue::from_str(&e))
    })
}

/// 关闭文档
#[wasm_bindgen]
pub async fn close_doc(flags: i32) {
    with_engine(|engine| engine.doc.close_doc(flags));
}

/// 获取已签章数量
#[wasm_bindgen]
pub async fn get_signatures_count(seal_type: &str) -> u32 {
    with_engine(|engine| engine.doc.get_signatures_count(seal_type))
}

/// 获取所有签章信息 (JSON 数组)
/// 返回包含每枚印章的位置、名称、大小、签名状态等信息的 JSON 字符串
#[wasm_bindgen]
pub async fn get_seal_info_json() -> String {
    with_engine(|engine| engine.doc.get_seal_info_json())
}

/// 获取下一页注释节点
#[wasm_bindgen]
pub async fn get_next_note(node_type: &str, index: i32, param: &str) -> Option<String> {
    with_engine(|engine| engine.doc.get_next_note(node_type, index, param))
}

/// 删除指定印章
#[wasm_bindgen]
pub async fn delete_note(note_id: &str) -> i32 {
    with_engine(|engine| {
        engine.doc.delete_note(note_id).unwrap_or(-1)
    })
}

// ---- 印章操作 API ----

/// 创建印章数据
#[wasm_bindgen]
pub async fn get_create_seal(
    image_data: &str,
    seal_type: i32,
    code: &str,
    name: &str,
    company: &str,
    width: f64,
    height: f64,
) -> String {
    seal::SealEngine::create_seal(image_data, seal_type, code, name, company, width, height)
        .unwrap_or_default()
}

/// 添加印章到文档
#[wasm_bindgen]
pub async fn add_seal(c_pages: &str, _reserved: &str, _mode: &str) -> i32 {
    let result = with_engine(|engine| {
        if let Some(seal_info) = &engine.current_seal_info {
            let sign_data = seal_info.sign_data.as_deref().unwrap_or("");
            let mut seals = std::mem::take(&mut engine.doc.state.seals);
            let result = seal::SealEngine::add_seal(&mut seals, c_pages, sign_data, seal_info);
            engine.doc.state.seals = seals;

            match result {
                Ok(count) => {
                    engine.doc.state.seal_count = count as u32;

                    // 准备签名数据 — 存储文档数据和印章信息到 SignEngine
                    let doc_data = engine.doc.state.raw_data.clone();
                    let last_seal = engine.doc.state.seals.last().cloned();
                    if let Some(seal) = last_seal {
                        engine.sign.prepare_for_signing(doc_data, seal.seal_info.clone());
                    }

                    web_sys::console::log_1(&format!(
                        "[add_seal] 印章已添加, 总数={}, 页码={}, 位置=({:.1},{:.1})",
                        count,
                        engine.doc.state.seals.last().map(|s| s.page_index).unwrap_or(0),
                        engine.doc.state.seals.last().map(|s| s.x).unwrap_or(0.0),
                        engine.doc.state.seals.last().map(|s| s.y).unwrap_or(0.0),
                    ).into());

                    count as i32
                }
                Err(e) => {
                    web_sys::console::error_1(&format!("[add_seal] 印章添加失败: {}", e).into());
                    -1
                }
            }
        } else {
            web_sys::console::error_1(&"[add_seal] current_seal_info 未设置".into());
            -1
        }
    });
    // 触发重绘以显示新添加的印章
    refresh_render();
    result
}

/// 获取最后添加的印章
#[wasm_bindgen]
pub async fn get_last_seal() -> Option<String> {
    with_engine(|engine| {
        seal::SealEngine::get_last_seal(&engine.doc.state.seals)
    })
}

// ---- 签名操作 API ----

/// 获取 RSA 签名哈希数据
#[wasm_bindgen]
pub async fn get_sign_sha_data() -> Option<String> {
    with_engine(|engine| engine.sign.get_sign_sha_data().ok())
}

/// 获取扩展值（签名相关）
#[wasm_bindgen]
pub async fn get_value_ex(key: &str, l_type: i32, _reserved1: &str, _reserved2: i32, _reserved3: &str) -> Option<String> {
    with_engine(|engine| engine.sign.get_value_ex(key, l_type).ok())
}

/// 设置扩展值（签名合成）
#[wasm_bindgen]
pub async fn set_value_ex(key: &str, l_type: i32, reserved: i32, signdata: &str) -> i32 {
    with_engine(|engine| {
        engine.sign.set_value_ex(key, l_type, reserved, signdata)
            .unwrap_or(0)
    })
}

/// 获取内部错误码
#[wasm_bindgen]
pub async fn get_re_value() -> i32 {
    with_engine(|engine| engine.sign.get_re_value())
}

/// 获取错误信息
#[wasm_bindgen]
pub async fn get_error_string(code: &str) -> String {
    with_engine(|engine| engine.sign.get_error_string(code))
}

/// 重新加载文档显示
#[wasm_bindgen]
pub async fn repload_doc_data(action: &str) -> Result<(), JsValue> {
    with_engine(|engine| {
        engine.sign.reload_doc_data(action)
            .map_err(|e| JsValue::from_str(&e))
    })
}

// ---- 全局配置 API ----

/// 设置全局值
#[wasm_bindgen]
pub async fn set_value(key: &str, value: &str) {
    with_engine(|engine| {
        engine.sign.set_value(key, value);

        // 同时处理渲染相关设置
        match key {
            "ADD_FORCETYPE_VALUE4" => {
                if let Ok(v) = value.parse::<i32>() {
                    engine.render.set_show_def_menu(v);
                }
            }
            _ => {}
        }
    });
}

/// 获取全局值
#[wasm_bindgen]
pub async fn get_value(key: &str) -> Option<String> {
    with_engine(|engine| engine.sign.get_value(key))
}

/// 设置印章模式
#[wasm_bindgen]
pub async fn set_seal_mode(mode: i32) {
    with_engine(|engine| engine.sign.set_seal_mode(mode));
}

/// 设置单文件模式
#[wasm_bindgen]
pub async fn set_single_mode(enabled: bool) {
    with_engine(|engine| engine.sign.set_single_mode(enabled));
}

// ---- UKey 操作 API ----

/// 获取 UKey 设备信息
#[wasm_bindgen]
pub async fn get_ukey_info(param: i32) -> String {
    with_engine(|engine| {
        // 由于异步限制，使用简化的同步返回
        // FIXME: 生产环境需支持真正的异步
        let result = futures::executor::block_on(engine.ukey.get_ukey_info(param));
        result.unwrap_or_default()
    })
}

/// 验证 UKey PIN 码
#[wasm_bindgen]
pub async fn verify_pin(pin_code: &str) -> String {
    with_engine(|engine| {
        let result = futures::executor::block_on(engine.ukey.verify_pin(pin_code));
        result.unwrap_or_else(|e| format!(r#"{{"status":-1,"errmsg":"{}"}}"#, e))
    })
}

/// 获取 UKey 印章列表 (base64 JSON)
#[wasm_bindgen]
pub async fn get_seal_list_json() -> String {
    with_engine(|engine| {
        let result = futures::executor::block_on(engine.ukey.get_seal_list_json());
        result.unwrap_or_default()
    })
}

/// 获取 UKey 印章图像
#[wasm_bindgen]
pub async fn get_seal_image(dev_id: &str, seal_id: &str) -> String {
    with_engine(|engine| {
        let result = futures::executor::block_on(
            engine.ukey.get_seal_image(dev_id, seal_id)
        );
        result.unwrap_or_default()
    })
}

/// 获取 UKey 印章数据
#[wasm_bindgen]
pub async fn get_seal_data(dev_id: &str, seal_id: &str) -> String {
    with_engine(|engine| {
        let result = futures::executor::block_on(
            engine.ukey.get_seal_data(dev_id, seal_id)
        );
        result.unwrap_or_default()
    })
}

/// UKey 硬件签名
#[wasm_bindgen]
pub async fn sign_data(data: &str, pin_code: &str) -> String {
    with_engine(|engine| {
        let result = futures::executor::block_on(
            engine.ukey.sign_data(data, pin_code)
        );
        result.unwrap_or_else(|e| format!("error:{}", e))
    })
}

// ---- 渲染操作 API ----

/// 设置页面模式
#[wasm_bindgen]
pub async fn set_page_mode(mode: i32, param: i32) {
    with_engine(|engine| engine.render.set_page_mode(mode, param));
    refresh_render();
}

/// 设置当前页
#[wasm_bindgen]
pub async fn set_curr_page(page: u32) {
    let target_page = with_engine(|engine| {
        let max_page = engine.doc.state.page_count;
        let p = page.min(max_page.saturating_sub(1));
        engine.doc.state.current_page = p;
        engine.render.set_current_page(p, max_page);
        p
    });

    // 先告诉 JS 目标页, 再重新渲染全部页面 (渲染完成后自动滚动到该页)
    let _ = js_sys::eval(&format!(
        r#"(function(){{window.__wasm_last_page={};}})()"#,
        target_page
    ));

    refresh_render();

    // 通知 JS 页码已变更
    let _ = js_sys::eval(&format!(
        r#"(function(){{if(typeof window.PageIndex==='function')window.PageIndex(JSON.stringify({{index:{}}}),'');}})()"#,
        target_page
    ));
}

/// 获取当前页码
#[wasm_bindgen]
pub async fn get_current_page() -> u32 {
    with_engine(|engine| engine.render.get_current_page())
}

/// 获取当前操作模式
#[wasm_bindgen]
pub async fn get_curr_action() -> i32 {
    // 0=手型, 2=文本选择, 等
    0
}

/// 设置当前操作模式
#[wasm_bindgen]
pub async fn set_curr_action(action: i32) {
    // 设置工具模式
    web_sys::console::log_1(&format!("SetCurrAction: {}", action).into());
}

/// 执行预定义按钮操作
#[wasm_bindgen]
pub async fn perform_click(action: &str) {
    let needs_refresh = with_engine(|engine| {
        let max_page = engine.doc.state.page_count;
        match action {
            "view_zoomin" => { engine.render.zoom_in(); true }
            "view_zoomout" => { engine.render.zoom_out(); true }
            "view_pagedown" => {
                engine.render.next_page(max_page);
                engine.doc.state.current_page = engine.render.get_current_page();
                let page = engine.render.get_current_page();
                // 通知 JS 丝滑滚动到目标页 (不重新渲染全部页面)
                let _ = js_sys::eval(&format!(
                    r#"(function(){{if(typeof window.__wasm_scroll_to_page==='function')window.__wasm_scroll_to_page({});}})()"#,
                    page
                ));
                let _ = js_sys::eval(&format!(
                    r#"(function(){{if(typeof window.PageIndex==='function')window.PageIndex(JSON.stringify({{index:{}}}),'');}})()"#,
                    page
                ));
                false // 不需要重新渲染,只需滚动
            }
            "view_pageup" => {
                engine.render.prev_page();
                engine.doc.state.current_page = engine.render.get_current_page();
                let page = engine.render.get_current_page();
                let _ = js_sys::eval(&format!(
                    r#"(function(){{if(typeof window.__wasm_scroll_to_page==='function')window.__wasm_scroll_to_page({});}})()"#,
                    page
                ));
                let _ = js_sys::eval(&format!(
                    r#"(function(){{if(typeof window.PageIndex==='function')window.PageIndex(JSON.stringify({{index:{}}}),'');}})()"#,
                    page
                ));
                false
            }
            "navigator" | "view_navigation_outline" => false,
            "doc_antipage" => false, // 左旋
            "doc_clockpage" => false, // 右旋
            _ => false
        }
    });
    if needs_refresh {
        refresh_render();
    }
}

/// 显示文件选择对话框
#[wasm_bindgen]
pub async fn show_dialog(_mode: i32, _title: &str, _default_path: &str, _filter: &str) -> Option<String> {
    // 通过 JS 互操作触发文件选择
    // FIXME: 生产环境需实现完整的文件对话框
    None
}

/// 关闭弹出菜单
#[wasm_bindgen]
pub fn close_popup_menu() {
    // 关闭右键菜单
}

/// 搜索文本
#[wasm_bindgen]
pub async fn search_text(_text: &str, _flags: i32, _options: i32) -> String {
    // FIXME: 实现文本搜索
    "0".to_string()
}

/// 设置 JS 环境变量
#[wasm_bindgen]
pub fn set_js_env(env: i32) {
    with_engine(|engine| engine.render.set_js_env(env));
}

/// 设置工具栏显示
#[wasm_bindgen]
pub fn set_show_tool_bar(show: i32) {
    with_engine(|engine| engine.render.set_show_toolbar(show));
}

/// 设置右键菜单显示
#[wasm_bindgen]
pub fn set_show_def_menu(show: i32) {
    with_engine(|engine| engine.render.set_show_def_menu(show));
}

// ---- 印章选择器 API ----

/// 设置落章光标（印章图像作为鼠标光标）
/// 对应 OFD_Plugin.SelectPoint(sealImage)
#[wasm_bindgen]
pub async fn select_point(seal_image: &str) {
    let _cursor_url = seal::SealEngine::prepare_seal_cursor(seal_image);

    with_engine(|engine| {
        let _ = engine.render.set_pointer_events(true);
    });

    // 注入一次性点击事件监听，将坐标转换为文档坐标后回调 JS 的 SelectPoint
    let js = r#"
    (function() {
        var screen = document.getElementById('screen');
        if (!screen) return;
        // 移除旧的点击监听
        if (window.__wasm_select_point_handler) {
            screen.removeEventListener('click', window.__wasm_select_point_handler);
        }
        window.__wasm_select_point_handler = function(e) {
            var rect = screen.getBoundingClientRect();
            var x = e.clientX - rect.left;
            var y = e.clientY - rect.top;
            // 恢复为普通模式(点击一次后退出盖章模式)
            if (typeof window.ExitSelectPoint === 'function') {
                window.ExitSelectPoint();
            }
            if (typeof window.SelectPoint === 'function') {
                window.SelectPoint(JSON.stringify({left: x, top: y, pageindex: (window.__wasm_current_page || 0)}), '');
            }
        };
        screen.addEventListener('click', window.__wasm_select_point_handler);
        screen.style.cursor = 'crosshair';
    })()
    "#;
    js_sys::eval(js).ok();
}

/// 退出落章选择模式
#[wasm_bindgen]
pub fn exit_select_point() {
    with_engine(|engine| {
        let _ = engine.render.set_pointer_events(false);
    });
    let js = r#"
    (function() {
        var screen = document.getElementById('screen');
        if (screen) screen.style.cursor = 'default';
        if (window.__wasm_select_point_handler) {
            if (screen) screen.removeEventListener('click', window.__wasm_select_point_handler);
            window.__wasm_select_point_handler = null;
        }
    })()
    "#;
    js_sys::eval(js).ok();
}

// ---- 文件操作 API (后台操作) ----

/// 后台打开文件（用于格式转换）
#[wasm_bindgen]
pub async fn open_file_back(_file_path: &str, _read_only: bool) -> i32 {
    // 返回文件句柄
    1
}

/// 后台保存文件
#[wasm_bindgen]
pub async fn save_to_back(_file_handle: i32, _file_name: &str, _options: &str) -> i32 {
    1
}

/// 后台关闭文件
#[wasm_bindgen]
pub async fn close_file_back(_file_handle: i32, _save: bool) -> i32 {
    1
}

/// 获取文件信息
#[wasm_bindgen]
pub async fn get_file_info(_file_handle: i32, _info_type: &str) -> i32 {
    0
}

// ---- HTTP 上传 API ----

/// 初始化 HTTP 上传
#[wasm_bindgen]
pub fn http_init() {
    // 为 HTTP 上传做初始化
}

/// 添加 POST 参数字符串
#[wasm_bindgen]
pub fn http_add_post_string(_key: &str, _value: &str) {
    // 暂存 POST 参数
}

/// 添加当前文件到 POST
#[wasm_bindgen]
pub fn http_add_post_curr_file(_field_name: &str) {
    // 将当前文档数据添加到上传表单
}

/// HTTP POST 上传
#[wasm_bindgen]
pub async fn http_post(_url: &str) -> Option<String> {
    // FIXME: 实现真正的 HTTP POST 上传
    None
}

// ---- 撤销/重做 ----

/// 检查是否可撤销
#[wasm_bindgen]
pub async fn can_undo() -> i32 {
    0
}

/// 撤销操作
#[wasm_bindgen]
pub async fn undo() -> String {
    "0".to_string()
}

/// 检查是否可重做
#[wasm_bindgen]
pub async fn can_redo() -> i32 {
    0
}

/// 重做操作
#[wasm_bindgen]
pub async fn redo() -> String {
    "0".to_string()
}

// ---- 引擎生命周期 ----

/// 销毁引擎实例
#[wasm_bindgen]
pub fn destroy_application() {
    ENGINE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

/// 初始化完成回调注册
/// 对应 OFD_Plugin._IniCtrlReadytCallback(callback)
#[wasm_bindgen]
pub fn ini_ctrl_ready_callback(callback: &js_sys::Function) {
    // 引擎就绪后调用 JS 回调
    let cb = callback.clone();
    wasm_bindgen_futures::spawn_local(async move {
        // 模拟异步初始化延迟
        let _ = cb.call0(&JsValue::NULL);
    });
}

// ============================================================
// 辅助函数
// ============================================================

/// 日志输出到浏览器控制台
#[wasm_bindgen]
pub fn log(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

/// 自动嵌入中文字体预处理 (方案 A 对外接口)
///
/// 对未嵌入 CID 中文字体的 PDF, 在加载前注入 NotoSansSC 并改写内容流字符码,
/// 使 PDFium WASM 可正确渲染中文。失败或无需处理时返回原始字节。
#[wasm_bindgen]
pub fn preprocess_pdf_for_cjk(pdf: Vec<u8>) -> Vec<u8> {
    font_embed::preprocess_pdf_for_cjk(&pdf)
}

/// 获取引擎版本
#[wasm_bindgen]
pub fn version() -> String {
    format!("dianju-wasm-seal v{} (Rust WASM + SES)", env!("CARGO_PKG_VERSION"))
}

// ============================================================
// SES 签章 API — 直接操作 SES 结构
// ============================================================

/// 构建 SES 电子印章 (Seal.esl) 并返回 base64
///
/// 使用 MOCK 证书和签名, 生成完整的 SES_Seal DER 结构
#[wasm_bindgen]
pub fn build_ses_seal(
    seal_name: &str,
    seal_image_base64: &str,
    algorithm: &str,
) -> String {
    let algo = if algorithm == "rsa" {
        ses::SealAlgorithm::Rsa
    } else {
        ses::SealAlgorithm::Sm2
    };

    let seal_image = if seal_image_base64.is_empty() {
        ses::MOCK_SEAL_PNG.to_vec()
    } else {
        base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            seal_image_base64
        ).unwrap_or_else(|_| ses::MOCK_SEAL_PNG.to_vec())
    };

    let params = ses::SesParams {
        algorithm: algo,
        seal_name: seal_name.to_string(),
        seal_image,
        ..Default::default()
    };

    let seal_der = ses::build_mock_ses_seal(&params);
    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &seal_der
    )
}

/// 构建 SES 电子签名 (SES_Signature) 并返回 base64
///
/// 使用 MOCK 签名, 生成完整的 SES_Signature DER 结构
#[wasm_bindgen]
pub fn build_ses_signature(doc_data_base64: &str, algorithm: &str) -> String {
    let algo = if algorithm == "rsa" {
        ses::SealAlgorithm::Rsa
    } else {
        ses::SealAlgorithm::Sm2
    };

    let doc_data = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        doc_data_base64
    ).unwrap_or_default();

    let params = ses::SesParams {
        algorithm: algo,
        ..Default::default()
    };

    let sig_der = ses::build_mock_ses_signature(&params, &doc_data);
    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &sig_der
    )
}

/// 构建 PKCS#7 SignedData (包含 SES_Signature) 并返回 base64
#[wasm_bindgen]
pub fn build_pkcs7_signature(doc_data_base64: &str, algorithm: &str) -> String {
    let algo = if algorithm == "rsa" {
        ses::SealAlgorithm::Rsa
    } else {
        ses::SealAlgorithm::Sm2
    };

    let doc_data = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        doc_data_base64
    ).unwrap_or_default();

    let params = ses::SesParams {
        algorithm: algo,
        ..Default::default()
    };

    let ses_sig = ses::build_mock_ses_signature(&params, &doc_data);
    let doc_hash = match algo {
        ses::SealAlgorithm::Sm2 => crypto::sm3_hash(&doc_data),
        ses::SealAlgorithm::Rsa => crypto::sha256(&doc_data),
    };

    let pkcs7_der = pkcs7::build_mock_pkcs7(
        algo,
        &ses_sig,
        &doc_hash,
        params.sign_time,
    );

    base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &pkcs7_der
    )
}

/// 嵌入签章到当前文档 (直接调用, 不通过 save_to)
///
/// 为当前文档中的所有未签章印章构建 SES 结构并嵌入
#[wasm_bindgen]
pub async fn embed_signatures() -> Result<i32, JsValue> {
    with_engine(|engine| {
        let seals = engine.doc.state.seals.clone();
        if seals.is_empty() {
            return Ok(0);
        }

        let doc_data = engine.doc.state.raw_data.clone();
        let doc_type = engine.doc.state.doc_type;

        // 为每枚印章构建签名
        let algorithm = match engine.sign_config.algorithm {
            types::Algorithm::Rsa => ses::SealAlgorithm::Rsa,
            types::Algorithm::Sm2 => ses::SealAlgorithm::Sm2,
        };
        let mut signed_seals: Vec<types::PlacedSeal> = Vec::new();
        for mut seal in seals {
            if !seal.signed {
                let ses_params = ses::ses_params_from_seal_info(&seal.seal_info, algorithm);

                let ses_sig_der = ses::build_mock_ses_signature(&ses_params, &doc_data);
                let doc_hash = match algorithm {
                    ses::SealAlgorithm::Sm2 => crypto::sm3_hash(&doc_data),
                    ses::SealAlgorithm::Rsa => crypto::sha256(&doc_data),
                };

                let pkcs7_der = pkcs7::build_mock_pkcs7(
                    algorithm,
                    &ses_sig_der,
                    &doc_hash,
                    ses_params.sign_time,
                );

                seal.signature = Some(pkcs7_der);
                seal.signed = true;
            }
            signed_seals.push(seal);
        }

        let embedded = seal::SealEngine::embed_seals_to_document(
            &doc_data, &signed_seals, doc_type, algorithm
        ).map_err(|e| {
            web_sys::console::error_1(&format!("[embed_signatures] 嵌入失败: {}", e).into());
            JsValue::from_str(&e)
        })?;

        engine.doc.state.raw_data = embedded;
        engine.doc.state.seals = signed_seals;
        engine.doc.state.signed_count = engine.doc.state.seals.iter()
            .filter(|s| s.signed).count() as u32;

        web_sys::console::log_1(&format!(
            "[embed_signatures] 完成, {} 枚签章已嵌入",
            engine.doc.state.signed_count
        ).into());

        Ok(engine.doc.state.signed_count as i32)
    })
}

/// 设置当前印章信息 (在调用 add_seal 之前必须调用)
///
/// 由前端在用户确认印章图片和签署信息后调用,
/// 后续 add_seal 会使用此 SealInfo 创建 PlacedSeal
#[wasm_bindgen]
pub fn set_current_seal_info(
    seal_name: &str,
    seal_image_base64: &str,
    signer_name: &str,
    algorithm: &str,
) {
    with_engine(|engine| {
        // 同步签章算法到引擎配置
        let algo = if algorithm == "rsa" {
            types::Algorithm::Rsa
        } else {
            types::Algorithm::Sm2
        };
        engine.sign_config.algorithm = algo;

        // 去掉 data URL 前缀 (如果有)
        let seal_image_clean = if seal_image_base64.starts_with("data:") {
            seal_image_base64.split(',').nth(1).unwrap_or("").to_string()
        } else {
            seal_image_base64.to_string()
        };

        let seal_id = format!("SEAL_{}", js_sys::Date::now() as u64);

        engine.current_seal_info = Some(types::SealInfo {
            origin: "local".to_string(),
            seal_id,
            seal_name: if seal_name.is_empty() { "电子印章".to_string() } else { seal_name.to_string() },
            width: 40.0,
            height: 40.0,
            seal_type: Some(1),
            seal_image: seal_image_clean,
            sign_cert_sn: None,
            sign_data: None,
            sign_cert: None,
            seal_start_time: None,
            seal_end_time: None,
            signer_name: if signer_name.is_empty() { None } else { Some(signer_name.to_string()) },
            sign_time: None,
            sign_method: None,
            cert_issuer: None,
            cert_subject: None,
            cert_start_time: None,
            cert_end_time: None,
            cert_algorithm: None,
            cert_data: None,
        });

        web_sys::console::log_1(&format!(
            "[set_current_seal_info] 印章: '{}', 签署人: '{}', 算法: {:?}",
            seal_name, signer_name, algo
        ).into());
    });
}

/// 获取当前文档数据 (用于前端下载)
#[wasm_bindgen]
pub async fn get_document_data() -> Vec<u8> {
    with_engine(|engine| engine.doc.state.raw_data.clone())
}

/// 设置签署配置 (由前端 applyConfig 调用)
#[wasm_bindgen]
pub fn set_sign_config(algorithm: &str, is_sm2_seal: bool, sign_mode: &str, file_format: &str) {
    with_engine(|engine| {
        engine.sign_config.algorithm = if algorithm == "rsa" {
            types::Algorithm::Rsa
        } else {
            types::Algorithm::Sm2
        };
        engine.sign_config.is_sm2_seal = is_sm2_seal;
        engine.sign_config.sign_mode = match sign_mode {
            "ukey" => types::SignMode::Ukey,
            "mobile" => types::SignMode::Mobile,
            _ => types::SignMode::Cloud,
        };
        engine.sign_config.file_format = if file_format == "ofd" {
            types::DocType::Ofd
        } else {
            types::DocType::Pdf
        };

        // 同步到 SignEngine 的算法
        let seal_algo = match engine.sign_config.algorithm {
            types::Algorithm::Rsa => ses::SealAlgorithm::Rsa,
            types::Algorithm::Sm2 => ses::SealAlgorithm::Sm2,
        };
        engine.sign.set_algorithm(seal_algo);

        web_sys::console::log_1(&format!(
            "[set_sign_config] 算法={:?}, 国密章={}, 签署模式={:?}, 输出格式={:?}",
            engine.sign_config.algorithm,
            is_sm2_seal,
            engine.sign_config.sign_mode,
            engine.sign_config.file_format
        ).into());
    });
}

/// 获取当前文档的 SES 签章信息 (JSON)
#[wasm_bindgen]
pub async fn get_ses_info() -> String {
    with_engine(|engine| {
        let seals = &engine.doc.state.seals;
        let infos: Vec<serde_json::Value> = seals.iter().map(|s| {
            serde_json::json!({
                "sealId": s.id,
                "sealName": s.seal_info.seal_name,
                "page": s.page_index,
                "position": {"x": s.x, "y": s.y, "w": s.width, "h": s.height},
                "signed": s.signed,
                "algorithm": "SM2-SM3",
                "certSubject": ses::get_mock_cert_subject(ses::SealAlgorithm::Sm2),
                "certIssuer": ses::get_mock_cert_issuer(ses::SealAlgorithm::Sm2),
                "certSN": ses::get_mock_cert_sn(ses::SealAlgorithm::Sm2),
            })
        }).collect();

        serde_json::to_string(&serde_json::json!({
            "count": seals.len(),
            "signed": seals.iter().filter(|s| s.signed).count(),
            "seals": infos,
        })).unwrap_or_default()
    })
}

// ============================================================
// 国密算法测试接口 — 暴露 SM2/SM3/SM4 供 JS 端直接调用和验证
// ============================================================

/// SM3 哈希计算
#[wasm_bindgen]
pub fn sm3_hash(data: Vec<u8>) -> Vec<u8> {
    crypto::sm3_hash(&data)
}

/// SM2 签名 (返回 DER 编码的签名值)
#[wasm_bindgen]
pub fn sm2_sign(data: Vec<u8>, privkey_der: Vec<u8>) -> Result<Vec<u8>, JsValue> {
    crypto::sm2_sign(&data, &privkey_der)
        .map_err(|e| JsValue::from_str(&e))
}

/// SM2 验签
#[wasm_bindgen]
pub fn sm2_verify(data: Vec<u8>, signature: Vec<u8>, pubkey_der: Vec<u8>) -> Result<bool, JsValue> {
    crypto::sm2_verify(&data, &signature, &pubkey_der)
        .map_err(|e| JsValue::from_str(&e))
}

/// 获取 SM2 公钥 (65字节, 非压缩格式)
#[wasm_bindgen]
pub fn get_sm2_pubkey() -> Vec<u8> {
    crypto::get_sm2_pubkey()
}

/// SM4 CBC 加密
#[wasm_bindgen]
pub fn sm4_encrypt(data: Vec<u8>, key: Vec<u8>, iv: Vec<u8>) -> Result<Vec<u8>, JsValue> {
    crypto::sm4_encrypt(&data, &key, &iv)
        .map_err(|e| JsValue::from_str(&e))
}

/// SM4 CBC 解密
#[wasm_bindgen]
pub fn sm4_decrypt(data: Vec<u8>, key: Vec<u8>, iv: Vec<u8>) -> Result<Vec<u8>, JsValue> {
    crypto::sm4_decrypt(&data, &key, &iv)
        .map_err(|e| JsValue::from_str(&e))
}

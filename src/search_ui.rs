//! "查找"弹框 —— 完全由 Rust/web-sys 构建与管理。
//! 前端只负责触发 show_search_dialog()；弹框、输入框、全部标记/上一个/下一个按钮
//! 及其事件处理、命中标记、翻页滚动都在这里完成。

use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

thread_local! {
    static KEEP: RefCell<Vec<Closure<dyn FnMut()>>> = RefCell::new(Vec::new());
    static KEY: RefCell<Option<Closure<dyn FnMut(web_sys::KeyboardEvent)>>> = RefCell::new(None);
}

const MODAL_HTML: &str = r#"<div style="position:fixed;inset:0;background:rgba(0,0,0,.28);z-index:100000;display:flex;align-items:flex-start;justify-content:center;padding-top:9vh;">
  <div style="width:440px;background:#fff;border-radius:8px;box-shadow:0 8px 32px rgba(0,0,0,.18);padding:16px 18px;box-sizing:border-box;font-family:'PingFang SC','Microsoft YaHei',sans-serif;font-size:13px;line-height:1.6;color:#262626;">
    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:12px;">
      <span style="font-size:15px;font-weight:500;">查找</span>
      <span id="__search_close" style="cursor:pointer;color:#999;font-size:18px;line-height:1;padding:0 2px;">×</span>
    </div>
    <input id="__search_input" type="text" placeholder="输入要查找的内容" style="width:100%;box-sizing:border-box;padding:7px 10px;border:1px solid #d9d9d9;border-radius:4px;font-size:13px;outline:none;" />
    <div style="display:flex;gap:8px;justify-content:flex-end;margin-top:12px;">
      <button id="__search_all" style="padding:5px 12px;border:1px solid #d9d9d9;background:#fff;border-radius:4px;cursor:pointer;font-size:13px;color:#333;">全部标记</button>
      <button id="__search_prev" style="padding:5px 12px;border:1px solid #d9d9d9;background:#fff;border-radius:4px;cursor:pointer;font-size:13px;color:#333;">上一个</button>
      <button id="__search_next" style="padding:5px 12px;border:1px solid #1677ff;background:#1677ff;border-radius:4px;cursor:pointer;font-size:13px;color:#fff;">下一个</button>
    </div>
    <div id="__search_status" style="margin-top:10px;color:#999;font-size:12px;min-height:16px;"></div>
  </div>
</div>"#;

fn doc() -> Result<Document, JsValue> {
    web_sys::window()
        .ok_or_else(|| JsValue::from_str("无 window"))?
        .document()
        .ok_or_else(|| JsValue::from_str("无 document"))
}

fn set_status(status: &Element, n: usize, cur: Option<usize>, page: Option<u32>) {
    if n == 0 {
        status.set_text_content(Some("未找到"));
    } else {
        let c = cur.map(|x| x + 1).unwrap_or(0);
        let p = page.map(|x| x + 1).unwrap_or(0);
        status.set_text_content(Some(&format!("第 {} / {} 处（第 {} 页）", c, n, p)));
    }
}

/// 打开查找弹框（已打开则聚焦输入框）
pub fn show() -> Result<(), JsValue> {
    let d = doc()?;

    if d.get_element_by_id("__search_modal").is_some() {
        if let Some(inp) = d.get_element_by_id("__search_input") {
            if let Ok(i) = inp.dyn_into::<HtmlInputElement>() {
                let _ = i.focus();
            }
        }
        return Ok(());
    }

    // 旧弹框已移除，清理上一次的闭包
    KEEP.with(|k| k.borrow_mut().clear());
    KEY.with(|k| *k.borrow_mut() = None);

    let body = d.body().ok_or_else(|| JsValue::from_str("无 body"))?;
    let root = d.create_element("div")?;
    root.set_id("__search_modal");
    root.set_inner_html(MODAL_HTML);
    body.append_child(&root)?;

    let input: HtmlInputElement = d
        .get_element_by_id("__search_input")
        .ok_or_else(|| JsValue::from_str("找不到输入框"))?
        .dyn_into()?;
    let status = d
        .get_element_by_id("__search_status")
        .ok_or_else(|| JsValue::from_str("找不到状态栏"))?;
    let btn_all = d.get_element_by_id("__search_all").ok_or_else(|| JsValue::from_str("缺按钮"))?;
    let btn_prev = d.get_element_by_id("__search_prev").ok_or_else(|| JsValue::from_str("缺按钮"))?;
    let btn_next = d.get_element_by_id("__search_next").ok_or_else(|| JsValue::from_str("缺按钮"))?;
    let btn_close = d.get_element_by_id("__search_close").ok_or_else(|| JsValue::from_str("缺按钮"))?;

    // 全部标记
    {
        let input = input.clone();
        let status = status.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let (n, page) = crate::search_run(&input.value(), true);
            set_status(&status, n, if n > 0 { Some(0) } else { None }, page);
        });
        btn_all
            .unchecked_ref::<web_sys::EventTarget>()
            .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        KEEP.with(|k| k.borrow_mut().push(cb));
    }

    // 上一个
    {
        let input = input.clone();
        let status = status.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let (n, cur, page) = crate::search_step(false, &input.value());
            set_status(&status, n, cur, page);
        });
        btn_prev
            .unchecked_ref::<web_sys::EventTarget>()
            .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        KEEP.with(|k| k.borrow_mut().push(cb));
    }

    // 下一个
    {
        let input = input.clone();
        let status = status.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let (n, cur, page) = crate::search_step(true, &input.value());
            set_status(&status, n, cur, page);
        });
        btn_next
            .unchecked_ref::<web_sys::EventTarget>()
            .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        KEEP.with(|k| k.borrow_mut().push(cb));
    }

    // 关闭
    {
        let root = root.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            // 关闭弹框时清除搜索状态并重绘，移除页面上的命中标记
            crate::search_clear();
            if let Some(parent) = root.parent_node() {
                let _ = parent.remove_child(&root);
            }
        });
        btn_close
            .unchecked_ref::<web_sys::EventTarget>()
            .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        KEEP.with(|k| k.borrow_mut().push(cb));
    }

    // 回车 = 下一个
    {
        let input_for_cb = input.clone();
        let status = status.clone();
        let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
            if ev.key() == "Enter" {
                ev.prevent_default();
                let (n, cur, page) = crate::search_step(true, &input_for_cb.value());
                set_status(&status, n, cur, page);
            }
        });
        input
            .unchecked_ref::<web_sys::EventTarget>()
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())?;
        KEY.with(|k| *k.borrow_mut() = Some(cb));
    }

    let _ = input.focus();
    Ok(())
}

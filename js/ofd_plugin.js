/**
 * ofd_plugin.js — JS 桥接层 (兼容原 OFD_Plugin API)
 *
 * 将 Rust WASM 引擎暴露的 API 包装为与原 OFD_Plugin 兼容的接口，
 * 使现有 Vue 组件无需修改即可使用重构后的 WASM 引擎。
 *
 * 使用方法：
 *   在 HTML 中先引入 WASM 产物，再引入此文件：
 *   <script type="module">
 *     import init, { init_application, load_file, ... } from './pkg/dianju_wasm_seal.js';
 *     await init();
 *     window.OFD_Plugin = createOFDPlugin();
 *   </script>
 */

/**
 * 创建 OFD_Plugin 兼容对象
 */
function createOFDPlugin(wasmModule) {
  const mod = wasmModule;

  // ============================================================
  // 引擎初始化
  // ============================================================

  /**
   * 初始化应用程序
   * @param {string} spinnerId - 加载指示器元素 ID
   * @param {string} screenId - 画布容器元素 ID
   * @param {string} statusId - 状态显示元素 ID
   */
  function _InitApplication(spinnerId, screenId, statusId) {
    return mod.init_application(spinnerId, screenId, statusId);
  }

  /**
   * 注册初始化完成回调
   * @param {Function} callback
   */
  function _IniCtrlReadytCallback(callback) {
    mod.ini_ctrl_ready_callback(callback);
  }

  /**
   * 销毁引擎实例
   */
  async function _DestroyApplication() {
    return mod.destroy_application();
  }

  // ============================================================
  // 事件监听
  // ============================================================

  /**
   * 注册事件监听器
   * @param {string} eventName - 事件名 (tool_selectpoint, pageindex)
   * @param {string} jsFuncName - JS 全局函数名
   * @param {boolean} async - 是否异步
   */
  function registListener(eventName, jsFuncName, async) {
    return mod.regist_listener(eventName, jsFuncName, async);
  }

  // ============================================================
  // 文档操作
  // ============================================================

  async function LoadFile(file) {
    // 支持 File 对象或 ArrayBuffer
    let data;
    if (file instanceof ArrayBuffer) {
      data = new Uint8Array(file);
    } else if (file instanceof Uint8Array) {
      data = file;
    } else if (file.arrayBuffer) {
      const buf = await file.arrayBuffer();
      data = new Uint8Array(buf);
    } else {
      throw new Error('LoadFile: 不支持的文件类型');
    }
    // FIXME: 通过 HTTP 加载文件时需要先获取文件数据
    // 这里假设 file 参数已经是可用的文件数据
    return mod.load_file(data, file.name || 'document.pdf');
  }

  async function IsOpened() {
    return mod.is_opened();
  }

  async function GetPageCount() {
    return mod.get_page_count();
  }

  async function GetPageWidth(pageIndex) {
    return mod.get_page_width(pageIndex);
  }

  async function GetPageHeight(pageIndex) {
    return mod.get_page_height(pageIndex);
  }

  async function GetDocType() {
    return mod.get_doc_type();
  }

  async function GetCurrFileSize() {
    return mod.get_curr_file_size();
  }

  async function getFileMd5Value(param) {
    return mod.get_file_md5_value(param);
  }

  async function getDocProperty(key) {
    return mod.get_doc_property(key);
  }

  async function SetDocProperty(key, value) {
    return mod.set_doc_property(key, value);
  }

  async function SaveTo(fileName, format, flags) {
    return mod.save_to(fileName, format, flags);
  }

  // ============================================================
  // SES 签章 API (新增)
  // ============================================================

  function SetCurrentSealInfo(sealName, sealImageBase64, signerName, algorithm) {
    return mod.set_current_seal_info(sealName, sealImageBase64, signerName, algorithm);
  }

  async function GetDocumentData() {
    return mod.get_document_data();
  }

  function SetSignConfig(algorithm, isSm2Seal, signMode, fileFormat) {
    return mod.set_sign_config(algorithm, isSm2Seal, signMode, fileFormat);
  }

  async function EmbedSignatures() {
    return mod.embed_signatures();
  }

  async function GetSesInfo() {
    return mod.get_ses_info();
  }

  function BuildSesSeal(sealName, sealImageBase64, algorithm) {
    return mod.build_ses_seal(sealName, sealImageBase64, algorithm);
  }

  async function CloseDoc(flags) {
    return mod.close_doc(flags);
  }

  async function getSignaturesCount(sealType) {
    return mod.get_signatures_count(sealType);
  }

  async function GetSealInfoJson() {
    return mod.get_seal_info_json();
  }

  async function GetNextNote(nodeType, index, param) {
    return mod.get_next_note(nodeType, index, param);
  }

  async function DeleteNote(noteId) {
    return mod.delete_note(noteId);
  }

  // ============================================================
  // 印章操作
  // ============================================================

  async function GetCreateSeal(imageData, sealType, code, name, company, width, height) {
    return mod.get_create_seal(imageData, sealType, code, name, company, width, height);
  }

  async function AddSeal(cPages, reserved, mode) {
    return mod.add_seal(cPages, reserved, mode);
  }

  /**
   * 设置落章光标
   * @param {string} sealImage - base64 印章图像
   */
  async function SelectPoint(sealImage) {
    return mod.select_point(sealImage);
  }

  function CloseSelectPoint() {
    return mod.exit_select_point();
  }

  async function GetCurrentPage() {
    return mod.get_current_page();
  }

  async function getLastSeal() {
    return mod.get_last_seal();
  }

  // ============================================================
  // 签名操作
  // ============================================================

  async function GetSignSHAData() {
    return mod.get_sign_sha_data();
  }

  async function GetValueEx(key, lType, reserved1, reserved2, reserved3) {
    return mod.get_value_ex(key, lType, reserved1, reserved2, reserved3);
  }

  async function SetValueEx(key, lType, reserved, signdata) {
    return mod.set_value_ex(key, lType, reserved, signdata);
  }

  async function GetReValue() {
    return mod.get_re_value();
  }

  async function GetErrorString(code) {
    return mod.get_error_string(code);
  }

  async function reploadDocData(action) {
    return mod.repload_doc_data(action);
  }

  // ============================================================
  // 全局配置
  // ============================================================

  async function SetValue(key, value) {
    return mod.set_value(key, value);
  }

  async function GetValue(key) {
    return mod.get_value(key);
  }

  async function SetSealMode(mode) {
    return mod.set_seal_mode(mode);
  }

  async function setSingleMode(enabled) {
    return mod.set_single_mode(enabled);
  }

  // ============================================================
  // UKey 操作
  // ============================================================

  async function GetUkeyInfo(param) {
    return mod.get_ukey_info(param);
  }

  async function VerifyPin(pinCode) {
    return mod.verify_pin(pinCode);
  }

  async function GetSealListJson() {
    return mod.get_seal_list_json();
  }

  async function GetSealImage(devId, sealId) {
    return mod.get_seal_image(devId, sealId);
  }

  async function GetSealData(devId, sealId) {
    return mod.get_seal_data(devId, sealId);
  }

  async function SignData(data, pinCode) {
    return mod.sign_data(data, pinCode);
  }

  // ============================================================
  // 渲染与交互
  // ============================================================

  async function SetPageMode(mode, param) {
    return mod.set_page_mode(mode, param);
  }

  async function SetCurrPage(page) {
    return mod.set_curr_page(page);
  }

  async function GetCurrAction() {
    return mod.get_curr_action();
  }

  async function SetCurrAction(action) {
    return mod.set_curr_action(action);
  }

  async function performClick(action) {
    return mod.perform_click(action);
  }

  async function ShowDialog(mode, title, defaultPath, filter) {
    return mod.show_dialog(mode, title, defaultPath, filter);
  }

  function ClosePopupMenu() {
    return mod.close_popup_menu();
  }

  async function SearchText(text, flags, options) {
    return mod.search_text(text, flags, options);
  }

  function SetJSEnv(env) {
    return mod.set_js_env(env);
  }

  function SetShowToolBar(show) {
    return mod.set_show_tool_bar(show);
  }

  function SetShowDefMenu(show) {
    return mod.set_show_def_menu(show);
  }

  // ============================================================
  // 后台文件操作
  // ============================================================

  async function openFile_Back(filePath, readOnly) {
    return mod.open_file_back(filePath, readOnly);
  }

  async function saveTo_Back(fileHandle, fileName, options) {
    return mod.save_to_back(fileHandle, fileName, options);
  }

  async function closeFile_Back(fileHandle, save) {
    return mod.close_file_back(fileHandle, save);
  }

  async function getFileInfo(fileHandle, infoType) {
    return mod.get_file_info(fileHandle, infoType);
  }

  // ============================================================
  // HTTP 上传
  // ============================================================

  function HttpInit() {
    return mod.http_init();
  }

  function HttpAddPostString(key, value) {
    return mod.http_add_post_string(key, value);
  }

  function HttpAddPostCurrFile(fieldName) {
    return mod.http_add_post_curr_file(fieldName);
  }

  async function HttpPost(url) {
    return mod.http_post(url);
  }

  // ============================================================
  // 撤销/重做
  // ============================================================

  async function CanUndo() {
    return mod.can_undo();
  }

  async function Undo() {
    return mod.undo();
  }

  async function CanRedo() {
    return mod.can_redo();
  }

  async function Redo() {
    return mod.redo();
  }

  // ============================================================
  // 工具函数
  // ============================================================

  function version() {
    return mod.version();
  }

  // ============================================================
  // 返回 OFD_Plugin 兼容对象
  // ============================================================

  return {
    // 初始化
    _InitApplication,
    _IniCtrlReadytCallback,
    _DestroyApplication,

    // 事件
    registListener,

    // 文档
    LoadFile,
    IsOpened,
    GetPageCount,
    GetPageWidth,
    GetPageHeight,
    GetDocType,
    GetCurrFileSize,
    getFileMd5Value,
    getDocProperty,
    SetDocProperty,
    SaveTo,
    CloseDoc,
    getSignaturesCount,
    GetSealInfoJson,
    GetNextNote,
    DeleteNote,

    // 印章
    GetCreateSeal,
    AddSeal,
    SelectPoint,
    CloseSelectPoint,
    getLastSeal,

    // 签名
    GetSignSHAData,
    GetValueEx,
    SetValueEx,
    GetReValue,
    GetErrorString,
    reploadDocData,

    // 配置
    SetValue,
    GetValue,
    SetSealMode,
    setSingleMode,

    // UKey
    GetUkeyInfo,
    VerifyPin,
    GetSealListJson,
    GetSealImage,
    GetSealData,
    SignData,

    // 渲染
    SetPageMode,
    SetCurrPage,
    GetCurrentPage,
    GetCurrAction,
    SetCurrAction,
    performClick,
    ShowDialog,
    ClosePopupMenu,
    SearchText,
    SetJSEnv,
    SetShowToolBar,
    SetShowDefMenu,

    // 后台文件
    openFile_Back,
    saveTo_Back,
    closeFile_Back,
    getFileInfo,

    // HTTP
    HttpInit,
    HttpAddPostString,
    HttpAddPostCurrFile,
    HttpPost,

    // 撤销/重做
    CanUndo,
    Undo,
    CanRedo,
    Redo,

    // 工具
    version,

    // SES 签章 (新增)
    SetCurrentSealInfo,
    GetDocumentData,
    SetSignConfig,
    EmbedSignatures,
    GetSesInfo,
    BuildSesSeal,
  };
}

// ES module 导出 (import 方式)
export { createOFDPlugin };

// 兼容非 module 场景
if (typeof window !== 'undefined') {
  window.createOFDPlugin = createOFDPlugin;
}

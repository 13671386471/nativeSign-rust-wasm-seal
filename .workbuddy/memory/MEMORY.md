# rust-wasm-seal 项目笔记

## 项目概述
- 点聚版式文档签章引擎的 Rust WASM 重构版
- 使用 pdfium-render 0.9.2 (PDFium 7763 API) 进行 PDF 渲染
- PDFium WASM 模块来自 paulocoutinhox/pdfium-lib 版本 7902
- 支持 PDF 和 OFD 文档格式
- 架构: lib.rs → engine.rs (文档引擎) + render.rs (渲染引擎) + seal.rs (印章) + sign.rs (签名)

## WASM 初始化流程 (index.html)
1. 加载 PDFium WASM (pdfium.js) → PDFiumModule()
2. 等待 PDFium 就绪 (pdfium-ready 事件)
3. 加载 Rust WASM (pkg/dianju_wasm_seal.js)
4. 调用 initialize_pdfium_render(pdfiumModule, localModule, debug)
   - pdfium_wasm_module = window.__pdfiumModule (PDFium Emscripten 模块)
   - local_wasm_module = wasm-bindgen 导出模块对象 (必须包含 read_block_from_callback_wasm 等回调)
5. 加载中文字体 (register_font 函数)
6. 调用 init_application()

## PDF 空白显示问题修复 (2026-07-09)
根因: 两个关键 Bug 导致 PDF 中文内容空白:
1. local_wasm_module 参数错误 — 传了 PDFium 模块而不是 wasm-bindgen 模块
   - 影响: PDFium 无法 patch 函数表查找 read_block_from_callback_wasm 等回调
   - 修复: 使用 Object.fromEntries(wasmBindings) 作为 local_wasm_module
2. PDFium WASM 缺少中文字体提供器 — CID 字体如 STSong-Light 无法渲染
   - PDFium WASM 没有 use_platform_default_font_provider
   - 修复: 新增 font_provider.rs 模块, 实现 PdfiumCustomFontProvider trait
   - 字体数据从 CDN 加载 NotoSansSC-Regular.otf, 通过 register_font() 注入

## 关键代码结构
- render.rs: get_pdfium() 使用 call_once 初始化 Pdfium::default(), 注册字体提供器
- font_provider.rs: ChineseFontProvider 实现, 支持精确匹配和字符集回退
- pdfium-render WASM 绑定: wasm_bindings.rs 中 PdfiumRenderWasmState 管理 pdfium_wasm_module 和 local_wasm_module

## 方案 A 自动嵌中文字体 (src/font_embed.rs) — 已落地 (2026-07-09)
在 load_file() 加载 PDF 前调用 `preprocess_pdf_for_cjk()`, 把未嵌入/不可渲染的 CID 中文字体
(FangSong/STSong 等 Identity-H) 替换为真正内嵌的 NotoSansSC, 并把内容流字符码改写为
NotoSansSC 的 CID。加载链路: lib.rs load_file → preprocess_pdf_for_cjk → engine.doc 存为 raw_data。

### 已修复的 3 个致命 Bug (实测 RuntimeError: unreachable / delta 0)
1. **parse_tounicode_cid_to_unicode 的 read_hex 索引 bug (最隐蔽)**
   - 旧: `read_hex(&data[i..])` 返回的是"相对子切片的索引", 但调用方把它当"绝对索引"
     用在 `read_hex(&data[ni..])` 和 `i = ni2` 上 → 实际回到文件头部反复解析同一区域,
     永远读不到真正的 CJK 映射 → code_to_unicode 空 → cjk=false → 不替换 (delta 0)。
   - 修复: read_hex 改为 `read_hex(data, start)`, 返回绝对索引; 调用方 `read_hex(data, i)` / `i = ni2`。
2. **ToUnicode 解析循环切片越界 panic**
   - 旧: 外层 `while i + 9 < data.len()` 却切片 `&data[i..i+11]` (11 字节关键字 beginbfchar),
     末尾 i+11 越界 → `RuntimeError: unreachable`。
   - 修复: 循环条件改为 `while i + 11 <= data.len()`。
3. **CJK 判定用了资源键而非字体名**
   - 旧: `is_cjk_font("FT8", ...)` 资源键不含语义 → 永远 false。
   - 修复: 取字体真实 BaseFont (如 `ABSEKN+FangSong`) 判断; 名称含 song/hei/kai/fang 等即 CJK。
     (ABSEKN+ 是子集前缀, 含 "fang"/"song" 仍命中)

### ToUnicode 去重 bug (Kangxi 字根泄漏, 2026-07-10 修复)
- 现象: 嵌入产物文本提取里出现 Kangxi 字根 (⼀/⼄/⼯→U+2F00~) 与 CJK 字根补充 (⺠/⻓→U+2E80~),
  而原文是标准汉字 (一/乙/工/民/长)。视觉无差异, 但文本提取/搜索/复制会拿到错误码点。
- 根因: `build_to_unicode_cmap()` 对"同一 CID 对应多个 unicode 码点"去重时, 旧逻辑
  `cid2uni.sort(); dedup_by_key(cid)` 保留 **unicode 最小** 的条目。但字体 cmap 中
  U+2F00 (⼀, Kangxi) < U+4E00 (一, 标准统一汉字), 且二者指向同一 CID (已用 node
  实测确认: U+2F00→cid 9481, U+4E00→cid 9481)。于是去重错误地保留了 Kangxi 字根,
  ToUnicode 写成 cid→U+2F00, 提取就得到 ⼀。
- 修复: 新增 `unicode_priority(u)`, 标准 CJK 统一汉字(0x3400~0x9FFF)=0 最优先, 兼容区=1,
  字根补充=2, Kangxi=3, 兼容补充=4; 排序 `cid2uni.sort_by(|a,b| a.0.cmp(&b.0).then(priority).then(unicode))`
  后 `dedup_by_key(cid)` 保留第一条(=最标准的统一汉字)。
- 验证 (font_embed pkg_node 真 wasm + pypdf): Kangxi/兼容区/字根补充 全部=0;
  原文/嵌入后各 389 去重 CJK **完全一致**, **0 个 U+FFFD(tofu)**。

### 关键实现要点
- 字符码→Unicode 推导: Identity-H 字体优先用内嵌 TrueType cmap (GID=Unicode),
  否则用源 ToUnicode (lopdf `decompressed_content()` 已正确解压 FlateDecode);
  Unicode2Byte (UCS2/UTF16) 字符码即 Unicode; WinAnsi 走 cp1252 表。
- Uni→NotoSansSC CID 来自预生成 `fonts/uni2cid.bin` (由 fonttools 生成, 非 ttf-parser,
  避免 GID/CID 歧义: 中=9544 而非 8805)。生成脚本见 scripts/gen_uni2cid.py。
- 生成的新 ToUnicode 用 `build_to_unicode_cmap()` 把 NotoSansSC CID→Unicode 写回, 保证文本可提取。
- 必须 `set_plain_content()` 去掉原内容流 /Filter (FlateDecode), 否则写入未压缩内容被当压缩解压。
- 源 ToUnicode 解析加 `MAX_ENTRIES=500_000` 与每 range `steps>=200_000` 上限, 防 Wingdings 等
  61k 条 / 0xFFFFFFFF 全量范围导致 OOM panic。

### 实测结果 (test_labor_contract.pdf, FangSong+Identity-H 未嵌入)
- WASM 处理后 96KB → 8.9MB (内嵌 NotoSansSC 8.3MB), 无 panic。
- pypdf 文本提取 (node 真 wasm 产物): 原文/嵌入后各 389 个去重 CJK, **完全一致**:
  原始有嵌入缺=无, 嵌入有原始无=无, **0 个 U+FFFD(tofu)**, Kangxi 字根/兼容区/字根补充均为 0。
- 所有非标准码点泄漏已彻底消除 (详见上方 "ToUnicode 去重 bug" 修复)。

### 构建与测试坑
- **wasm-pack 在 Windows 上安装 wasm-bindgen 偶发 "failed to create temp dir"**: 不要覆盖 TEMP
  环境变量 (设 TEMP=/tmp 会让 wasm-pack 原生 temp 创建失败)。直接重试通常能过 (间歇性问题)。
- **wasm-bindgen-cli 无法在 Windows GNU 原生编译** (缺 dlltool.exe, 链接 windows-sys 失败);
  GitHub release 二进制下载被网络 reset。改用 **`wasm-pack build --target nodejs --out-dir pkg_node`**
  验证真实 wasm 产物 (cargo 从 crates.io 安装 wasm-bindgen 成功), 再用 `tools/node_test_embed.cjs`
  (Node 22) 跑 preprocess_pdf_for_cjk; 文本提取用 venv 里的 pypdf 验证。
- 2026-07-09/10 Node 验证环境: `C:/Users/tempuser1/.workbuddy/binaries/node/versions/22.22.2/node.exe`;
  Python: **直接用 managed python `-m pip` 安装到其隔离 site-packages** (无需 venv), 路径
  `C:/Users/tempuser1/.workbuddy/binaries/python/versions/3.13.12/python.exe` (已装 pypdf)。
  注意: 该 managed python 的 venv 用 `Scripts/` 而非 `bin/`, 且直接用 `-m pip` 更简单可靠。


## 中文空白问题最终链路 (2026-07-09, JS 端跨模块字体提供器)
未嵌入中文字体的 PDF（如 sample.pdf 用 /STSong-Light Type0 CID 未嵌入）必须靠 JS 端
`installChineseFontProvider()`（index.html）把字体回调装进 PDFium 函数表才能渲染。
已嵌入字体的 PDF（如阳光电源订单用嵌入 DengXian TrueType）不走这条路，永远正常。

排障中依次踩过 4 个坑（均在 installChineseFontProvider）：
1. cbGetFontData 对 table!=0 直接 return 0 → 用 extractTable() 按 tag 提取 sfnt 表
2. cbMapFont 形参顺序错 → 必须与 fpdf_sysfontinfo.h 一致:
   `(pThis, weight, italic, charset, pitch_family, face, bExact)`
3. 函数表识别用 `typeof t.length==='function'` 判断 → 错！.length 是 number 属性
4. 后续 `table.length()` 当方法调用 → 报 "table.length is not a function"

## 关键约定（避免重复踩坑）
- **WebAssembly.Table**: `.length` 是 number 属性(不加括号)；`.get(i)/.set(i,v)/.grow(n)` 才是方法。
- **给 PDFium 装字体/文件回调**: 优先用 pdfium.js 导出的 `Module.addFunction(fn, sig)`
  （签名首字符=返回类型 i/v，其余=参数，均 i32），返回表索引；比手动 grow+set 裸 JS 函数可靠。
- **PDFium 函数表获取路径**: `mod.wasmExports.__indirect_function_table`。
- FPDF_SYSFONTINFO version=2 时 EnumFonts 不被调用，只需实现 MapFont/GetFontData 等。

## ⚠️ 致命坑: ccall 未导出函数会永久毒化整个 PDFium 运行时 (2026-07-10)
- 现象: 刷新页面报 `Aborted(Assertion failed: Cannot call unknown function FPDF_GetSystemFontInfo, make sure it is exported)`。
- 根因: 该构建的 pdfium.js **只导出 `FPDF_SetSystemFontInfo` (作为 `_FPDF_SetSystemFontInfo`),
  `FPDF_GetSystemFontInfo` 未导出**。在 `installChineseFontProvider()` 末尾用
  `mod.ccall('FPDF_GetSystemFontInfo', ...)` 做"读回验证"时, Emscripten 的 `getCFunc` 找不到函数
  → `abort()`, 而 `abort()` 会 **`ABORT=true` + `readyPromiseReject` + 打印 `Aborted(...)` +
  抛出 `WebAssembly.RuntimeError`**。即使 JS `try/catch` 接住了异常, `ABORT` 标志已置位, 之后所有
  PDFium 调用(load/render)都会以同一 `Aborted(...)` 失败 → 页面完全不可用。
- 修复: **切勿调用未导出的 `FPDF_GetSystemFontInfo` 做验证**。改用
  `typeof mod['_FPDF_SetSystemFontInfo'] === 'function'` 守卫, 仅调用已导出的 Set 函数;
  安装是否生效交由实际渲染结果判断。index.html 已据此修改。
- 排查手法: 用 node 读 pdfium.js, `s.indexOf('FPDF_GetSystemFontInfo')` 确认未导出;
  再查 `function abort(what){...}` 确认其会置 `ABORT=true` 并 `readyPromiseReject`。

## 构建步骤
- wasm-pack build --target web --out-dir pkg --out-name dianju_wasm_seal
- 或手动: cargo build → wasm-bindgen CLI 生成 JS 绑定

# 未实现功能的 Rust/WASM 实现可行性分析

> 结论先行：**从 Rust 语言与 WASM 生态角度，没有任何实现阻塞点。** 上一文档列出的所有“未实现/MOCK”功能，都能用 Rust 在浏览器 WASM 里写出来。
> 真正的“卡点”只有两类：
> 1. **外部信任/集成基础设施**（CA 私钥、云签/UKey/手机盾/TSA/DCS 服务端）——与语言无关，是后端或本机代理问题；
> 2. **少量“工程量”问题**（OFD 高保真渲染、完整 PDF↔OFD 转换）——不是技术不可行，是大。
>
> 关键依据：`Cargo.toml` 的 `web-sys` 已启用 `WebSocket`、`Request`/`Response`、`HtmlInputElement`、`Crypto`/`SubtleCrypto`、`Blob`/`Url` 等特性，`wasm-bindgen-futures` 已就位；所有 stub（`search_text`/`show_dialog`/`http_post`/`undo`/`open_file_back` 等）均为纯占位、底层 Rust 代码尚未编写。

---

## 一、WASM 环境的真实约束（唯一算“环境级”的限制）

这些是所有浏览器 WASM 程序共有的，不是本项目特有的“Rust 阻塞”，但决定了**实现方式**：

| 约束 | 说明 | 对本项目的影响 | 是否阻塞 |
|---|---|---|---|
| 无直接系统资源 | WASM 内没有文件系统、没有原始 TCP、不能起本地进程 | 一切硬件/文件交互必须走浏览器 API（WebSocket 到 localhost 代理、File/Blob、fetch） | 否（已有 web-sys 能力） |
| WebCrypto 不支持国密 | `SubtleCrypto` 只有 RSA/ECDSA/AES 等标准算法，**没有 SM2/SM3/SM4** | 国密签名/摘要必须留在 Rust（`libsm` 已实现）——这恰恰是选 Rust 的理由，不是阻塞 | 否（已规避） |
| 只能异步 | WASM 不能阻塞主线程；网络/文件必须 async | 已用 `wasm-bindgen-futures`，写 async 函数即可 | 否 |
| 体积 | 加 `rsa`/`fontdue` 等会增加 wasm 体积 | 工程已用 `opt-level="s"` + `lto`；增量可控 | 否 |

**推论**：Rust 在这里不是瓶颈，反而因为 `libsm`（国密）、`lopdf`、`pdfium-render`、`quick-xml`、`zip`、`x509-parser` 等纯 Rust 库的成熟，是**最顺手的选择**。

---

## 二、逐功能可行性与阻塞点明细

> 可行性评级：极高 / 高 / 中高 / 中 / 低。
> “阻塞点”专指**用 Rust 在 WASM 里写不出来/极难写**的因素；若写得出但需外部配合，归为“外部依赖”而非“阻塞”。

### 2.1 信任链：`REPLACE_WITH_REAL_CERT`（证书/密钥）
- **现状**：`crypto.rs:28/32` 两个 base64 假证书；`ses.rs:65-78` 四个 `include_bytes!` 假 DER；SM2 密钥运行时随机生成（`crypto.rs:101`）。
- **Rust 阻塞点**：**无。** 证书 = DER 字节（已有 MOCK DER 容器）；真实证书只要换成真 DER 即可，无需改任何算法代码。`x509-parser` 已能解析。
- **可行性：极高。** 纯数据/配置替换 + 少量加载逻辑。
- **真正的 gate（非 Rust）**：私钥“存在哪、谁授权”。可选三方案：① 私钥永远在后端/云签服务（WASM 只拿签名结果）；② 私钥在 UKey/HSM（走 §2.3）；③ 私钥经 WebCrypto 注入（但 WebCrypto **无 SM2**，故国密必须走 Rust + 真实密钥注入）。**这是安全/架构决策，不是 Rust 能不能的问题。**

### 2.2 云签服务：`REPLACE_WITH_REAL_SIGN_SERVICE`
- **现状**：`sign.rs:278` `cloud_sign` 直接返回 `MOCK_SN_*` 假响应，未发起真实请求。
- **Rust 阻塞点**：**无。** `web-sys` 的 `Request`/`Response`/`Headers`（已在 `Cargo.toml:29-32` 启用）+ `wasm-bindgen-futures` 即可发异步 fetch。
- **可行性：高。** 约一个模块：拼 JSON → fetch → 解析签名结果。
- **外部依赖**：后端 `/local/sign/sealSign`（或等价）接口的**请求/响应契约** + 浏览器 CORS/同源策略（通常由后端代理解决）。

### 2.3 UKey 硬件代理：`REPLACE_WITH_REAL_UKEY_PROXY`
- **现状**：`ukey.rs` 全模块返回假数据（设备状态永远正常、印章列表硬编码、签名用本地假 SM2）。无任何 WebSocket 建连。
- **Rust 阻塞点**：**无客户端阻塞。** `web-sys` 的 `WebSocket`/`MessageEvent`/`BinaryType`（已在 `Cargo.toml:34-36` 启用），连 `ws://127.0.0.1:18080` 完全可行。
- **可行性：高（客户端）/ 受限于外部。** 写 WebSocket 客户端很直接；
- **真正的 gate（非 Rust）**：① 用户本机必须装有**原厂 UKey 本地代理程序**（Rust 无法凭空提供）；② 该代理的**二进制通信协议是私有/未文档化的**——需要原 C++ 引擎的 UKey 协议资料复用。这部分是“外部依赖 + 协议逆向/对接”，不是 Rust 语言难关。

### 2.4 手机盾 / 移动签名
- **现状**：无模块，仅 `types.rs` 有 `SignMode::Mobile` 枚举。
- **Rust 阻塞点**：**无。** 本质是“轮询后端签名状态 + 前端展示二维码”，与 §2.2 同类。
- **可行性：高。**
- **外部依赖**：移动签名后端编排服务（下发待签数据、推送、回传签名）。

### 2.5 RSA 真实签名（修复 `crypto.rs:234` 假实现）
- **现状**：`rsa_sign` 仅把 SHA256 摘要塞进 256 字节数组，非真实私钥签名。
- **Rust 阻塞点**：**无。** 加一个纯 Rust 的 `rsa` crate（RustCrypto，WASM 兼容，无原生依赖），实现 PKCS#1v1.5/PSS + SHA256，约 50 行。SHA256 已就绪（`crypto.rs:53`）。
- **可行性：极高。**

### 2.6 文本搜索 `search_text`（`lib.rs:712` 返回 `"0"`）
- **现状**：纯占位。
- **Rust 阻塞点**：**无。**
- **可行性：高（OFD 易 / PDF 中）。**
  - OFD：文本对象已在 `ofd_parser.rs` 解析出（`OfdTextItem.text`），搜索即字符串匹配，**几乎零成本**。
  - PDF：`lopdf` 可读取内容流，但需结合 CMap/ToUnicode 把字符码还原成真实 Unicode——本项目 `font_embed.rs` 已写过完整的 ToUnicode/CMap 解析逻辑，可直接复用，**中等工作量**，无技术阻塞。

### 2.7 文件对话框 `show_dialog`（`lib.rs:698` 返回 `None`）
- **现状**：占位 + `FIXME`。
- **Rust 阻塞点**：**无。** 打开用 `HtmlInputElement`（`type=file`，已启用）触发系统选择；保存用 `Blob`+`Url` 创建下载。
- **可行性：高。**

### 2.8 HTTP 上传 `http_init/add_post_*/http_post`（`lib.rs:825-846`）
- **现状**：全部空实现，`http_post` 返回 `None`。
- **Rust 阻塞点**：**无。** `fetch` + `FormData` + `Blob`（均已启用）即可构造 multipart 上传。
- **可行性：高。** 需把 `http_add_post_curr_file` 里暂存当前文档字节（已有 `engine.state.raw_data`）。
- **外部依赖**：上传目标服务的 CORS 策略。

### 2.9 撤销/重做 `can_undo/undo/can_redo/redo`（`lib.rs:852-871`）
- **现状**：全返回 `0`/`"0"`。
- **Rust 阻塞点**：**无。** 纯状态管理：命令栈（记录每次落章/删章操作）或文档快照（`lopdf::Document` 可 clone，印章操作也可重放）。
- **可行性：高。** 设计选择问题（快照 vs 命令模式），非技术阻塞。

### 2.10 后台文件操作 `open_file_back/save_to_back/close_file_back/get_file_info`（`lib.rs:798-819`）
- **现状**：占位，注释写明“用于格式转换”。
- **Rust 阻塞点**：**取决于定义。** 若指“调用本机/服务端文件存储”，则属于后端能力；若仅指本地文件读写/句柄，浏览器 `File`/`Blob` 已够。
- **可行性：中（若纯本地则高）。**
- **外部依赖**：若对接原引擎的“后台文件服务”，需其接口契约。

### 2.11 文档大纲/书签 `get_next_note`（`engine.rs:558` 返回 `None`）
- **现状**：占位。
- **Rust 阻塞点**：**无。** PDF 大纲需手动遍历 `/Outlines`（`lopdf` 无高级 API，但 `get_object` 足够）；OFD 大纲在 `Document.xml`，`ofd_parser.rs` 的 XML 解析可直接扩展。
- **可行性：高。**

### 2.12 DCS 格式转换后端（PDF↔OFD）
- **现状**：无配置、无调用（原引擎 `reader_config.ini [convert]` 指向 `47.94.147.6:8085/DCS`）。
- **Rust 阻塞点**：**客户端无阻塞**——WASM 侧只是发请求拿转换后文件（与 §2.2 同类）。
- **可行性：客户端高 / 完整转换器工程量极大。**
- **外部依赖 + 工程量**：真正的 PDF↔OFD 双向转换是一个**完整的文档渲染+模型重建系统**，即使全用 Rust（已有 `lopdf`/`pdfium-render`/`quick-xml`/`zip` 全部基元）也需数月级工程量。**务实建议：转换服务放后端（独立微服务），WASM 只做客户端调用，不要在浏览器里重写转换器。**

### 2.13 TSA 时间戳 / LTV / PAdES
- **现状**：未实现；签名时间硬编码（`ses.rs:493` 永远 `2026-07-10`）。
- **Rust 阻塞点**：**无。** 需扩展 `der.rs`（已有 SEQUENCE/SET/OID/UTCTime）编码 RFC 3161 `TimeStampToken`，并对 TSA 发 HTTP 请求（`web-sys` 已支持）。证书链校验可复用 `x509-parser`。
- **可行性：中高。** 比基础签名复杂，但所有 ASN.1 基元已具备。
- **外部依赖**：TSA 服务端端点。

### 2.14 PDF 页数文本扫描脆弱（`engine.rs:72`）
- **现状**：用 `/Count` 文本扫描 + `/Type /Page` 统计兜底，对压缩/线性化/加密 PDF 易误判。
- **Rust 阻塞点**：**无。** 页面尺寸提取已用 `lopdf`（`engine.rs:127`），**把页数也改用 `lopdf::Document::get_pages()` 即可**，与现有宽度/高度路径一致。
- **可行性：极高（纯重构）。**

### 2.15 OFD 渲染保真度（CSS 字体栈 → 内嵌字体）
- **现状**：`render.rs:472` 用 CSS 字体栈（`sans-serif` 等）而非 OFD 内嵌字体，缺字距/微调/图层/模板，复杂版式会走形。
- **Rust 阻塞点**：**无。** Rust 有纯 Rust、WASM 兼容的字形库：`fontdue` / `ab_glyph`（字形光栅化）、`ttf-parser`（字体解析）、`resvg`/`tiny-skia`（矢量绘制）。OFD ZIP 内字体文件已被 `ofd_parser` 收集（`public_images`/`fonts`），可解析后光栅，再用 `CanvasRenderingContext2d.putImageData` 或 `Path2D` 绘制。
- **可行性：中高（工程量）。** 单字形绘制不难，难点在精确文本排版（字距、竖排、复杂脚本、与 OFD 度量对齐）。
- **外部依赖**：无（纯前端工作量）。

### 2.16 签名时间硬编码（`ses.rs:493` `current_time_tuple`）
- **现状**：永远返回 `(2026,7,10,16,30,0)`。
- **Rust 阻塞点**：**无。** 经 `js-sys`/`web-sys` 的 `Date` 取真实时间注入即可（甚至现有 `set_value_ex` 流程里加一个 JS 时间参数）。
- **可行性：极高。**

---

## 三、汇总表

| # | 功能 | Rust 语言阻塞点 | 可行性 | 主要外部依赖 / 工程量 |
|---|---|---|---|---|
| 1 | 真实证书/密钥 | 无 | 极高 | CA 证书、密钥托管策略 |
| 2 | 云签服务 | 无 | 高 | 后端签名契约 + CORS |
| 3 | UKey 代理 | 无（客户端） | 高 | 本机 UKey 代理 + 私有协议 |
| 4 | 手机盾 | 无 | 高 | 移动签名后端 |
| 5 | RSA 真签名 | 无 | 极高 | 加 `rsa` crate |
| 6 | 文本搜索 | 无 | 高（OFD易/PDF中） | 复用 font_embed 的 CMap 逻辑 |
| 7 | 文件对话框 | 无 | 高 | — |
| 8 | HTTP 上传 | 无 | 高 | 上传目标 CORS |
| 9 | 撤销/重做 | 无 | 高 | 仅设计选择 |
| 10 | 后台文件操作 | 视定义 | 中（纯本地高） | 后端文件服务契约 |
| 11 | 大纲/书签 | 无 | 高 | — |
| 12 | DCS 格式转换 | 客户端无 | 客户端高/完整转换器极大 | DCS 服务端（建议后端化） |
| 13 | TSA/LTV/PAdES | 无 | 中高 | TSA 服务端 |
| 14 | PDF 页数扫描脆弱 | 无 | 极高（重构） | 改用 lopdf |
| 15 | OFD 渲染保真 | 无 | 中高（工程量） | fontdue/ab_glyph 等纯 Rust 库 |
| 16 | 签名时间硬编码 | 无 | 极高 | JS 注入时间 |

---

## 四、最终判断与建议

**1. 阻塞点结论**：**Rust/WASM 层面零阻塞。** 16 项中 11 项“可行性=极高/高”且无需任何新依赖；5 项（UKey 代理、手机盾、DCS 转换、TSA、OFD 保真）也都能用 Rust 写出，只是分别受“外部本机代理/后端服务”或“工程量”制约——这些制约与语言无关。

**2. 真正要解决的不是“Rust 能不能”，而是“信任链与外围系统怎么接”**：P0 的证书/云签/UKey 三项，工作重心在**后端服务、CA 体系、本机代理协议对接**，Rust 侧只是把目前已占位的 stub 接上真实调用。

**3. 顺手程度印证选型正确**：国密（libsm）、PDF（lopdf/pdfium-render）、OFD（quick-xml/zip）、X.509（x509-parser）、DER（自研 der.rs）全部是成熟纯 Rust 库；唯一 WebCrypto 缺失国密反而是 Rust 必须扛起来的部分，且已落地。

**4. 建议落地顺序**（与 `feature-status.md` §五一致，但强调“可行性”视角）：
- 先啃 **§2.14、§2.16、§2.5、§2.9、§2.11、§2.7、§2.8**（极高/高可行性、无外部依赖）→ 快速把“看起来能用的原型”变成“功能完整的原型”。
- 再攻 **§2.1、§2.2**（信任链 + 云签）→ 决定签名是否可被监管认可，**这一步才是上线硬门槛**，但 Rust 工作量小、外部对接大。
- 最后按业务优先级补 **§2.3/§2.4（UKey/手机盾）、§2.13（TSA）、§2.15（OFD 保真）、§2.12（转换放后端）**。

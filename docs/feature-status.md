# rust-wasm-seal 功能实现状态对照表

> 项目：`dianju-wasm-seal`（点聚版式文档签章引擎 · Rust WASM 重构版）
> 对比对象：原 C++/Qt WASM 签章引擎（`front-new/dianju-ui-admin`，即 `dianjureader.wasm`，见 `docs/wasm-seal-engine/`）
> 生成日期：基于源码实地核查（`src/*.rs` 全量阅读）
>
> **一句话结论**：Rust 重构版在“文档渲染 + 签章嵌入 + 国密算法 + SES/PKCS#7 结构编码 + OFD 解析”这条主干上已经做到**真实可用**（不是壳），但**信任链与外围 IO 全部是 MOCK**——证书/密钥、云签服务、UKey 硬件代理、RSA 签名都是假的，签名产物结构合法但**密码学上不可验真**。

---

## 一、总览矩阵（已实现 vs 未实现）

| 能力域 | 状态 | 关键证据 | 与原 C++ 引擎对比 |
|---|---|---|---|
| PDF 渲染（位图） | ✅ 真实现 | `render.rs:236` `render_all_pdf_pages` 经 `pdfium-render` 出图 | 原引擎用 Qt 内置渲染；二者均可出图 |
| OFD 渲染（矢量） | ✅ 真实现（近似） | `render.rs:472` `render_all_ofd_pages` Canvas2D 文本/路径/图片 | 原引擎同样矢量渲染，但保真度更高 |
| 中文字体渲染 | ✅ 真实现 | `font_provider.rs` + `font_embed.rs`（CID 字体注入 PDF） | 原引擎靠 JS 端 `installChineseFontProvider` |
| PDF 解析（页数/尺寸） | ⚠️ 部分真/部分糙 | `engine.rs:127` 用 `lopdf` 取 MediaBox（真）；`engine.rs:72` 页数用文本扫描（糙） | 原引擎内部全程 C++ PDF 解析 |
| OFD 解析（页数/对象） | ✅ 真实现 | `ofd_parser.rs` 真实 ZIP+XML 解析 | 对等 |
| 已有签章/签名域解析 | ✅ 真实现 | `engine.rs:251` `parse_existing_signatures` + `x509-parser` 抽证书 | 对等 |
| SM3 哈希 | ✅ 真实现 | `crypto.rs:40` `libsm::sm3` | 对等 |
| SM2 签名/验签 | ✅ 算法真·密钥假 | `crypto.rs:141`/`164` `libsm`；密钥随机生成 `crypto.rs:101` | 算法对等，信任链缺失 |
| SM4 加解密 | ✅ 真实现 | `crypto.rs:193`/`215` `libsm` CBC | 对等 |
| SHA256 / MD5 | ✅ 真实现 | `crypto.rs:53`/`71` | 对等 |
| RSA 签名 | ❌ 假实现 | `crypto.rs:234` 仅把哈希塞进 256 字节 | 原引擎走真实 RSA/PKCS#1 |
| SES 结构编码（GM/T 0031） | ✅ 真实现 | `ses.rs:275`/`369`/`330` DER 编码 | 结构对等（内容用 MOCK 证书） |
| PKCS#7 SignedData 编码 | ✅ 真实现 | `pkcs7.rs:244`/`312` 含 AuthAttr | 结构对等（签名值 MOCK） |
| PDF 签章嵌入（含 ByteRange） | ✅ 真实现 | `seal.rs:125` `embed_seals_to_pdf` + `finalize_pdf_signatures` 两遍填充 | 对等（含合法 ByteRange） |
| OFD 签章嵌入（Sign_N/Seal.esl/SignedValue） | ✅ 真实现 | `ofd_sign.rs:58` ZIP 重写 | 对等 |
| 证书 / CA 信任链 | ❌ MOCK | `crypto.rs:28/32`、`ses.rs:65-78` `MOCK_*_CERT` | 原引擎接真实 CA/云证书 |
| 云签服务 | ❌ MOCK | `sign.rs:278` `cloud_sign` 返回假响应 | 原引擎真发 `/local/sign/sealSign` |
| UKey 硬件代理 | ❌ MOCK | `ukey.rs` 全模块返回假数据 | 原引擎接本地 WebSocket 代理 |
| 手机盾/移动签名 | ❌ 未实现 | 无对应模块 | 原引擎支持 |
| 文本搜索 | ❌ 未实现 | `lib.rs` `search_text` 返回 `"0"` | 原引擎支持 |
| 文件对话框 | ❌ 未实现 | `lib.rs` `show_dialog` 返回 `None` | 原引擎支持 |
| HTTP 上传/回传 | ❌ 未实现 | `lib.rs` `http_init/add_post_*/http_post` 仅 FIXME | 原引擎支持 |
| 撤销/重做 | ❌ 未实现 | `lib.rs` `can_undo/undo/can_redo/redo` 返回 `0` | 原引擎支持 |
| 文档大纲/书签 | ❌ 未实现 | `engine.rs:558` `get_next_note` 返回 `None` | 原引擎支持 |
| 文档/格式转换后端（DCS） | ❌ 未实现 | 无 `convert` 配置与调用 | 原引擎接 `47.94.147.6:8085/DCS` |
| 签名时间戳 TSA / LTV | ❌ 未实现 | 时间为硬编码 `ses.rs:493` | 原引擎可带 TSA |

---

## 二、已真实实现的功能（含代码证据）

### 2.1 国密与哈希算法（密码学算法本身为真）
- **SM3**（`crypto.rs:40`）：`libsm::sm3::hash` 标准实现，单元测过 32 字节输出。
- **SM2 签名/验签**（`crypto.rs:141`/`164`）：`libsm` 椭圆曲线签名，DER 编码，自带 `sm2_sign → sm2_verify` 闭环测试通过。⚠️ 但密钥是 `OnceLock` 首次随机生成（`crypto.rs:101`），**每次加载 WASM 都不同、无持久性、非 CA 签发**。
- **SM4-CBC**（`crypto.rs:193`/`215`）：真实分组密码，PKCS#7 自动填充，含空数据/整块/错密钥单测。
- **SHA256 / MD5 / base64 / 文件前 N 字节 MD5**：全部真实（`crypto.rs:53`/`71`/`270`）。

### 2.2 文档解析
- **PDF 页数提取**（`engine.rs:72`）：用 `/Count` 文本扫描 + `/Type /Page` 统计兜底。⚠️ 对压缩/线性化/加密 PDF 可能失效（非完整 PDF 解析器）。
- **PDF 页面尺寸**（`engine.rs:127`/`170`）：用 `lopdf` 真实读取 `MediaBox`，这是真正可靠的解析路径。
- **OFD 解析**（`ofd_parser.rs`）：真实 ZIP（zip crate）+ XML（quick-xml）解析，提取 Text/Path/Image 对象、物理框、注释、公共资源字体与图片，是完整可用的 OFD 读取器。
- **已有签章/签名域提取**（`engine.rs:251` `parse_existing_signatures`）：遍历 Annots，识别 `/Subtype Widget /FT Sig` 与 `/Stamp`，抽取 Rect、字段名、是否含 `/V` 签名值，并**从 `/Contents` 的 PKCS#7 中用 `x509-parser` 真实解析出证书序列号/颁发者/主题/CN/有效期**（`engine.rs:617` `extract_cert_from_pkcs7`）。这部分完全真实、可直接用于展示已签信息。

### 2.3 渲染
- **PDF 渲染**（`render.rs:236`）：经 `pdfium-render` 把每页渲染成位图，按 FitWidth/FitPage/Original 等模式计算尺寸，写入每页独立 `<canvas>`，并叠加印章图层。带“非白像素诊断”日志确认字体是否生效。**这是与原引擎对等的核心能力。**
- **OFD 渲染**（`render.rs:472`）：用 Canvas 2D 真实绘制文本（CTM 变换 + CSS 字体栈）、路径（含 SVG 圆弧 `draw_svg_arc`、轴对齐矩形优化）、图片（PNG 解码）、注释。属于“矢量近似渲染”，保真度弱于原引擎的 Qt 渲染，但**能正确呈现版式**。
- **中文显示**：`font_provider.rs` 给 PDFium 装 CID 中文字体提供器；`font_embed.rs`（51KB）在加载未嵌入中文字体的 PDF 时，把 CID 字体替换为内嵌 NotoSansSC 并改写 ToUnicode——真实解决“中文空白”问题。

### 2.4 签章嵌入（落章 + 签名值写入）
- **PDF 嵌入**（`seal.rs:125` `embed_seals_to_pdf`）：用 `lopdf` 创建图片 XObject、外观 Form XObject、签名字典（预留 `/Contents` 占位 + `/ByteRange` 占位）、Widget 注释、AcroForm 字段；保存后用 `finalize_pdf_signatures`（`seal.rs:545`）**迭代计算 ByteRange 并填入 PKCS#7**（含 lopdf 0.34 无空格分隔的兼容处理）。这是标准、可被 PDF 阅读器识别的签名域写入流程。
- **OFD 嵌入**（`ofd_sign.rs:58` `embed_seals_to_ofd`）：重写 OFD ZIP，新增 `Sign_N/Sign_N.xml`、`Seal.esl`、`SignedValue.dat`，并在 `OFD.xml` 注入 `Signs` 引用；`build_sign_xml` 按 GB/T 33190-2016 第 18 章生成签章描述，`update_ofd_xml` 处理已有/新增 Signs。完整可落地。

### 2.5 国密签章结构编码（DER）
- **自研 DER 编码器**（`der.rs`）：手工实现 SEQUENCE/SET/INTEGER/OID/BIT STRING/OCTET STRING/UTF8String/PrintableString/UTCTime/上下文标签，符合 X.690。
- **SES 结构**（`ses.rs`）：`build_ses_seal`（SES_Seal）、`build_ses_signature`/`build_tbs_sign`（SES_Signature/TBS_Sign）、`SES_Header/SealInfo/SealProperty/SealPicture` 全部按 GM/T 0031-2014 ASN.1 真实编码。
- **PKCS#7 SignedData**（`pkcs7.rs:244`）：ContentInfo → SignedData → SignerInfo，含 `authenticatedAttributes`（contentType/messageDigest/signingTime）、`IssuerAndSerialNumber` 抽取、`certificates` 集合——结构完全合法。

### 2.6 WASM 对外 API 兼容层
- `lib.rs` 暴露了与 `index.vue` / `OFD_Plugin` 大量同名方法（`LoadFile`、`GetPageCount`、`AddSeal`、`GetSignSHAData`、`GetValueEx/SetValueEx`、`GetCreateSeal`、`GetSealImage/Data/ListJson`、`GetUkeyInfo`、`VerifyPin`、`SignData`、`SaveTo`、`CloseDoc`、`_InitApplication`、`_DestroyApplication`、`reploadDocData`、`selectPoint`、`searchText` 等），使前端可在“接口对齐”层面替换引擎。

---

## 三、尚未实现 / 占位（MOCK）的功能

> 以下全部带有源码中的 `FIXME: REPLACE_WITH_REAL_*` 标记，定位明确、待替换。

### 3.1 信任链（最关键的缺口）
- **证书与密钥全是假的**：`crypto.rs:28/32` 两个 base64 假证书；`ses.rs:65-78` 四个 `include_bytes!` 假 DER 证书/私钥（RSA + EC/SM2 替代）。`sm2_sign` 用的密钥是运行时随机生成、无 CA、不可验证（`crypto.rs:101`）。
- **RSA 签名是假的**：`crypto.rs:234` `rsa_sign` 仅把 SHA256 摘要塞进 256 字节数组，并非真实 RSA 私钥签名。
- **后果**：所有 `build_mock_pkcs7` / `build_mock_ses_seal` / `build_mock_ses_signature` 产出的签名值，结构合法但**用任何标准验签工具都会失败**。要上线必须把 `REPLACE_WITH_REAL_CERT` 换成真实 CA 证书 + 私钥（或改为调用签名服务/HSM）。

### 3.2 云签服务（FIXME: REPLACE_WITH_REAL_SIGN_SERVICE）
- `sign.rs:278` `cloud_sign` 直接返回 `MOCK_SN_*` 与假证书，**没有真正请求**原引擎对接的 `/local/sign/sealSign` 云端签名接口。生产环境应由后端/云盾完成私钥运算，这里只是占位。

### 3.3 UKey 硬件代理（FIXME: REPLACE_WITH_REAL_UKEY_PROXY）
- `ukey.rs` 整个模块都是假数据：`get_ukey_info` 永远返回 `status:0`；`verify_pin` 永远成功；`get_seal_list_json`/`get_seal_image`/`get_seal_data` 返回硬编码“测试公章/法人章”和 1×1 占位 PNG；`sign_data` 用本地假 SM2 算。
- 原引擎通过 `ws://127.0.0.1` 本地 WebSocket 代理与真实 UKey 通信（密钥不出硬件）。这里**完全没有建连逻辑**。

### 3.4 手机盾 / 移动端签名
- 无模块、无接口实现。`types.rs` 里 `SignMode::Mobile` 仅有枚举定义，未落地。

### 3.5 渲染与解析的保真度缺口
- **OFD 渲染保真度**：使用 CSS 字体栈（如 `sans-serif`）而非 OFD 内嵌字体，缺少字距/微调/图层/模板/精确度量；复杂版式可能走形。属“能看不可精校”。
- **PDF 页数解析脆弱**：`engine.rs:72` 的文本扫描对压缩对象名、线性化、加密 PDF 可能误判；应改为 `lopdf` 解析（尺寸提取已用 `lopdf`，页数未统一）。
- **签名时间硬编码**：`ses.rs:493` `current_time_tuple` 永远返回 `2026-07-10`，WASM 内未注入真实系统时间。

### 3.6 交互 / IO 功能（Q5 列表里的明确 stub）
| 接口 | 现状 | 源码位置 |
|---|---|---|
| `show_dialog` | 返回 `None`（文件对话框未实现） | `lib.rs` |
| `search_text` | 返回 `"0"`（文本搜索未实现） | `lib.rs` |
| `http_init/add_post_string/add_post_curr_file/http_post` | 仅 FIXME，无实现（HTTP 回传） | `lib.rs` |
| `can_undo/undo/can_redo/redo` | 返回 `0`/`"0"`（撤销重做未实现） | `lib.rs` |
| `open_file_back/save_to_back/close_file_back/get_file_info` | 返回 `1`/`None`（后台文件通道未实现） | `lib.rs` |
| `get_next_note` | 返回 `None`（大纲/书签未解析） | `engine.rs:558` |
| 文档/格式转换后端（DCS） | 无配置与调用 | 原引擎 `reader_config.ini [convert]` 指向 `47.94.147.6:8085/DCS` |
| TSA 时间戳 / LTV / PAdES | 未实现 | — |
| 骑缝章自动分割、关键字自动落章（AUTO_ADD_SEAL） | `seal.rs:54` 直接报错“需先搜索关键字”，未实现 | `seal.rs` |

---

## 四、与原 C++/Qt 引擎的能力落差（点聚完整产品视角）

原 `dianjureader.wasm`（31MB，Qt + Emscripten + Embind）提供的是**完整商业产品能力**；Rust 重构版目前是**聚焦“渲染+落章+国密结构”的可验证原型**，二者在“能打开、能看、能盖一个结构合法的章”上对齐，但外围完整性和可信度差距明显：

| 维度 | 原 C++/Qt 引擎 | Rust 重构版 |
|---|---|---|
| 渲染引擎 | Qt 内置，PDF+OFD 高保真 | PDFium（PDF 真）+ Canvas2D（OFD 近似） |
| 字体 | 内嵌 + 在线字体列表 + 平台字体提供器 | 自研 CID 注入 + PDFium 字体提供器（中文已解决） |
| 签章信任 | 真实 CA/云证书、真实 RSA/SM2 私钥 | MOCK 证书 + 随机 SM2 密钥（签名不可验真） |
| 签名来源 | 云签 / UKey / 手机盾 真实通道 | 仅 MOCK（云签/UKey 占位，手机盾无） |
| 格式转换 | 接 DCS 服务端转 PDF/OFD | 无 |
| 文档编辑/批注 | 注释、书签、大纲、搜索、撤销重做 | 仅解析与落章，搜索/大纲/撤销均 stub |
| 外部依赖注入 | DI_FileOpen/Save/IndexDB/FetchDRC/Font 等 | 无（JS 桥接层未建） |
| 体积/启动 | 31MB，Qt 运行时重 | 远小于 Qt（WASM 体积优势明显） |

**结论性判断**：Rust 版在“签名结构正确性 + 渲染可用性 + 国产算法 + 小体积”上已经有真实价值，是合格的**前端可替换引擎骨架**；但凡涉及“签名必须被人信任/被监管认可”的场景，必须先补齐 §3.1–§3.3 的信任链与真实通道，否则产物只是“看起来签了”的演示章。

---

## 五、落地优先级建议（按可信度依赖排序）

1. **P0 信任链**：`REPLACE_WITH_REAL_CERT` + `REPLACE_WITH_REAL_SIGN_SERVICE`——接入真实 CA 证书与云签/签名服务，使 PKCS#7/SES 可被标准工具验签。这是所有上线场景的硬前提。
2. **P1 UKey/手机盾真实通道**：`REPLACE_WITH_REAL_UKEY_PROXY` + 移动签名模块，覆盖硬件与移动签名场景。
3. **P1 真实系统时间注入**：JS 注入当前时间，替换 `ses.rs:493` 硬编码，否则签名时间永远错。
4. **P2 PDF 解析统一**：把 `engine.rs:72` 页数也改用 `lopdf` 解析，消除文本扫描的误判。
5. **P2 交互/IO 补全**：`search_text`、`show_dialog`、`http_*` 回传、撤销重做、大纲——按需补齐。
6. **P3 OFD 渲染保真**：引入 OFD 内嵌字体实测度量、图层/模板，提升与原引擎的对等度。
7. **P3 转换后端/DS**：如需跨格式转换，对接或自建 DCS 类服务。

@echo off
REM ============================================================
REM 构建脚本 — dianju-wasm-seal (Rust WASM 重构版)
REM
REM 前置要求:
REM   1. Rust 工具链 (rustup, cargo)
REM   2. wasm32-unknown-unknown 目标: rustup target add wasm32-unknown-unknown
REM   3. wasm-pack: cargo install wasm-pack
REM
REM 构建产物位于: pkg/
REM ============================================================

echo [1/3] 检查 Rust 工具链...
rustc --version >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo 错误: 未找到 Rust 工具链，请先安装 https://rustup.rs
    exit /b 1
)

echo [2/3] 构建 WASM (release)...
call wasm-pack build --target web --out-dir pkg --release
if %ERRORLEVEL% NEQ 0 (
    echo 错误: WASM 构建失败
    exit /b 1
)

echo [3/3] 构建完成!
echo.
echo 构建产物:
echo   pkg/dianju_wasm_seal_bg.wasm  — WASM 二进制文件
echo   pkg/dianju_wasm_seal.js       — JS 胶水代码 (自动生成)
echo   pkg/dianju_wasm_seal.d.ts     — TypeScript 类型定义
echo.
echo 配合 js/ofd_plugin.js 使用即可兼容原 OFD_Plugin API
echo.
echo 生产部署前请检查以下 TODO:
echo   - FIXME: REPLACE_WITH_REAL_CERT  (crypto.rs)
echo   - FIXME: REPLACE_WITH_REAL_SIGN_SERVICE (sign.rs)
echo   - FIXME: REPLACE_WITH_REAL_UKEY_PROXY (ukey.rs)
echo   - FIXME: REPLACE_WITH_REAL_DOC_ENGINE (engine.rs)
echo   - FIXME: REPLACE_WITH_REAL_RENDER (render.rs)

exit /b 0

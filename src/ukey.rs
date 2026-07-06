//! UKey 硬件通信模块
//!
//! 通过 WebSocket 代理与本地 UKey 硬件通信
//!
//! ⚠️ MOCK 说明:
//! - UKey 设备交互通过本地 WebSocket 代理 (ws://localhost:xxxxx)
//! - 当前返回模拟数据，标记 FIXME: REPLACE_WITH_REAL_UKEY_PROXY
//! - 生产环境需连接真实的 UKey WebSocket 代理服务

use crate::types::*;

/// UKey 通信引擎
pub struct UkeyEngine {
    /// WebSocket 代理地址
    ws_url: String,
    /// 当前 PIN 码 (会话中缓存)
    pin_code: Option<String>,
    /// 当前 UKey 连接状态
    connected: bool,
}

impl UkeyEngine {
    pub fn new() -> Self {
        Self {
            // FIXME: REPLACE_WITH_REAL_UKEY_PROXY — 替换为真实的 WebSocket 代理地址
            ws_url: String::from("ws://127.0.0.1:18080/ukey"),
            pin_code: None,
            connected: false,
        }
    }

    /// 获取 UKey 设备信息
    /// 对应 OFD_Plugin.GetUkeyInfo(1)
    ///
    /// 返回值:
    ///   "" — 未安装或未启动 UKey 本地服务
    ///   "error:xxx" — 错误信息
    ///   JSON {status: 0, errmsg: ""} — 设备状态
    ///     status: 0=正常, 2=未插UKey, 12=请拔下其他UKey
    pub async fn get_ukey_info(&self, _param: i32) -> Result<String, String> {
        // FIXME: REPLACE_WITH_REAL_UKEY_PROXY
        // 生产环境通过 WebSocket 发送 GetDeviceStatus 指令

        let info = UkeyInfo {
            status: 0,
            errmsg: None,
            retstr: None,
        };

        Ok(serde_json::to_string(&info).unwrap_or_default())
    }

    /// 验证 UKey PIN 码
    /// 对应 OFD_Plugin.VerifyPin(pinCode)
    ///
    /// 返回值 JSON:
    ///   {status: 0} — 验证成功
    ///   {status: 134, retstr: ["remaining"]} — PIN码错误
    pub async fn verify_pin(&mut self, pin_code: &str) -> Result<String, String> {
        // FIXME: REPLACE_WITH_REAL_UKEY_PROXY
        // 生产环境通过 WebSocket 发送 VerifyPIN 指令

        self.pin_code = Some(pin_code.to_string());

        // 模拟 PIN 验证
        // 在真实场景中，错误 PIN 会返回 status=134
        let result = serde_json::json!({
            "status": 0,
            "errmsg": "",
            "retstr": []
        });

        Ok(result.to_string())
    }

    /// 获取 UKey 中的印章列表
    /// 对应 OFD_Plugin.GetSealListJson()
    ///
    /// 返回 base64 编码的 JSON 字符串
    pub async fn get_seal_list_json(&self) -> Result<String, String> {
        // FIXME: REPLACE_WITH_REAL_UKEY_PROXY
        // 生产环境通过 WebSocket 发送 ListSeals 指令

        let mock_list = UkeySealList {
            dev_list: vec![UkeyDevice {
                dev_id: "DEV001".to_string(),
                seal_list: vec![
                    UkeySeal {
                        seal_id: "SEAL001".to_string(),
                        seal_name: "测试公章".to_string(),
                    },
                    UkeySeal {
                        seal_id: "SEAL002".to_string(),
                        seal_name: "法人章".to_string(),
                    },
                ],
            }],
        };

        let json = serde_json::to_string(&mock_list)
            .map_err(|e| format!("序列化印章列表失败: {}", e))?;

        // 编码为 URL-encoded 的 base64 (与现有实现一致)
        let encoded = urlencoding::encode(&base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json.as_bytes(),
        ));

        Ok(encoded)
    }

    /// 获取指定印章的图像数据
    /// 对应 OFD_Plugin.GetSealImage(devId, sealId)
    pub async fn get_seal_image(&self, _dev_id: &str, _seal_id: &str) -> Result<String, String> {
        // FIXME: REPLACE_WITH_REAL_UKEY_PROXY
        // 生产环境通过 WebSocket 发送 ReadSealImage 指令

        // 返回一个模拟的红色圆形印章图像 (1x1 像素的占位图)
        // 实际返回 base64 编码的 PNG 图像
        let mock_png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        Ok(mock_png.to_string())
    }

    /// 获取指定印章的数据
    /// 对应 OFD_Plugin.GetSealData(devId, sealId)
    pub async fn get_seal_data(&self, _dev_id: &str, _seal_id: &str) -> Result<String, String> {
        // FIXME: REPLACE_WITH_REAL_UKEY_PROXY
        // 生产环境通过 WebSocket 发送 ReadSealData 指令

        // 返回模拟的印章数据 (base64)
        let mock_data = "MOCK_SEAL_DATA_FROM_UKEY";
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            mock_data.as_bytes(),
        ))
    }

    /// 获取当前证书公钥
    /// 对应 OFD_Plugin.GetValue("GET_CURRENT_CERT")
    pub fn get_current_cert(&self) -> Option<String> {
        // FIXME: REPLACE_WITH_REAL_UKEY_PROXY
        // 生产环境从 UKey 硬件读取 X.509 证书
        Some(crate::crypto::MOCK_SM2_CERT.to_string())
    }

    /// UKey 硬件签名
    /// 对应 OFD_Plugin.SignData(data, pinCode)
    pub async fn sign_data(&self, data: &str, _pin_code: &str) -> Result<String, String> {
        // FIXME: REPLACE_WITH_REAL_UKEY_PROXY
        // 生产环境通过 WebSocket 发送 SignData 指令到 UKey 硬件
        // UKey 内部使用私钥签名，私钥永不离开硬件设备

        crate::crypto::sm2_sign(data.as_bytes(), &[])
            .map(|sig| crate::crypto::b64_encode(&sig))
    }
}

/// 简单的 URL 编码 (避免额外依赖)
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        for byte in s.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
                | b'-' | b'.' | b'_' | b'~' => result.push(byte as char),
                _ => {
                    result.push('%');
                    result.push_str(&format!("{:02X}", byte));
                }
            }
        }
        result
    }
}

impl Default for UkeyEngine {
    fn default() -> Self {
        Self::new()
    }
}

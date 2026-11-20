# DadaDuck 后续迭代备忘（v0.3.1+）

> 后续取件兼容性事项。公开反馈请使用虚构短信、案号和链接参数。

## 真实链接兼容改进清单

### P1 · 短链和未知平台的浏览器入口
- **涉及模块**：`src-tauri/src/parser.rs`
- **现状痛点**：当前解析器采用硬编码域名白名单，白名单外的各省高院/地方法院自建送达域名（命中 `UnsupportedHost`）、短信缩链以及缺参的新版路由链接（命中 `MissingParams`）会被直接拒收，无法进入内置保底窗口。
- **改进方向**：
  - 保留未识别的短信。提供明确的系统浏览器入口，由用户核对原始网址。
  - 含取件参数的短链由用户在系统浏览器中展开，后续可导入本地文件。

### P1 · PDF.js 在线预览真实地址提取与文件名 URL 解码
- **涉及模块**：`src-tauri/src/browser_fallback.rs`
- **现状痛点**：
  - 部分法院点开文书是 `viewer.html?file=...` 或内嵌 PDF.js `<canvas>` 预览，直接扫 `iframe[src]` 会抓到 HTML 预览页并被魔数校验拒绝。
  - 服务端返回的百分号编码文件名（如 `%E4%BC%A0%E7%A5%A8.pdf`）在 `sanitize_hint_name` 中未做 URL 解码。
- **改进方向**：
  - `window.__dadaduckAssist.scanDocumentLinks()` 增加对 `viewer.html?file=` 参数的解析与还原，提取真实文书 URL 触发拦截。
  - `sanitize_hint_name` 对下载建议名先做 UTF-8 percent-decode 再做路径安全清洗。

### P1 · 验证码输入框识别与短信提取码正则增强
- **涉及模块**：`src-tauri/src/browser_fallback.rs`、`src-tauri/src/parser.rs`
- **现状痛点**：
  - 部分移动端法院 H5 使用 4 格/6 格独立 `<input maxlength="1">` 分段输入框，当前 `findCodeInput()` 仅找单一输入框且偏好 `maxLength` 4~8。
  - 同屏同时存在「图形验证码框」与「短信提取码框」时，`CODE_HINT_RE` 评分相同可能互相误填。
  - `sms_code_regex()` 暂不支持 `提取码【8392】`、`验证码为“123456”` 等带括号或引号包裹的写法。
- **改进方向**：
  - 识别连续多个 `maxlength="1"` 同组输入框并按位拆分填入并逐格触发事件。
  - 区分图形码特征词（图形/图片/右侧字符/captcha）与短信/提取码特征词，手动填入提取码时优先匹配短信码框。
  - 放宽 `sms_code_regex()` 对 `【】`、`「」`、`""`、`''` 等常见中文包裹符号的匹配。

### P2 · 保底窗口手机/桌面视图与 UA 切换
- **涉及模块**：`src-tauri/src/browser_fallback.rs`、`src/components/FallbackControlBar.tsx`
- **现状痛点**：当前保底窗口固定 `1080x780` 桌面尺寸与默认 UA，少数仅适配手机 H5 的送达页面排版偏异常。
- **改进方向**：在主界面保底控制栏增加「切换手机视图」开关（调整窗口宽度并可选切换 Mobile UA）。

---

## 小范围试用期样本收集建议

公开提交问题时请提供 DadaDuck 版本、系统版本、出错步骤和页面类型。只使用虚构姓名、案号、短信、URL 和验证码。可描述短链、分格验证码或 PDF.js 等页面结构。

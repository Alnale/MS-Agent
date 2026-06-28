//! HTTP工具执行器
//!
//! 支持发送HTTP请求到任意URL，具备网页爬取、内容提取、批量搜索能力。
//! 具备浏览器兼容性：真实 UA 轮换、Sec-Fetch 头、TLS 指纹伪装、反爬检测与自动重试。

use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_teams_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error, tool_success,
};
use agent_teams_core::error::Result;

/// Realistic User-Agent pool (Chrome/Firefox/Edge on Windows/Mac/Linux)
const USER_AGENTS: &[&str] = &[
    // Chrome 131 Windows
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    // Chrome 131 Mac
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    // Chrome 130 Windows
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
    // Firefox 133 Windows
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:133.0) Gecko/20100101 Firefox/133.0",
    // Firefox 133 Mac
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:133.0) Gecko/20100101 Firefox/133.0",
    // Edge 131 Windows
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
    // Chrome 131 Linux
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
    // Safari 18 Mac
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Safari/605.1.15",
];

/// Global UA rotation counter
static UA_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Maximum response body size in bytes (512KB) to avoid overwhelming LLM context
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

/// Safely find `needle` in `haystack[start..]` and return absolute byte position.
/// Returns `haystack.len()` if not found or if the computed position exceeds bounds.
fn safe_find(haystack: &str, needle: &str, start: usize) -> usize {
    if start >= haystack.len() {
        return haystack.len();
    }
    haystack[start..].find(needle)
        .map(|p| p + start)
        .unwrap_or(haystack.len())
}

/// Safely find `ch` in `haystack[start..]` and return absolute byte position + 1 (past the char).
/// Returns `haystack.len()` if not found.
fn safe_find_char_past(haystack: &str, ch: char, start: usize) -> usize {
    if start >= haystack.len() {
        return haystack.len();
    }
    haystack[start..].find(ch)
        .map(|p| p + start + 1)
        .unwrap_or(haystack.len())
}

/// Safely slice a string by byte range, clamping to char boundaries.
/// Returns empty string if range is invalid.
fn safe_slice(s: &str, start: usize, end: usize) -> &str {
    let len = s.len();
    if start >= len || start >= end {
        return "";
    }
    let end = end.min(len);
    // Clamp to valid char boundaries
    let mut start = start;
    let mut end = end;
    while start < len && !s.is_char_boundary(start) {
        start += 1;
    }
    while end > start && !s.is_char_boundary(end) {
        end -= 1;
    }
    if start >= end {
        return "";
    }
    &s[start..end]
}

/// HTTP工具执行器
pub struct HttpToolExecutor {
    client: reqwest::Client,
}

impl HttpToolExecutor {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(20)
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::USER_AGENT,
                    reqwest::header::HeaderValue::from_static(USER_AGENTS[0]),
                );
                headers.insert(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7"),
                );
                headers.insert(
                    reqwest::header::ACCEPT_LANGUAGE,
                    reqwest::header::HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8,en-US;q=0.7"),
                );
                // Do NOT set Accept-Encoding — let reqwest handle decompression
                // automatically via its built-in gzip/brotli/deflate/zstd support.
                // Setting this header manually can cause double-compression issues.
                headers.insert(
                    reqwest::header::CACHE_CONTROL,
                    reqwest::header::HeaderValue::from_static("no-cache"),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("sec-ch-ua"),
                    reqwest::header::HeaderValue::from_static(r#""Chromium";v="131", "Not_A Brand";v="24""#),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("sec-ch-ua-mobile"),
                    reqwest::header::HeaderValue::from_static("?0"),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("sec-ch-ua-platform"),
                    reqwest::header::HeaderValue::from_static(r#""Windows""#),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("sec-fetch-dest"),
                    reqwest::header::HeaderValue::from_static("document"),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("sec-fetch-mode"),
                    reqwest::header::HeaderValue::from_static("navigate"),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("sec-fetch-site"),
                    reqwest::header::HeaderValue::from_static("none"),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("sec-fetch-user"),
                    reqwest::header::HeaderValue::from_static("?1"),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("upgrade-insecure-requests"),
                    reqwest::header::HeaderValue::from_static("1"),
                );
                headers.insert(
                    reqwest::header::HeaderName::from_static("priority"),
                    reqwest::header::HeaderValue::from_static("u=0, i"),
                );
                headers
            })
            .build()
            .unwrap_or_else(|e| {
                tracing::error!("Failed to build HTTP client: {}, using default", e);
                reqwest::Client::new()
            });
        Self { client }
    }

    /// Get next User-Agent from rotation pool
    fn next_user_agent() -> &'static str {
        let idx = UA_INDEX.fetch_add(1, Ordering::Relaxed) % USER_AGENTS.len();
        USER_AGENTS[idx]
    }

    /// Percent-encode a string for use in URLs (RFC 3986)
    fn percent_encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                // Unreserved characters
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                // Space -> +
                b' ' => {
                    result.push('+');
                }
                // Everything else gets percent-encoded
                _ => {
                    result.push('%');
                    result.push_str(&format!("{:02X}", byte));
                }
            }
        }
        result
    }
}

impl Default for HttpToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for HttpToolExecutor {
    fn executor_id(&self) -> &str {
        "http"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("http_request")
                .description(concat!(
                    "发送HTTP请求，支持搜索、爬取网页、调用API、提取JS渲染数据。\n",
                    "⚠️ 关键规则：当用户说「搜索/搜一下/查一下/找一下/帮我找/帮我搜/look up/search」时，必须使用 search 参数，绝对不要用 url 参数！\n",
                    "  ✅ 正确：http_request(search=\"B站发展历程\")\n",
                    "  ❌ 错误：http_request(url=\"https://bilibili.com\") ← 这会爬主页，不是搜索！\n\n",
                    "核心能力：\n",
                    "- 网页搜索：search 参数传入关键词，自动使用多个搜索引擎（百度、Bing、Google）并发搜索\n",
                    "- 单URL请求：url 参数传入完整URL（含路径），GET/POST/PUT/PATCH/DELETE\n",
                    "- 批量请求：urls 参数传入多个完整URL，并发请求后合并结果\n",
                    "- 内容提取 extract 参数：\n",
                    "  none=原样返回\n",
                    "  text=纯文本(去除HTML标签)\n",
                    "  links=提取所有链接\n",
                    "  meta=提取标题/描述/关键词\n",
                    "  jsdata=提取JS嵌入数据(Next.js/Nuxt/Vue/React等框架的SSR数据、API端点、框架检测)\n",
                    "  all=全部提取(含jsdata)\n",
                    "- 自动设置浏览器 User-Agent，支持爬取大多数网站\n",
                    "- 响应超过 512KB 自动截断\n\n",
                    "参数选择指南：\n",
                    "- 用户要搜索信息 → search=\"关键词\"（关键词从用户原话提取，不要编造URL）\n",
                    "- 用户给出具体URL要访问 → url=\"完整URL\"\n",
                    "- 用户要同时访问多个页面 → urls=[\"url1\", \"url2\"]\n\n",
                    "JS数据提取：对SPA/SSR页面，jsdata模式从script标签中提取嵌入的JSON数据和API端点\n",
                    "禁止用途：不要用于翻译、词典查询，模型自身知识足以处理翻译需求\n\n",
                    "工具联动：\n",
                    "- 爬取的网页内容可用 file(write) 保存到本地\n",
                    "- 下载的文档文件可用 docreader 读取内容\n",
                    "- 下载的文档文件可用 docflow 转换格式\n",
                    "- 搜索结果可用于 xxt 的答案生成",
                ))
                .executor("http")
                .tag("network")
                .tag("api")
                .tag("web")
                .tag("search")
                .tag("crawl")
                .tag("js")
                .timeout(120_000)
                .param_string("url", "单个请求URL", false)
                .param_array("urls", "批量请求URL列表（用于搜索多个来源），与 url 二选一", false)
                .param_string("search", "搜索关键词（自动使用百度、Bing、Google多引擎搜索），与 url/urls 三选一", false)
                .param_integer("num_results", "搜索时每个引擎返回的结果数量，默认5", false)
                .param_enum("method", "HTTP方法", &["GET", "POST", "PUT", "PATCH", "DELETE"], false)
                .param_object("body", "请求体（POST/PUT/PATCH时使用，支持JSON对象）", false)
                .param_object("headers", "自定义HTTP头（会合并到默认浏览器头之上）", false)
                .param_integer("timeout", "超时时间（秒），默认120", false)
                .param_enum("extract", "内容提取模式：none=原样返回, text=纯文本, links=链接, meta=元信息, jsdata=JS嵌入数据+API端点+框架检测, all=全部(含jsdata)", &["none", "text", "links", "meta", "jsdata", "all"], false)
                .param_integer("max_size", "最大响应字节数，默认524288(512KB)", false)
                .data_flow("search: 输入关键词字符串，输出搜索结果（标题+摘要+URL列表）")
                .data_flow("url/urls: 输入URL，输出网页内容/JSON数据")
                .data_flow("下载文件后可用 file(write) 保存，或用 docreader/docflow 处理文档")
                .output_field("results: 搜索结果数组（含 title/url/snippet）")
                .output_field("content: 网页内容或API响应")
                .output_field("url: 最终请求的URL")
                .output_field("status: HTTP状态码")
                .build(),
            ToolBuilder::new("http_get")
                .description(concat!(
                    "发送GET请求到指定URL。简化版的http_request，适用于获取资源。\n",
                    "工具联动：获取的内容可用 file(write) 保存，下载的文档可用 docreader 读取"
                ))
                .executor("http")
                .tag("network")
                .tag("api")
                .tag("web")
                .timeout(60_000)
                .param_string("url", "请求的完整URL", true)
                .param_object("headers", "自定义HTTP头", false)
                .data_flow("输入URL，输出响应内容")
                .data_flow("下载文件后可用 file(write) 保存，或用 docreader/docflow 处理文档")
                .output_field("content: 响应内容")
                .output_field("status: HTTP状态码")
                .build(),
            ToolBuilder::new("http_post")
                .description(concat!(
                    "发送POST请求到指定URL。简化版的http_request，适用于提交数据。\n",
                    "工具联动：当body很大时，可先用 file(write) 写入临时文件再读取"
                ))
                .executor("http")
                .tag("network")
                .tag("api")
                .tag("web")
                .timeout(60_000)
                .param_string("url", "请求的完整URL", true)
                .param_object("body", "请求体（JSON对象）", true)
                .param_object("headers", "自定义HTTP头", false)
                .data_flow("输入URL和JSON body，输出响应内容")
                .data_flow("当body很大时，可先用 file(write) 将数据写入文件，再从文件读取构造body")
                .output_field("content: 响应内容")
                .output_field("status: HTTP状态码")
                .build(),
        ]
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        let start = std::time::Instant::now();

        let result = match call.name.as_str() {
            "http_request" => self.execute_request(call).await,
            "http_get" => self.execute_get(call).await,
            "http_post" => self.execute_post(call).await,
            "web_search" => {
                // Backward compatibility: convert web_search to http_request with search param
                let mut args = call.arguments.clone();
                let query = args.get("query").and_then(|v| v.as_str()).map(|s| s.to_string());
                if let Some(q) = query {
                    args.as_object_mut().map(|o| o.insert("search".to_string(), serde_json::json!(q)));
                }
                let converted = ToolCall { arguments: args, ..call.clone() };
                self.execute_request(&converted).await
            },
            _ => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call,
                    format!("未知的 HTTP 工具: {}", call.name),
                    format!("工具名 '{}' 未注册", call.name),
                    "请使用 http_request、http_get、http_post 或 web_search",
                    ms,
                ));
            }
        };

        let ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(output) => Ok(tool_success(call, output, ms)),
            Err(e) => {
                let err_str = e.to_string();
                let (details, suggestion) = if err_str.contains("timeout") || err_str.contains("timed out") {
                    (err_str.clone(), "请求超时。请检查 URL 是否可达，或增大 timeout 参数")
                } else if err_str.contains("connect") {
                    (err_str.clone(), "连接失败。请检查 URL 是否正确、目标服务是否运行")
                } else if err_str.contains("dns") || err_str.contains("resolve") {
                    (err_str.clone(), "DNS 解析失败。请检查域名是否正确")
                } else {
                    (err_str.clone(), "请检查 URL 格式和网络连接")
                };
                Ok(tool_error(call, format!("HTTP 请求失败: {}", e), details, suggestion, ms))
            }
        }
    }
}

/// Extracted content from an HTML page
struct ExtractedContent {
    title: Option<String>,
    description: Option<String>,
    keywords: Option<String>,
    links: Vec<Link>,
    text: String,
    js_data: JsExtractedData,
    platform: PlatformInfo,
    structured: StructuredData,
    tables: Vec<TableData>,
    headings: Vec<Heading>,
    images: Vec<ImageInfo>,
}

struct Link {
    href: String,
    text: String,
}

/// Structured data extracted from HTML (JSON-LD, OpenGraph, Twitter Cards, Schema.org microdata)
struct StructuredData {
    /// JSON-LD structured data blocks (parsed from <script type="application/ld+json">)
    json_ld: Vec<serde_json::Value>,
    /// OpenGraph metadata
    og: std::collections::HashMap<String, String>,
    /// Twitter Card metadata
    twitter: std::collections::HashMap<String, String>,
    /// Schema.org microdata items
    microdata: Vec<serde_json::Value>,
}

/// HTML table extracted as structured data
struct TableData {
    /// Table caption (if any)
    caption: Option<String>,
    /// Header row
    headers: Vec<String>,
    /// Data rows
    rows: Vec<Vec<String>>,
}

/// Heading with level and text
struct Heading {
    level: u8,
    text: String,
}

/// Image with src, alt, and context
struct ImageInfo {
    src: String,
    alt: String,
    title: Option<String>,
}

/// Detected backend platform / CMS / framework info
struct PlatformInfo {
    /// Backend language: PHP, Python, Ruby, Java, C#, Node.js, Go
    language: Option<String>,
    /// Framework / CMS: WordPress, Django, Laravel, Rails, Spring, Express, etc.
    framework: Option<String>,
    /// Web server: nginx, Apache, IIS, Gunicorn, etc.
    server: Option<String>,
    /// Database hints from cookies/headers
    db_hint: Option<String>,
    /// CMS-specific: theme name, plugin hints, etc.
    cms_details: Vec<String>,
}

/// Data extracted from JavaScript in the page
struct JsExtractedData {
    /// Embedded JSON data from script tags (framework SSR data, initial state, etc.)
    embedded_data: Vec<serde_json::Value>,
    /// API endpoints found in JS code (fetch/axios/XHR calls)
    api_endpoints: Vec<String>,
    /// Detected frontend framework
    framework: Option<String>,
    /// Inline script contents (truncated, for debugging)
    inline_scripts: Vec<String>,
}

impl HttpToolExecutor {
    /// Decode response bytes to UTF-8 string, respecting Content-Type charset.
    /// Supports UTF-8, GBK, GB2312, GB18030, Big5, Shift_JIS, EUC-KR, ISO-8859-1.
    fn decode_response(data: &[u8], content_type: &str) -> String {
        // Extract charset from Content-Type header
        let charset = Self::extract_charset(content_type);

        // If charset is UTF-8 or not specified, try UTF-8 first
        if charset.as_deref().unwrap_or("utf-8") == "utf-8"
            || charset.as_deref() == Some("utf8")
            || charset.is_none()
        {
            if let Ok(s) = std::str::from_utf8(data) {
                return s.to_string();
            }
            // UTF-8 failed, fall through to other encodings
        }

        // Try encoding_rs for non-UTF-8 encodings
        let encoding = match charset.as_deref() {
            Some("gbk") | Some("gb2312") | Some("gb18030") => encoding_rs::GBK,
            Some("big5") => encoding_rs::BIG5,
            Some("shift_jis") | Some("sjis") | Some("shift-jis") => encoding_rs::SHIFT_JIS,
            Some("euc-kr") => encoding_rs::EUC_KR,
            Some("iso-8859-1") | Some("latin1") | Some("latin-1") => encoding_rs::UTF_8, // fallback
            Some("windows-1252") | Some("cp1252") => encoding_rs::UTF_8, // fallback
            _ => {
                // Unknown charset, try auto-detection
                return Self::auto_decode(data);
            }
        };

        let (decoded, _, had_errors) = encoding.decode(data);
        if had_errors {
            tracing::warn!("Encoding {} had decode errors for {} bytes", encoding.name(), data.len());
        }
        decoded.into_owned()
    }

    /// Extract charset from Content-Type header value
    fn extract_charset(content_type: &str) -> Option<String> {
        for part in content_type.split(';') {
            let part = part.trim();
            if let Some(rest) = part.strip_prefix("charset=") {
                return Some(rest.trim().trim_matches('"').to_ascii_lowercase());
            }
        }
        None
    }

    /// Auto-detect encoding by trying common encodings in order
    fn auto_decode(data: &[u8]) -> String {
        // Try UTF-8 first
        if let Ok(s) = std::str::from_utf8(data) {
            return s.to_string();
        }
        // Try GBK (most common for Chinese websites)
        let (decoded, _encoding, _had_errors) = encoding_rs::GBK.decode(data);
        decoded.into_owned()
    }

    /// Try to decompress response bytes if they are gzip/deflate/brotli/zstd compressed.
    /// Returns None if the data is not compressed or decompression fails.
    fn try_decompress(data: &[u8]) -> Option<Vec<u8>> {
        // gzip magic bytes: 0x1f 0x8b
        if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
            tracing::info!("Detected gzip-compressed response ({} bytes), decompressing...", data.len());
            match Self::decompress_gzip(data) {
                Ok(decompressed) => {
                    tracing::info!("gzip decompression successful: {} -> {} bytes", data.len(), decompressed.len());
                    return Some(decompressed);
                }
                Err(e) => {
                    tracing::warn!("gzip decompression failed: {}", e);
                }
            }
        }

        // zlib/deflate magic bytes: 0x78 0x01 (low), 0x78 0x5E (default), 0x78 0x9C (best), 0x78 0xDA (best compression)
        if data.len() >= 2 && data[0] == 0x78 && matches!(data[1], 0x01 | 0x5E | 0x9C | 0xDA) {
            tracing::info!("Detected zlib/deflate-compressed response ({} bytes), decompressing...", data.len());
            match Self::decompress_deflate(data) {
                Ok(decompressed) => {
                    tracing::info!("deflate decompression successful: {} -> {} bytes", data.len(), decompressed.len());
                    return Some(decompressed);
                }
                Err(e) => {
                    tracing::warn!("deflate decompression failed: {}", e);
                }
            }
        }

        // brotli doesn't have a simple magic number, but often starts with specific patterns
        // zstd magic bytes: 0x28 0xB5 0x2F 0xFD
        if data.len() >= 4 && data[0] == 0x28 && data[1] == 0xB5 && data[2] == 0x2F && data[3] == 0xFD {
            tracing::warn!("Detected zstd-compressed response but zstd decompression is not available inline");
        }

        None
    }

    /// Decompress gzip data using flate2
    fn decompress_gzip(data: &[u8]) -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }

    /// Decompress zlib/deflate data using flate2
    fn decompress_deflate(data: &[u8]) -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let mut decoder = flate2::read::ZlibDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)?;
        Ok(decompressed)
    }

    /// Build request with common options (headers, timeout, redirect)
    fn build_request(
        &self,
        method: &str,
        url: &str,
        headers: Option<&serde_json::Map<String, serde_json::Value>>,
        timeout: u64,
    ) -> reqwest::RequestBuilder {
        let mut builder = match method.to_uppercase().as_str() {
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "PATCH" => self.client.patch(url),
            "DELETE" => self.client.delete(url),
            _ => self.client.get(url),
        };

        if timeout != 120 {
            builder = builder.timeout(std::time::Duration::from_secs(timeout));
        }

        // Rotate User-Agent per request for anti-bot evasion
        let ua = Self::next_user_agent();
        builder = builder.header(reqwest::header::USER_AGENT, ua);

        // POST/PUT/PATCH need different Sec-Fetch headers
        if method.eq_ignore_ascii_case("POST") || method.eq_ignore_ascii_case("PUT") || method.eq_ignore_ascii_case("PATCH") {
            builder = builder.header("sec-fetch-dest", "empty");
            builder = builder.header("sec-fetch-mode", "cors");
            builder = builder.header("sec-fetch-site", "same-origin");
        }

        if let Some(hdrs) = headers {
            for (k, v) in hdrs {
                if let Some(val) = v.as_str() {
                    builder = builder.header(k.as_str(), val);
                }
            }
        }

        builder
    }

    /// Detect anti-bot/anti-scraping responses
    fn detect_anti_bot(status: u16, headers: &std::collections::HashMap<String, String>, body: &str) -> Option<&'static str> {
        match status {
            403 => {
                let body_lower = body.to_ascii_lowercase();
                if body_lower.contains("cloudflare") || body_lower.contains("cf-browser-verification") {
                    return Some("Cloudflare 403 challenge");
                }
                if body_lower.contains("captcha") || body_lower.contains("recaptcha") || body_lower.contains("hcaptcha") {
                    return Some("CAPTCHA required");
                }
                if body_lower.contains("access denied") || body_lower.contains("forbidden") {
                    return Some("Access denied (403)");
                }
                Some("Forbidden (403)")
            }
            429 => Some("Rate limited (429)"),
            503 => {
                let body_lower = body.to_ascii_lowercase();
                if body_lower.contains("cloudflare") || body_lower.contains("checking your browser") {
                    return Some("Cloudflare browser check");
                }
                if body_lower.contains("ddos") || body_lower.contains("security") {
                    return Some("DDoS protection");
                }
                Some("Service unavailable (503)")
            }
            _ => {
                // Check for Cloudflare challenge in 200 responses
                if status == 200 {
                    let body_lower = body.to_ascii_lowercase();
                    if body_lower.contains("cf-challenge") || body_lower.contains("challenge-platform") {
                        return Some("Cloudflare JS challenge");
                    }
                    if body_lower.contains("just a moment") && body_lower.contains("cloudflare") {
                        return Some("Cloudflare 'Just a moment' page");
                    }
                    if body_lower.contains("attention required") && body_lower.contains("cloudflare") {
                        return Some("Cloudflare attention required");
                    }
                    // Check for WAF/bot detection pages
                    if body_lower.contains("please verify you are human") || body_lower.contains("please enable javascript") && body_lower.len() < 3000 {
                        return Some("Bot detection page");
                    }
                }
                // Check headers for anti-bot
                if let Some(server) = headers.get("server") {
                    if server.to_ascii_lowercase().contains("cloudflare") && (status == 403 || status == 503) {
                        return Some("Cloudflare protection");
                    }
                }
                if headers.get("x-sucuri-id").is_some() {
                    return Some("Sucuri WAF");
                }
                if headers.get("x-sucuri-cache").is_some() {
                    return Some("Sucuri WAF");
                }
                None
            }
        }
    }

    /// Execute a single HTTP request and return structured response
    #[allow(clippy::too_many_arguments)]
    async fn execute_single(
        &self,
        url: &str,
        method: &str,
        body: Option<&serde_json::Value>,
        headers: Option<&serde_json::Map<String, serde_json::Value>>,
        timeout: u64,
        extract_mode: &str,
        max_size: usize,
    ) -> Result<serde_json::Value> {
        let mut builder = self.build_request(method, url, headers, timeout);

        if let Some(b) = body {
            builder = builder.json(b);
        }

        let resp = builder.send().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Provider(format!("HTTP请求失败: {}", e)))?;

        let status = resp.status().as_u16();
        let resp_headers: std::collections::HashMap<String, String> = resp.headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let content_type = resp_headers.get("content-type").cloned().unwrap_or_default();
        let is_html = content_type.contains("text/html") || content_type.contains("application/xhtml");

        let resp_bytes = resp.bytes().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Provider(format!("读取响应失败: {}", e)))?;

        // Step 1: Decompress if needed (gzip/deflate/zlib)
        let decompressed = Self::try_decompress(&resp_bytes);
        let data = decompressed.as_deref().unwrap_or(&resp_bytes);

        // Step 2: Decode bytes to UTF-8 string, respecting Content-Type charset
        let raw_text = Self::decode_response(data, &content_type);

        // Detect if the response is garbled (binary data misinterpreted as text).
        let is_garbled = {
            let total = raw_text.chars().count();
            if total == 0 {
                false
            } else {
                let control_count = raw_text.chars().filter(|c| {
                    c.is_control() && !matches!(c, '\n' | '\r' | '\t')
                }).count();
                control_count * 100 / total > 10
            }
        };

        if is_garbled {
            tracing::warn!(
                "Response from {} still garbled after decompression ({} bytes, content-type: {}). \
                 May be an unsupported encoding or binary content.",
                url, data.len(), content_type
            );
        }

        // Detect anti-bot/anti-scraping
        let anti_bot = Self::detect_anti_bot(status, &resp_headers, &raw_text);

        // Truncate if too large
        let text = if raw_text.len() > max_size {
            let mut end = max_size;
            while end > 0 && !raw_text.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...(truncated, {} bytes total)", &raw_text[..end], raw_text.len())
        } else {
            raw_text
        };

        // Build base response
        let mut result = serde_json::json!({
            "status": status,
            "url": url,
            "method": method,
            "content_type": content_type,
            "size_bytes": text.len(),
            "encoding_ok": !is_garbled,
        });

        // Include anti-bot detection result
        if let Some(block_type) = anti_bot {
            result["anti_bot_detected"] = serde_json::json!(true);
            result["anti_bot_type"] = serde_json::json!(block_type);
            result["anti_bot_suggestion"] = serde_json::json!(match block_type {
                t if t.contains("Cloudflare") => "网站使用 Cloudflare 保护，需要浏览器环境才能访问。建议：1) 使用 xxt 工具通过浏览器访问 2) 检查是否有可用的 API 端点 3) 尝试添加 Referer 头",
                t if t.contains("CAPTCHA") => "网站需要验证码验证，无法自动绕过。建议使用 xxt 工具通过浏览器手动操作",
                t if t.contains("Rate limit") => "请求频率被限制，建议增大请求间隔或使用不同的 User-Agent",
                t if t.contains("Sucuri") => "网站使用 Sucuri WAF 保护，需要浏览器环境",
                t if t.contains("Bot detection") => "检测到机器人，建议使用 xxt 工具通过浏览器访问",
                _ => "网站有反爬保护，建议使用 xxt 工具通过浏览器访问",
            });
        } else {
            result["anti_bot_detected"] = serde_json::json!(false);
        }

        // Apply content extraction based on mode
        if is_html && extract_mode != "none" {
            // Wrap HTML extraction in catch_unwind to prevent panics from malformed HTML
            let extracted = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::extract_html_content(&text, &resp_headers)
            })) {
                Ok(e) => e,
                Err(_) => {
                    tracing::warn!("HTML extraction panicked for {}, falling back to raw text", url);
                    ExtractedContent {
                        title: None, description: None, keywords: None,
                        links: Vec::new(), text: Self::strip_tags(&text),
                        js_data: JsExtractedData {
                            embedded_data: Vec::new(),
                            api_endpoints: Vec::new(),
                            framework: None,
                            inline_scripts: Vec::new(),
                        },
                        platform: PlatformInfo {
                            language: None, framework: None, server: None,
                            db_hint: None, cms_details: Vec::new(),
                        },
                        structured: StructuredData {
                            json_ld: Vec::new(),
                            og: std::collections::HashMap::new(),
                            twitter: std::collections::HashMap::new(),
                            microdata: Vec::new(),
                        },
                        tables: Vec::new(), headings: Vec::new(), images: Vec::new(),
                    }
                }
            };

            match extract_mode {
                "text" => {
                    result["text"] = serde_json::Value::String(extracted.text);
                }
                "links" => {
                    result["links"] = serde_json::json!(extracted.links.iter().map(|l| {
                        serde_json::json!({"href": l.href, "text": l.text})
                    }).collect::<Vec<_>>());
                    result["link_count"] = serde_json::json!(extracted.links.len());
                }
                "meta" => {
                    result["title"] = serde_json::json!(extracted.title);
                    result["description"] = serde_json::json!(extracted.description);
                    result["keywords"] = serde_json::json!(extracted.keywords);
                }
                "jsdata" => {
                    Self::fill_jsdata_fields(&mut result, &extracted);
                }
                "all" => {
                    result["title"] = serde_json::json!(extracted.title);
                    result["description"] = serde_json::json!(extracted.description);
                    result["keywords"] = serde_json::json!(extracted.keywords);
                    Self::fill_jsdata_fields(&mut result, &extracted);
                    result["text"] = serde_json::Value::String(extracted.text);
                    result["links"] = serde_json::json!(extracted.links.iter().map(|l| {
                        serde_json::json!({"href": l.href, "text": l.text})
                    }).collect::<Vec<_>>());
                    result["link_count"] = serde_json::json!(extracted.links.len());
                }
                _ => {
                    // "none" or unknown — try parse as JSON, else return raw
                    let json = serde_json::from_str(&text)
                        .unwrap_or(serde_json::json!({"raw": text}));
                    result["body"] = json;
                }
            }
        } else {
            // Not HTML or extract_mode == "none" — try JSON parse
            let json = serde_json::from_str(&text)
                .unwrap_or(serde_json::Value::String(text));
            result["body"] = json;
            result["headers"] = serde_json::json!(resp_headers);
        }

        Ok(result)
    }

    /// 执行通用HTTP请求（支持单URL和批量URL）
    async fn execute_request(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let method = call.arguments["method"].as_str().unwrap_or("GET");
        let body = call.arguments.get("body").cloned();
        let timeout = call.arguments["timeout"].as_u64().unwrap_or(120);
        let headers = call.arguments.get("headers").and_then(|h| h.as_object());
        let extract_mode = call.arguments["extract"].as_str().unwrap_or("all");
        let max_size = call.arguments["max_size"].as_u64().unwrap_or(MAX_RESPONSE_BYTES as u64) as usize;

        // Check for search query — auto-construct search engine URLs, then crawl result pages
        if let Some(query) = call.arguments["search"].as_str() {
            return self.execute_search(query, call, headers, timeout, max_size).await;
        }

        // Check for batch URLs
        if let Some(urls_val) = call.arguments.get("urls") {
            if let Some(urls) = urls_val.as_array() {
                if !urls.is_empty() {
                    return self.execute_batch(urls, method, body.as_ref(), headers, timeout, extract_mode, max_size).await;
                }
            }
        }

        // Single URL
        let url = call.arguments["url"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound(
                "缺少必需参数 'url' 或 'urls'。请提供请求 URL".to_string()
            ))?;

        // Fallback: if URL is just a homepage (no path beyond domain), redirect to search
        // e.g. "https://bilibili.com" or "https://www.baidu.com/" → search for domain name
        if let Some(stripped) = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")) {
            let after_scheme = stripped.strip_prefix("//").unwrap_or(stripped);
            // Find where path starts (after domain)
            let domain_end = after_scheme.find('/').unwrap_or(after_scheme.len());
            let domain_part = &after_scheme[..domain_end];
            // Find where path ends (before query/hash)
            let path_and_rest = &after_scheme[domain_end..];
            let path = path_and_rest.split('?').next().unwrap_or("").split('#').next().unwrap_or("");
            let path_clean = path.trim_end_matches('/');

            if path_clean.is_empty() && !domain_part.is_empty() {
                let query = domain_part.strip_prefix("www.").unwrap_or(domain_part);
                tracing::info!("http_request: homepage URL detected ({}), redirecting to search for '{}'", url, query);
                return self.execute_search(query, call, headers, timeout, max_size).await;
            }
        }

        self.execute_single(url, method, body.as_ref(), headers, timeout, extract_mode, max_size).await
    }

    /// Execute multi-engine search: fetch search pages, extract URLs, crawl top results
    async fn execute_search(
        &self,
        query: &str,
        call: &ToolCall,
        headers: Option<&serde_json::Map<String, serde_json::Value>>,
        timeout: u64,
        max_size: usize,
    ) -> Result<serde_json::Value> {
        let num_results = call.arguments["num_results"].as_u64().unwrap_or(10) as usize;
        let encoded = Self::percent_encode(query);
        let search_urls = vec![
            serde_json::json!(format!("https://www.baidu.com/s?wd={}&rn={}&ie=utf-8", encoded, num_results)),
            serde_json::json!(format!("https://www.bing.com/search?q={}&count={}", encoded, num_results)),
            serde_json::json!(format!("https://www.google.com/search?q={}&num={}", encoded, num_results)),
        ];

        // Step 1: Fetch search engine pages with full extraction (text + links)
        let search_result = self.execute_batch(&search_urls, "GET", None, headers, timeout, "all", max_size).await?;

        // Step 1.5: Filter out engines with anti-bot detection
        let filtered_result = Self::filter_anti_bot_results(&search_result);

        // Step 2: Extract result page URLs from search engine pages (with relevance filtering)
        let result_urls = Self::extract_result_urls_filtered(&filtered_result, num_results * 2, query);

        // Step 3: Crawl the top result pages
        if !result_urls.is_empty() {
            let crawl_urls: Vec<serde_json::Value> = result_urls.iter()
                .take(num_results)
                .map(|u| serde_json::json!(u))
                .collect();
            let crawled = self.execute_batch(&crawl_urls, "GET", None, headers, timeout, "text", max_size).await?;

            // Merge: search snippets + crawled page content
            let mut combined = filtered_result.clone();
            if let Some(crawled_results) = crawled.get("results").and_then(|r| r.as_array()) {
                let crawled_texts: Vec<&str> = crawled_results.iter()
                    .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
                    .collect();
                if !crawled_texts.is_empty() {
                    combined["crawled_pages"] = serde_json::json!(crawled_texts.len());
                    combined["crawled_content"] = serde_json::Value::String(
                        crawled_texts.join("\n\n---\n\n")
                    );
                }
            }
            return Ok(combined);
        }

        Ok(filtered_result)
    }

    /// Batch fetch multiple URLs concurrently
    #[allow(clippy::too_many_arguments)]
    async fn execute_batch(
        &self,
        urls: &[serde_json::Value],
        method: &str,
        body: Option<&serde_json::Value>,
        headers: Option<&serde_json::Map<String, serde_json::Value>>,
        timeout: u64,
        extract_mode: &str,
        max_size: usize,
    ) -> Result<serde_json::Value> {
        let futures: Vec<_> = urls.iter().filter_map(|u| {
            let url = u.as_str()?.to_string();
            let method = method.to_string();
            let extract_mode = extract_mode.to_string();
            // Clone headers for each concurrent request
            let headers_json = headers.map(|h| serde_json::json!(h));
            let body = body.cloned();
            Some(async move {
                let headers_ref = headers_json.as_ref().and_then(|v| v.as_object());
                let result = self.execute_single(
                    &url, &method, body.as_ref(), headers_ref, timeout, &extract_mode, max_size,
                ).await;
                (url, result)
            })
        }).collect();

        let results = futures::future::join_all(futures).await;

        let mut responses = Vec::new();
        let mut errors = Vec::new();
        for (url, result) in results {
            match result {
                Ok(val) => responses.push(val),
                Err(e) => errors.push(serde_json::json!({"url": url, "error": e.to_string()})),
            }
        }

        // For search use-case: merge extracted text from all successful responses
        let merged_text: String = responses.iter()
            .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        let merged_links: Vec<serde_json::Value> = responses.iter()
            .filter_map(|r| r.get("links").and_then(|l| l.as_array()))
            .flat_map(|arr| arr.iter().cloned())
            .collect();

        let merged_titles: Vec<String> = responses.iter()
            .filter_map(|r| r.get("title").and_then(|t| t.as_str()).map(|s| s.to_string()))
            .collect();

        let mut result = serde_json::json!({
            "batch": true,
            "total": responses.len() + errors.len(),
            "success": responses.len(),
            "failed": errors.len(),
            "results": responses,
        });

        if !errors.is_empty() {
            result["errors"] = serde_json::json!(errors);
        }

        // Add merged fields for search convenience
        if !merged_text.is_empty() {
            // Truncate merged text
            let text = if merged_text.len() > max_size {
                let mut end = max_size;
                while end > 0 && !merged_text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...(truncated)", &merged_text[..end])
            } else {
                merged_text
            };
            result["merged_text"] = serde_json::Value::String(text);
        }
        if !merged_links.is_empty() {
            result["merged_links"] = serde_json::json!(merged_links);
        }
        if !merged_titles.is_empty() {
            result["merged_titles"] = serde_json::json!(merged_titles);
        }

        Ok(result)
    }

    /// 执行GET请求
    async fn execute_get(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let url = call.arguments["url"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("url参数不能为空".to_string()))?;

        let headers = call.arguments.get("headers").and_then(|h| h.as_object());
        let builder = self.build_request("GET", url, headers, 60);

        let resp = builder.send().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Provider(format!("HTTP GET失败: {}", e)))?;

        let status = resp.status().as_u16();
        let text = resp.text().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Provider(format!("读取响应失败: {}", e)))?;
        let json = serde_json::from_str(&text)
            .unwrap_or(serde_json::json!({"raw": text}));

        Ok(serde_json::json!({
            "status": status,
            "body": json,
            "url": url
        }))
    }

    /// 执行POST请求
    async fn execute_post(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let url = call.arguments["url"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("url参数不能为空".to_string()))?;
        let body = call.arguments.get("body")
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("body参数不能为空".to_string()))?;

        let headers = call.arguments.get("headers").and_then(|h| h.as_object());
        let mut builder = self.build_request("POST", url, headers, 60);
        builder = builder.json(body);

        let resp = builder.send().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Provider(format!("HTTP POST失败: {}", e)))?;

        let status = resp.status().as_u16();
        let text = resp.text().await
            .map_err(|e| agent_teams_core::error::AgentTeamsError::Provider(format!("读取响应失败: {}", e)))?;
        let json = serde_json::from_str(&text)
            .unwrap_or(serde_json::json!({"raw": text}));

        Ok(serde_json::json!({
            "status": status,
            "body": json,
            "url": url
        }))
    }

    /// Filter out search engine results that have anti-bot detection.
    /// Keeps only results from engines that returned valid content.
    fn filter_anti_bot_results(search_result: &serde_json::Value) -> serde_json::Value {
        let mut filtered = search_result.clone();

        if let Some(results) = filtered.get_mut("results").and_then(|r| r.as_array_mut()) {
            let original_count = results.len();
            results.retain(|r| {
                // Keep results that have valid content (links or text)
                let has_links = r.get("links")
                    .and_then(|l| l.as_array())
                    .map(|a| !a.is_empty())
                    .unwrap_or(false);
                let has_text = r.get("text")
                    .and_then(|t| t.as_str())
                    .map(|t| t.len() > 100)
                    .unwrap_or(false);
                let encoding_ok = r.get("encoding_ok")
                    .and_then(|e| e.as_bool())
                    .unwrap_or(true);
                let anti_bot = r.get("anti_bot_detected")
                    .and_then(|a| a.as_bool())
                    .unwrap_or(false);

                // Keep if: has content AND no anti-bot AND encoding is OK
                (has_links || has_text) && !anti_bot && encoding_ok
            });

            let kept = results.len();
            if kept < original_count {
                tracing::info!(
                    "Filtered search results: kept {}/{} engines (removed {} with anti-bot or bad encoding)",
                    kept, original_count, original_count - kept
                );
            }
        }

        filtered
    }

    /// Extract result page URLs with relevance filtering against the original query.
    /// Scores URLs by keyword matches and cross-engine deduplication.
    fn extract_result_urls_filtered(search_result: &serde_json::Value, max_urls: usize, query: &str) -> Vec<String> {
        // Search engine and irrelevant domains to exclude
        let excluded_domains = [
            // Search engine internals
            "baidu.com/s", "baidu.com/cache", "baidu.com/link",
            "bing.com/search", "bing.com/images", "bing.com/videos",
            "google.com/search", "google.com/url", "google.com/imgres",
            "googleapis.com", "gstatic.com", "googleusercontent.com",
            // Translation / dictionary sites
            "iciba.com", "dict.baidu.com", "fanyi.baidu.com", "fanyi.",
            "youdao.com", "dict.youdao.com", "translate.google",
            "huangdao.com", "haici.com", "dict.cn", "zdic.net",
            "etymonline.com", "merriam-webster.com", "dictionary.com",
            "cambridge.org/dictionary", "oxfordlearnersdictionaries.com",
            // Ad / tracking / redirect domains
            "doubleclick.net", "googlesyndication.com", "ads.", "ad.",
            "click.baidu.com", "pos.baidu.com", "cpro.baidu.com",
            // Finance / stock portals (often have widget links that pollute results)
            "xueqiu.com", "eastmoney.com", "guba.eastmoney.com",
            "finance.sina.com", "finance.sina.cn", "stock.163.com",
            "finance.qq.com", "stockpage.10jqka.com", "quote.eastmoney.com",
            "guba.sina.com", "biz.baidu.com", "caifu.baidu.com",
            // Social media / app download pages (often low quality for informational queries)
            "zhihu.com/signin", "weibo.com/login", "douyin.com",
            "kuaishou.com", "tiktok.com",
        ];

        // Extract query keywords for relevance scoring
        // Split by common delimiters and filter out short tokens
        let keywords: Vec<String> = query.split(|c: char| !c.is_alphanumeric() && c != '·' && c != '—')
            .filter(|s| s.len() >= 1)
            .map(|s| s.to_ascii_lowercase())
            .collect();

        // Collect candidates with scores: (url, score, link_text)
        let mut candidates: Vec<(String, usize, String)> = Vec::new();
        let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Extract from links in batch results
        if let Some(results) = search_result.get("results").and_then(|r| r.as_array()) {
            for result in results {
                if let Some(links) = result.get("links").and_then(|l| l.as_array()) {
                    for link in links {
                        if let Some(href) = link.get("href").and_then(|h| h.as_str()) {
                            if !href.starts_with("http") {
                                continue;
                            }
                            let lower = href.to_ascii_lowercase();
                            if excluded_domains.iter().any(|d| lower.contains(d)) {
                                continue;
                            }

                            let link_text = link.get("text")
                                .and_then(|t| t.as_str())
                                .unwrap_or("")
                                .to_string();
                            let link_text_lower = link_text.to_ascii_lowercase();

                            // Score: count keyword matches in URL + link text
                            let mut score = 0usize;
                            for kw in &keywords {
                                if lower.contains(kw.as_str()) {
                                    score += 2; // URL match is stronger signal
                                }
                                if link_text_lower.contains(kw.as_str()) {
                                    score += 3; // Link text match is strongest signal
                                }
                            }

                            // Deduplicate: if same URL seen before, boost its score
                            let url_key = Self::normalize_url(href);
                            if let Some(existing) = candidates.iter_mut().find(|(u, _, _)| Self::normalize_url(u) == url_key) {
                                existing.1 += 1; // Cross-engine boost
                                continue;
                            }

                            seen_urls.insert(url_key);
                            candidates.push((href.to_string(), score, link_text));
                        }
                    }
                }
            }
        }

        // Sort by score descending, then by cross-engine count
        candidates.sort_by(|a, b| b.1.cmp(&a.1));

        // Filter: keep URLs with score > 0 (at least one keyword match), or all if none match
        let has_any_score = candidates.iter().any(|(_, s, _)| *s > 0);
        let filtered: Vec<String> = if has_any_score {
            candidates.into_iter()
                .filter(|(_, s, _)| *s > 0)
                .take(max_urls)
                .map(|(u, _, _)| u)
                .collect()
        } else {
            // No keyword matches — fall back to order-based selection
            candidates.into_iter()
                .take(max_urls)
                .map(|(u, _, _)| u)
                .collect()
        };

        if !filtered.is_empty() {
            return filtered;
        }

        // Fallback: extract URLs from merged text
        let mut urls = Vec::new();
        if let Some(text) = search_result.get("merged_text").and_then(|t| t.as_str()) {
            for line in text.lines() {
                let trimmed = line.trim();
                if let Some(start) = trimmed.find("http") {
                    let rest = &trimmed[start..];
                    let end = rest.find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '>' || c == ')')
                        .unwrap_or(rest.len());
                    let url = &rest[..end];
                    if url.len() > 15 && url.starts_with("http") {
                        let lower = url.to_ascii_lowercase();
                        if !excluded_domains.iter().any(|d| lower.contains(d))
                            && !urls.contains(&url.to_string()) {
                                urls.push(url.to_string());
                                if urls.len() >= max_urls {
                                    return urls;
                                }
                            }
                    }
                }
            }
        }

        urls
    }

    /// Normalize URL for deduplication: strip trailing slash, query params, and fragments
    fn normalize_url(url: &str) -> String {
        let mut s = url.to_string();
        // Remove fragment
        if let Some(pos) = s.find('#') {
            s.truncate(pos);
        }
        // Remove trailing slash
        if s.ends_with('/') && s.len() > 1 {
            s.pop();
        }
        s.to_ascii_lowercase()
    }

    /// Fill JS data, platform, and structured data fields into the result JSON
    fn fill_jsdata_fields(result: &mut serde_json::Value, extracted: &ExtractedContent) {
        let js = &extracted.js_data;
        let plat = &extracted.platform;
        let sd = &extracted.structured;

        // Frontend framework
        if let Some(ref fw) = js.framework {
            result["frontend_framework"] = serde_json::json!(fw);
        }

        // Backend platform
        if let Some(ref lang) = plat.language {
            result["backend_language"] = serde_json::json!(lang);
        }
        if let Some(ref fw) = plat.framework {
            result["backend_framework"] = serde_json::json!(fw);
        }
        if let Some(ref srv) = plat.server {
            result["server"] = serde_json::json!(srv);
        }
        if let Some(ref db) = plat.db_hint {
            result["database_hint"] = serde_json::json!(db);
        }
        if !plat.cms_details.is_empty() {
            result["cms_details"] = serde_json::json!(plat.cms_details);
        }

        // JS embedded data
        if !js.embedded_data.is_empty() {
            result["js_embedded_data"] = serde_json::json!(js.embedded_data);
            result["js_embedded_count"] = serde_json::json!(js.embedded_data.len());
        }
        if !js.api_endpoints.is_empty() {
            result["js_api_endpoints"] = serde_json::json!(js.api_endpoints);
            result["js_api_count"] = serde_json::json!(js.api_endpoints.len());
        }
        if !js.inline_scripts.is_empty() {
            result["js_inline_scripts"] = serde_json::json!(js.inline_scripts);
        }

        // Structured data (JSON-LD, OpenGraph, Twitter, microdata)
        if !sd.json_ld.is_empty() {
            result["json_ld"] = serde_json::json!(sd.json_ld);
        }
        if !sd.og.is_empty() {
            result["open_graph"] = serde_json::json!(sd.og);
        }
        if !sd.twitter.is_empty() {
            result["twitter_card"] = serde_json::json!(sd.twitter);
        }
        if !sd.microdata.is_empty() {
            result["microdata"] = serde_json::json!(sd.microdata);
        }

        // Tables
        if !extracted.tables.is_empty() {
            result["tables"] = serde_json::json!(extracted.tables.iter().map(|t| {
                let mut obj = serde_json::json!({
                    "headers": t.headers,
                    "rows": t.rows,
                    "row_count": t.rows.len(),
                });
                if let Some(ref cap) = t.caption {
                    obj["caption"] = serde_json::json!(cap);
                }
                obj
            }).collect::<Vec<_>>());
            result["table_count"] = serde_json::json!(extracted.tables.len());
        }

        // Headings hierarchy
        if !extracted.headings.is_empty() {
            result["headings"] = serde_json::json!(extracted.headings.iter().map(|h| {
                serde_json::json!({"level": h.level, "text": h.text})
            }).collect::<Vec<_>>());
        }

        // Images
        if !extracted.images.is_empty() {
            result["images"] = serde_json::json!(extracted.images.iter().map(|img| {
                let mut obj = serde_json::json!({"src": img.src, "alt": img.alt});
                if let Some(ref t) = img.title { obj["title"] = serde_json::json!(t); }
                obj
            }).collect::<Vec<_>>());
            result["image_count"] = serde_json::json!(extracted.images.len());
        }
    }

    /// Extract structured content from HTML (no external dependencies)
    fn extract_html_content(html: &str, resp_headers: &std::collections::HashMap<String, String>) -> ExtractedContent {
        let lower = html.to_ascii_lowercase();
        let title = Self::extract_tag_content(html, "title");
        let description = Self::extract_meta_content(html, &lower, "description")
            .or_else(|| Self::extract_meta_content(html, &lower, "og:description"));
        let keywords = Self::extract_meta_content(html, &lower, "keywords");
        let links = Self::extract_links(html, &lower);
        let js_data = Self::extract_js_data(html, &lower);
        let platform = Self::detect_platform(resp_headers, html, &lower);
        let structured = Self::extract_structured_data(html, &lower);
        let tables = Self::extract_tables(html, &lower);
        let headings = Self::extract_headings(html, &lower);
        let images = Self::extract_images(html, &lower);

        // Use platform-aware main content extraction instead of generic html_to_text
        let text = Self::extract_main_content(html, &platform);

        ExtractedContent { title, description, keywords, links, text, js_data, platform, structured, tables, headings, images }
    }

    /// Extract all JavaScript-related data from HTML
    fn extract_js_data(html: &str, lower: &str) -> JsExtractedData {
        let mut embedded_data = Vec::new();
        let mut inline_scripts = Vec::new();
        let mut api_endpoints = Vec::new();

        // Known framework SSR data patterns in script tags
        let ssr_patterns = [
            ("__NEXT_DATA__", true),       // Next.js
            ("__NUXT__", true),            // Nuxt.js
            ("__NUXT_DATA__", true),       // Nuxt 3
            ("__INITIAL_STATE__", true),   // Redux / generic
            ("__APP_INITIAL_STATE__", true),
            ("__PRELOADED_STATE__", true),
            ("__SERVER_DATA__", true),
            ("__PAGE_DATA__", true),       // Gatsby
            ("window.__data", true),
            ("window.__STATE__", true),
            ("window.__INITIAL_DATA__", true),
        ];

        // Iterate all <script> tags
        let len = html.len();
        let mut search_from = 0;
        while let Some(script_start) = lower[search_from..].find("<script") {
            let abs_start = search_from + script_start;
            let tag_end = safe_find_char_past(html, '>', abs_start);
            if tag_end > len { break; }
            let script_tag = &html[abs_start..tag_end];

            // Skip external scripts (src="...")
            let is_external = script_tag.to_ascii_lowercase().contains("src=");
            let close_script = safe_find(&lower[tag_end..], "</script>", 0) + tag_end;
            let script_content = safe_slice(html, tag_end + 1, close_script).trim();

            if !is_external && !script_content.is_empty() {
                // Try to extract embedded SSR data
                for (pattern, _is_json) in &ssr_patterns {
                    if let Some(data) = Self::extract_ssr_data(script_content, pattern) {
                        embedded_data.push(data);
                    }
                }

                // Extract API endpoints from this script
                let endpoints = Self::extract_api_endpoints_from_js(script_content);
                api_endpoints.extend(endpoints);

                // Store truncated inline script for debugging
                if script_content.len() > 50 {
                    let truncated = if script_content.len() > 2000 {
                        format!("{}...(truncated)", &script_content[..2000])
                    } else {
                        script_content.to_string()
                    };
                    inline_scripts.push(truncated);
                }
            }

            search_from = close_script + 1;
        }

        // Also check for API endpoints in HTML attributes (data-api, data-url, etc.)
        let attr_endpoints = Self::extract_api_endpoints_from_attrs(html);
        api_endpoints.extend(attr_endpoints);

        // Deduplicate endpoints
        api_endpoints.sort();
        api_endpoints.dedup();

        // Limit inline scripts to avoid bloat
        inline_scripts.truncate(10);

        let framework = Self::detect_framework(html, lower, &embedded_data);

        JsExtractedData { embedded_data, api_endpoints, framework, inline_scripts }
    }

    /// Extract SSR data from a script content given a known variable name pattern.
    /// Handles patterns like:
    ///   __NEXT_DATA__ = {...}
    ///   window.__NUXT__ = {...}
    ///   self.__next_f.push([1, "...")
    fn extract_ssr_data(script: &str, pattern: &str) -> Option<serde_json::Value> {
        // Pattern 1: variable = {...} or window.X = {...}
        if let Some(eq_pos) = script.find(pattern) {
            let after_pattern = &script[eq_pos + pattern.len()..];
            let after_trimmed = after_pattern.trim_start();

            // Direct assignment: pattern = {...}
            if let Some(after_eq) = after_trimmed.strip_prefix('=') {
                return Self::extract_json_from_position(after_eq.trim_start());
            }
        }

        // Pattern 2: self.__next_f.push([1, "..."]) — Next.js RSC payload
        if pattern == "__NEXT_DATA__" {
            if let Some(data) = Self::extract_next_rsc_data(script) {
                return Some(data);
            }
        }

        None
    }

    /// Extract JSON value starting from a position in script text.
    /// Handles {...} and [...] and "string" and `template literals`.
    fn extract_json_from_position(text: &str) -> Option<serde_json::Value> {
        let trimmed = text.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            // Find matching closing bracket
            let open = trimmed.as_bytes()[0];
            let close = if open == b'{' { b'}' } else { b']' };
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escape = false;
            let mut end_pos = 0;
            for (i, &ch) in trimmed.as_bytes().iter().enumerate() {
                if escape {
                    escape = false;
                    continue;
                }
                if ch == b'\\' && in_string {
                    escape = true;
                    continue;
                }
                if ch == b'"' {
                    in_string = !in_string;
                    continue;
                }
                if in_string {
                    continue;
                }
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = i + 1;
                        break;
                    }
                }
            }
            if end_pos > 0 {
                let json_str = &trimmed[..end_pos];
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                    return Some(val);
                }
                // Try with single quotes replaced (some frameworks use single quotes)
                let fixed = json_str.replace('\'', "\"");
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&fixed) {
                    return Some(val);
                }
            }
        }
        None
    }

    /// Extract Next.js React Server Components payload from self.__next_f.push calls
    fn extract_next_rsc_data(script: &str) -> Option<serde_json::Value> {
        // Look for self.__next_f.push([1,"<script>self.__next_f.push(...)</script>"])
        // The actual page data is embedded in these RSC chunks
        let mut chunks = Vec::new();
        let mut search = 0;
        while let Some(push_start) = script[search..].find("self.__next_f.push(") {
            let abs = search + push_start;
            let content_start = abs + "self.__next_f.push(".len();
            if let Some(end) = script[content_start..].find(')') {
                let arg = &script[content_start..content_start + end];
                // Extract string content from the push argument
                if let Some(s) = Self::extract_rsc_string(arg) {
                    chunks.push(s);
                }
                search = content_start + end + 1;
            } else {
                break;
            }
        }

        if chunks.is_empty() {
            return None;
        }

        // Combine chunks and try to extract meaningful data
        let combined = chunks.join("");
        // The RSC format contains lines like: 0:["$","div",null,...]
        // Try to find JSON objects within
        let mut data_items = Vec::new();
        for line in combined.lines() {
            let line = line.trim();
            if line.starts_with('{') || line.starts_with('[') {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                    data_items.push(val);
                }
            }
        }

        if data_items.is_empty() {
            None
        } else {
            Some(serde_json::json!({
                "type": "next_rsc",
                "chunks": data_items,
            }))
        }
    }

    /// Extract string from RSC push argument like [1,"content..."]
    fn extract_rsc_string(arg: &str) -> Option<String> {
        let trimmed = arg.trim();
        if !trimmed.starts_with('[') {
            return None;
        }
        // Find the string part after the first comma
        if let Some(comma) = trimmed.find(',') {
            let rest = trimmed[comma + 1..].trim();
            if let Some(rest_content) = rest.strip_prefix('"') {
                // Find end of string (handle escaped quotes)
                let chars: Vec<char> = rest_content.chars().collect();
                let mut result = String::new();
                let mut i = 0;
                while i < chars.len() {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        match chars[i + 1] {
                            'n' => result.push('\n'),
                            't' => result.push('\t'),
                            '"' => result.push('"'),
                            '\\' => result.push('\\'),
                            _ => {
                                result.push(chars[i]);
                                result.push(chars[i + 1]);
                            }
                        }
                        i += 2;
                    } else if chars[i] == '"' {
                        break;
                    } else {
                        result.push(chars[i]);
                        i += 1;
                    }
                }
                return Some(result);
            }
        }
        None
    }

    /// Extract API endpoints from JavaScript code
    fn extract_api_endpoints_from_js(script: &str) -> Vec<String> {
        let mut endpoints = Vec::new();

        // Pattern 1: fetch("...") / fetch('...')
        for pattern in &[r#"fetch(""#, r#"fetch('"#] {
            let mut search = 0;
            while let Some(pos) = script[search..].find(pattern) {
                let abs = search + pos + pattern.len() - 1;
                let quote = script.as_bytes()[abs];
                let end_quote = if quote == b'"' { '"' } else { '\'' };
                let url_start = abs + 1;
                if let Some(url_end) = script[url_start..].find(end_quote) {
                    let url = &script[url_start..url_start + url_end];
                    if url.starts_with("http") || url.starts_with('/') || url.starts_with("api") {
                        endpoints.push(url.to_string());
                    }
                }
                search = abs + 1;
            }
        }

        // Pattern 2: axios.get/post/put/delete("...")
        for method in &[".get(", ".post(", ".put(", ".delete(", ".patch("] {
            let mut search = 0;
            while let Some(pos) = script[search..].find(method) {
                let abs = search + pos + method.len();
                let after = script[abs..].trim_start();
                if after.starts_with('"') || after.starts_with('\'') {
                    let quote = after.as_bytes()[0] as char;
                    if let Some(end) = after[1..].find(quote) {
                        let url = &after[1..1 + end];
                        if url.starts_with("http") || url.starts_with('/') || url.starts_with("api") {
                            endpoints.push(url.to_string());
                        }
                    }
                }
                search = abs + 1;
            }
        }

        // Pattern 3: XMLHttpRequest .open("METHOD", "url")
        let mut search = 0;
        while let Some(pos) = script[search..].find(".open(") {
            let abs = search + pos + ".open(".len();
            // Skip the method string, find the URL string
            if let Some(first_quote) = script[abs..].find('"') {
                let after_first = abs + first_quote + 1;
                if let Some(end_first) = script[after_first..].find('"') {
                    let after_method = after_first + end_first + 1;
                    if let Some(second_quote) = script[after_method..].find('"') {
                        let url_start = after_method + second_quote + 1;
                        if let Some(url_end) = script[url_start..].find('"') {
                            let url = &script[url_start..url_start + url_end];
                            if url.starts_with("http") || url.starts_with('/') {
                                endpoints.push(url.to_string());
                            }
                        }
                    }
                }
            }
            search = abs;
        }

        // Pattern 4: API URL strings in common formats
        // /api/v1/..., /api/..., https://api.example.com/...
        let mut search = 0;
        while let Some(pos) = script[search..].find("/api/") {
            let abs = search + pos;
            // Find end of URL (stop at whitespace, quote, bracket, parenthesis)
            let end = script[abs..].find(|c: char| {
                c.is_whitespace() || c == '"' || c == '\'' || c == ')' || c == ']' || c == '>' || c == ';'
            }).unwrap_or(script.len() - abs);
            if end > 5 {
                endpoints.push(script[abs..abs + end].to_string());
            }
            search = abs + end;
        }

        endpoints.sort();
        endpoints.dedup();
        endpoints
    }

    /// Extract API endpoints from HTML data attributes
    fn extract_api_endpoints_from_attrs(html: &str) -> Vec<String> {
        let mut endpoints = Vec::new();
        // Look for data-api="...", data-url="...", data-endpoint="..."
        for attr in &["data-api=\"", "data-url=\"", "data-endpoint=\"", "data-src=\""] {
            let mut search = 0;
            while let Some(pos) = html[search..].find(attr) {
                let abs = search + pos + attr.len();
                if let Some(end) = html[abs..].find('"') {
                    let val = &html[abs..abs + end];
                    if val.starts_with("http") || val.starts_with('/') {
                        endpoints.push(val.to_string());
                    }
                }
                search = abs + 1;
            }
        }
        endpoints
    }

    /// Detect frontend framework from HTML content and embedded data
    fn detect_framework(_html: &str, lower: &str, embedded_data: &[serde_json::Value]) -> Option<String> {

        // Check embedded data patterns
        for data in embedded_data {
            if let Some(obj) = data.as_object() {
                if obj.contains_key("page") && obj.contains_key("query") && obj.contains_key("buildId") {
                    return Some("Next.js".to_string());
                }
                if obj.contains_key("data") && obj.contains_key("serverRendered") {
                    return Some("Nuxt.js".to_string());
                }
            }
        }

        // Check HTML patterns
        if lower.contains("__next") || lower.contains("_next/static") || lower.contains("__next_f") {
            return Some("Next.js".to_string());
        }
        if lower.contains("__nuxt") || lower.contains("_nuxt/") {
            return Some("Nuxt.js".to_string());
        }
        if lower.contains("vue") && (lower.contains("data-v-") || lower.contains("v-cloak") || lower.contains("v-bind")) {
            return Some("Vue.js".to_string());
        }
        if lower.contains("react") && (lower.contains("data-reactroot") || lower.contains("data-reactid") || lower.contains("__react")) {
            return Some("React".to_string());
        }
        if lower.contains("angular") && (lower.contains("ng-version") || lower.contains("ng-app") || lower.contains("ng-controller")) {
            return Some("Angular".to_string());
        }
        if lower.contains("svelte") && (lower.contains("svelte-") || lower.contains("__svelte")) {
            return Some("Svelte".to_string());
        }
        if lower.contains("gatsby") || lower.contains("___gatsby") {
            return Some("Gatsby".to_string());
        }
        if lower.contains("remix") || lower.contains("__remixContext") {
            return Some("Remix".to_string());
        }
        if lower.contains("ember") && lower.contains("ember-view") {
            return Some("Ember.js".to_string());
        }
        if lower.contains("jquery") || lower.contains("jquery.min.js") {
            return Some("jQuery".to_string());
        }

        None
    }

    /// Extract content of a tag (e.g., <title>...</title>)
    fn extract_tag_content(html: &str, tag: &str) -> Option<String> {
        let open = format!("<{}", tag);
        let close = format!("</{}>", tag);
        let start = html.find(&open)?;
        let content_start = html[start..].find('>')? + start + 1;
        let end = html[content_start..].find(&close)? + content_start;
        Some(html[content_start..end].trim().to_string())
    }

    /// Extract meta tag content by name attribute
    fn extract_meta_content(html: &str, lower: &str, name: &str) -> Option<String> {
        // Look for <meta name="X" content="Y"> or <meta content="Y" name="X">
        let name_lower = name.to_ascii_lowercase();

        // Find all <meta tags
        let len = html.len();
        let mut search_from = 0;
        while let Some(meta_start) = lower[search_from..].find("<meta") {
            let meta_abs = search_from + meta_start;
            let meta_end = safe_find_char_past(lower, '>', meta_abs);
            if meta_end > len { break; }
            let meta_lower = &lower[meta_abs..meta_end];

            if meta_lower.contains(&name_lower) {
                // Extract content="..."
                if let Some(content_start) = meta_lower.find("content=\"") {
                    let val_start = meta_abs + content_start + 9; // len of content="
                    if let Some(val_end) = html[val_start..].find('"') {
                        return Some(html[val_start..val_start + val_end].to_string());
                    }
                }
            }
            search_from = meta_end + 1;
        }
        None
    }

    /// Extract all links from HTML
    fn extract_links(html: &str, lower: &str) -> Vec<Link> {
        let mut links = Vec::new();
        let len = html.len();
        let mut search_from = 0;

        while let Some(a_start) = lower[search_from..].find("<a ") {
            let a_abs = search_from + a_start;
            let tag_end = safe_find_char_past(html, '>', a_abs);
            if tag_end > len { break; }
            let tag = safe_slice(html, a_abs, tag_end);

            // Extract href
            if let Some(href_start) = tag.to_ascii_lowercase().find("href=\"") {
                let val_start = a_abs + href_start + 6;
                if let Some(val_end) = html[val_start..].find('"') {
                    let href = safe_slice(html, val_start, val_start + val_end).to_string();

                    // Extract link text (between <a> and </a>)
                    let text_start = tag_end;
                    let close_a = safe_find(html, "</a>", text_start);
                    let link_text = Self::strip_tags(safe_slice(html, text_start, close_a)).trim().to_string();

                    if !href.is_empty() && !href.starts_with("javascript:") {
                        links.push(Link { href, text: link_text });
                    }
                }
            }
            search_from = tag_end;
        }

        links
    }

    /// Convert HTML to readable text
    fn html_to_text(html: &str) -> String {
        let text = html.to_string();

        // Remove script and style blocks entirely
        let text = Self::remove_blocks(&text, "script");
        let text = Self::remove_blocks(&text, "style");
        let text = Self::remove_blocks(&text, "noscript");

        // Add newlines for block elements
        let text = text
            .replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n")
            .replace("</p>", "\n\n").replace("</div>", "\n")
            .replace("</li>", "\n").replace("</h1>", "\n\n")
            .replace("</h2>", "\n\n").replace("</h3>", "\n\n")
            .replace("</h4>", "\n\n").replace("</h5>", "\n\n")
            .replace("</h6>", "\n\n").replace("</tr>", "\n")
            .replace("<hr>", "\n---\n").replace("<hr/>", "\n---\n");

        // Strip all remaining HTML tags
        let text = Self::strip_tags(&text);

        // Decode common HTML entities
        let text = text
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&nbsp;", " ")
            .replace("&#39;", "'")
            .replace("&#34;", "\"");

        // Normalize whitespace: collapse multiple spaces/newlines
        let mut result = String::new();
        let mut prev_was_space = false;
        let mut prev_was_newline = false;
        for ch in text.chars() {
            match ch {
                '\n' => {
                    if !prev_was_newline {
                        result.push('\n');
                    }
                    prev_was_space = false;
                    prev_was_newline = true;
                }
                ' ' | '\t' | '\r' => {
                    if !prev_was_space && !prev_was_newline {
                        result.push(' ');
                    }
                    prev_was_space = true;
                }
                _ => {
                    result.push(ch);
                    prev_was_space = false;
                    prev_was_newline = false;
                }
            }
        }

        result.trim().to_string()
    }

    /// Remove all content between open and close tags (case-insensitive)
    fn remove_blocks(html: &str, tag: &str) -> String {
        let open = format!("<{}", tag);
        let close = format!("</{}>", tag);
        let lower = html.to_ascii_lowercase();
        let len = html.len();
        let mut result = String::new();
        let mut last_end = 0;

        let mut search_from = 0;
        while search_from < len {
            let Some(start) = lower[search_from..].find(&open) else { break };
            let abs_start = search_from + start;
            // Find end of opening tag
            let open_end = html[abs_start..].find('>')
                .map(|p| p + abs_start + 1)
                .unwrap_or(len);
            if open_end >= len { break; }
            // Find closing tag
            let close_pos = lower[open_end..].find(&close)
                .map(|p| p + open_end)
                .unwrap_or(len);
            let close_end = if close_pos >= len {
                len
            } else {
                html[close_pos..].find('>')
                    .map(|p| p + close_pos + 1)
                    .unwrap_or(len)
            };

            result.push_str(&html[last_end..abs_start]);
            last_end = close_end;
            search_from = close_end;
        }
        if last_end < len {
            result.push_str(&html[last_end..]);
        }
        result
    }

    /// Strip all HTML tags from text
    fn strip_tags(html: &str) -> String {
        let mut result = String::new();
        let mut in_tag = false;
        for ch in html.chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => result.push(ch),
                _ => {}
            }
        }
        result
    }

    /// Detect backend platform from HTTP headers and HTML content
    fn detect_platform(
        resp_headers: &std::collections::HashMap<String, String>,
        html: &str,
        lower: &str,
    ) -> PlatformInfo {
        let mut language = None;
        let mut framework = None;
        let mut server = None;
        let mut db_hint = None;
        let mut cms_details = Vec::new();

        // === HTTP Header Detection ===
        if let Some(srv) = resp_headers.get("server") {
            let s = srv.to_ascii_lowercase();
            if s.contains("nginx") { server = Some("nginx".to_string()); }
            else if s.contains("apache") { server = Some("Apache".to_string()); }
            else if s.contains("iis") { server = Some("IIS".to_string()); language.get_or_insert_with(|| "C#".to_string()); }
            else if s.contains("gunicorn") { server = Some("Gunicorn".to_string()); language.get_or_insert_with(|| "Python".to_string()); }
            else if s.contains("uvicorn") { server = Some("Uvicorn".to_string()); language.get_or_insert_with(|| "Python".to_string()); }
            else if s.contains("golang") || s.contains("go-http") { server = Some("Go".to_string()); language.get_or_insert_with(|| "Go".to_string()); }
            else if s.contains("openresty") { server = Some("OpenResty".to_string()); language.get_or_insert_with(|| "Lua".to_string()); }
            else if s.contains("litespeed") { server = Some("LiteSpeed".to_string()); }
            else if s.contains("caddy") { server = Some("Caddy".to_string()); }
            else if s.contains("jetty") { server = Some("Jetty".to_string()); language.get_or_insert_with(|| "Java".to_string()); }
            else if s.contains("tomcat") { server = Some("Tomcat".to_string()); language.get_or_insert_with(|| "Java".to_string()); }
            else if s.contains("werkzeug") { server = Some("Werkzeug".to_string()); language.get_or_insert_with(|| "Python".to_string()); }
            else if s.contains("puma") { server = Some("Puma".to_string()); language.get_or_insert_with(|| "Ruby".to_string()); }
            else if s.contains("thin") { server = Some("Thin".to_string()); language.get_or_insert_with(|| "Ruby".to_string()); }
            else if s.contains("cowboy") { server = Some("Cowboy".to_string()); language.get_or_insert_with(|| "Erlang/Elixir".to_string()); }
            else if s.contains("actix") { server = Some("Actix".to_string()); language.get_or_insert_with(|| "Rust".to_string()); }
            else if s.contains("hyper") { server = Some("Hyper".to_string()); language.get_or_insert_with(|| "Rust".to_string()); }
        }

        if let Some(powered) = resp_headers.get("x-powered-by") {
            let p = powered.to_ascii_lowercase();
            if p.contains("php") { language.get_or_insert_with(|| "PHP".to_string()); }
            if p.contains("express") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Express".to_string()); }
            if p.contains("asp.net") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "ASP.NET".to_string()); }
            if p.contains("sails") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Sails.js".to_string()); }
        }

        // Cookie patterns
        if let Some(cookies) = resp_headers.get("set-cookie") {
            let ck = cookies.to_ascii_lowercase();
            if ck.contains("phpsessid") { language.get_or_insert_with(|| "PHP".to_string()); }
            if ck.contains("jsessionid") { language.get_or_insert_with(|| "Java".to_string()); }
            if ck.contains("connect.sid") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Express".to_string()); }
            if ck.contains("_rails_session") || ck.contains("_session_id") { language.get_or_insert_with(|| "Ruby".to_string()); framework.get_or_insert_with(|| "Rails".to_string()); }
            if ck.contains("laravel_session") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Laravel".to_string()); }
            if ck.contains("django") || ck.contains("csrftoken") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "Django".to_string()); }
            if ck.contains("rack.session") { language.get_or_insert_with(|| "Ruby".to_string()); }
            if ck.contains(".aspxauth") || ck.contains("asp.net_sessionid") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "ASP.NET".to_string()); }
            if ck.contains("koa:sess") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Koa".to_string()); }
            if ck.contains("phoenix") { language.get_or_insert_with(|| "Elixir".to_string()); framework.get_or_insert_with(|| "Phoenix".to_string()); }
            if ck.contains("rack.session") && ck.contains("csrf") { language.get_or_insert_with(|| "Ruby".to_string()); framework.get_or_insert_with(|| "Sinatra".to_string()); }
            if ck.contains("play_session") || ck.contains("play_flash") { language.get_or_insert_with(|| "Scala".to_string()); framework.get_or_insert_with(|| "Play Framework".to_string()); }
            if ck.contains("io.n2o") { language.get_or_insert_with(|| "Erlang".to_string()); }
            if ck.contains("gorilla.session") { language.get_or_insert_with(|| "Go".to_string()); }
        }

        // === PHP CMS/Framework Detection ===
        if lower.contains("wp-content/") || lower.contains("wp-includes/") {
            language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "WordPress".to_string());
            if let Some(theme) = Self::extract_between(lower, "wp-content/themes/", "/") { cms_details.push(format!("theme: {}", theme)); }
        }
        if lower.contains("drupal.js") || lower.contains("drupal.min.js") || lower.contains("sites/default/files") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Drupal".to_string()); }
        if lower.contains("/media/jui/") || lower.contains("joomla") || lower.contains("/components/com_") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Joomla".to_string()); }
        if lower.contains("thinkphp") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "ThinkPHP".to_string()); }
        if lower.contains("laravel") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Laravel".to_string()); }
        if lower.contains("typecho") || lower.contains("usr/themes/") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Typecho".to_string()); }
        if lower.contains("discuz") || lower.contains("ucenter") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Discuz!".to_string()); }
        if lower.contains("ecshop") || lower.contains("shopex") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "ECShop".to_string()); }
        if lower.contains("phpcms") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "PHPCMS".to_string()); }
        if lower.contains("dedecms") || lower.contains("dedeajax") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "DedeCMS".to_string()); }
        if lower.contains("帝国cms") || lower.contains("ecmsinfo") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "EmpireCMS".to_string()); }
        if lower.contains("yii") || lower.contains("yii-") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Yii".to_string()); }
        if lower.contains("symfony") || lower.contains("sf-toolbar") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Symfony".to_string()); }
        if lower.contains("codeigniter") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "CodeIgniter".to_string()); }
        if lower.contains("cakephp") || lower.contains("cake_") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "CakePHP".to_string()); }
        if lower.contains("zend") || lower.contains("zendframework") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Zend/Laminas".to_string()); }
        if lower.contains("hyperf") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Hyperf".to_string()); }
        if lower.contains("webman") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "webman".to_string()); }

        // === Python ===
        if lower.contains("csrfmiddlewaretoken") || lower.contains("django") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "Django".to_string()); }
        if lower.contains("flask") || lower.contains("jinja2") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "Flask".to_string()); }
        if lower.contains("fastapi") || lower.contains("swagger-ui") && lower.contains("fastapi") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "FastAPI".to_string()); }
        if lower.contains("tornado") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "Tornado".to_string()); }
        if lower.contains("pyramid") || lower.contains("pylons") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "Pyramid".to_string()); }
        if lower.contains("bottle") && lower.contains("python") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "Bottle".to_string()); }
        if lower.contains("sanic") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "Sanic".to_string()); }
        if lower.contains("web.py") || lower.contains("webpy") { language.get_or_insert_with(|| "Python".to_string()); framework.get_or_insert_with(|| "web.py".to_string()); }

        // === Ruby ===
        if lower.contains("csrf-token") && lower.contains("authenticity_token") { language.get_or_insert_with(|| "Ruby".to_string()); framework.get_or_insert_with(|| "Rails".to_string()); }
        if lower.contains("data-turbo") || lower.contains("turbo-frame") { language.get_or_insert_with(|| "Ruby".to_string()); framework.get_or_insert_with(|| "Rails (Hotwire/Turbo)".to_string()); }
        if lower.contains("sinatra") { language.get_or_insert_with(|| "Ruby".to_string()); framework.get_or_insert_with(|| "Sinatra".to_string()); }
        if lower.contains("hanami") { language.get_or_insert_with(|| "Ruby".to_string()); framework.get_or_insert_with(|| "Hanami".to_string()); }

        // === Java / JVM ===
        if lower.contains("thymeleaf") || lower.contains("whitelabel error page") { language.get_or_insert_with(|| "Java".to_string()); framework.get_or_insert_with(|| "Spring".to_string()); }
        if lower.contains("spring") && lower.contains("boot") { language.get_or_insert_with(|| "Java".to_string()); framework.get_or_insert_with(|| "Spring Boot".to_string()); }
        if lower.contains("struts") || lower.contains("struts2") { language.get_or_insert_with(|| "Java".to_string()); framework.get_or_insert_with(|| "Struts".to_string()); }
        if lower.contains("jsf") || lower.contains("javax.faces") { language.get_or_insert_with(|| "Java".to_string()); framework.get_or_insert_with(|| "JSF".to_string()); }
        if lower.contains("grails") || lower.contains("gsp") { language.get_or_insert_with(|| "Groovy".to_string()); framework.get_or_insert_with(|| "Grails".to_string()); }
        if lower.contains("play") && (lower.contains("play-framework") || lower.contains("play.api")) { language.get_or_insert_with(|| "Scala".to_string()); framework.get_or_insert_with(|| "Play Framework".to_string()); }
        if lower.contains("ktor") { language.get_or_insert_with(|| "Kotlin".to_string()); framework.get_or_insert_with(|| "Ktor".to_string()); }
        if lower.contains("micronaut") { language.get_or_insert_with(|| "Java/Kotlin".to_string()); framework.get_or_insert_with(|| "Micronaut".to_string()); }
        if lower.contains("quarkus") { language.get_or_insert_with(|| "Java".to_string()); framework.get_or_insert_with(|| "Quarkus".to_string()); }
        if lower.contains("vaadin") { language.get_or_insert_with(|| "Java".to_string()); framework.get_or_insert_with(|| "Vaadin".to_string()); }
        if lower.contains("wicket") { language.get_or_insert_with(|| "Java".to_string()); framework.get_or_insert_with(|| "Wicket".to_string()); }

        // === C# / .NET ===
        if lower.contains("__viewstate") || lower.contains("__requestverificationtoken") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "ASP.NET WebForms".to_string()); }
        if lower.contains("__antiforgery") || lower.contains("asp-") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "ASP.NET".to_string()); }
        if lower.contains("blazor") || lower.contains("_blazor") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "Blazor".to_string()); }
        if lower.contains("umbraco") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "Umbraco".to_string()); }
        if lower.contains("sitecore") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "Sitecore".to_string()); }
        if lower.contains(" Orchard") || lower.contains("orchardcore") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "Orchard".to_string()); }
        if lower.contains("nopcommerce") { language.get_or_insert_with(|| "C#".to_string()); framework.get_or_insert_with(|| "nopCommerce".to_string()); }

        // === Node.js ===
        if lower.contains("next") && (lower.contains("_next/static") || lower.contains("__next")) { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Next.js".to_string()); }
        if lower.contains("nuxt") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Nuxt.js".to_string()); }
        if lower.contains("sails") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Sails.js".to_string()); }
        if lower.contains("nestjs") || lower.contains("@nestjs") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "NestJS".to_string()); }
        if lower.contains("hapi") || lower.contains("@hapi") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Hapi".to_string()); }
        if lower.contains("meteor") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Meteor".to_string()); }
        if lower.contains("adonis") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "AdonisJS".to_string()); }
        if lower.contains("strapi") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Strapi".to_string()); }
        if lower.contains("ghost") && lower.contains("ghost-theme") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "Ghost".to_string()); }

        // === Go ===
        if lower.contains("gin-gonic") || lower.contains("gin/") { language.get_or_insert_with(|| "Go".to_string()); framework.get_or_insert_with(|| "Gin".to_string()); }
        if lower.contains("echo") && lower.contains("labstack") { language.get_or_insert_with(|| "Go".to_string()); framework.get_or_insert_with(|| "Echo".to_string()); }
        if lower.contains("beego") { language.get_or_insert_with(|| "Go".to_string()); framework.get_or_insert_with(|| "Beego".to_string()); }
        if lower.contains("fiber") && lower.contains("gofiber") { language.get_or_insert_with(|| "Go".to_string()); framework.get_or_insert_with(|| "Fiber".to_string()); }
        if lower.contains("hugo") || lower.contains("gohugo") { language.get_or_insert_with(|| "Go".to_string()); framework.get_or_insert_with(|| "Hugo".to_string()); }

        // === Rust ===
        if lower.contains("actix") { language.get_or_insert_with(|| "Rust".to_string()); framework.get_or_insert_with(|| "Actix-web".to_string()); }
        if lower.contains("rocket") && lower.contains("rust") { language.get_or_insert_with(|| "Rust".to_string()); framework.get_or_insert_with(|| "Rocket".to_string()); }
        if lower.contains("axum") { language.get_or_insert_with(|| "Rust".to_string()); framework.get_or_insert_with(|| "Axum".to_string()); }

        // === Elixir ===
        if lower.contains("phoenix") { language.get_or_insert_with(|| "Elixir".to_string()); framework.get_or_insert_with(|| "Phoenix".to_string()); }

        // === Scala ===
        if lower.contains("play-framework") || lower.contains("play.api") { language.get_or_insert_with(|| "Scala".to_string()); framework.get_or_insert_with(|| "Play Framework".to_string()); }
        if lower.contains("akka") && lower.contains("http") { language.get_or_insert_with(|| "Scala".to_string()); framework.get_or_insert_with(|| "Akka HTTP".to_string()); }
        if lower.contains("lift") && lower.contains("scala") { language.get_or_insert_with(|| "Scala".to_string()); framework.get_or_insert_with(|| "Lift".to_string()); }

        // === Kotlin ===
        if lower.contains("ktor") { language.get_or_insert_with(|| "Kotlin".to_string()); framework.get_or_insert_with(|| "Ktor".to_string()); }
        if lower.contains("javalin") { language.get_or_insert_with(|| "Kotlin/Java".to_string()); framework.get_or_insert_with(|| "Javalin".to_string()); }

        // === Lua ===
        if lower.contains("openresty") || lower.contains("lua-resty") { language.get_or_insert_with(|| "Lua".to_string()); framework.get_or_insert_with(|| "OpenResty/Lua".to_string()); }
        if lower.contains("sailor") || lower.contains("lapis") { language.get_or_insert_with(|| "Lua".to_string()); framework.get_or_insert_with(|| "Lapis".to_string()); }

        // === Perl ===
        if lower.contains("catalyst") { language.get_or_insert_with(|| "Perl".to_string()); framework.get_or_insert_with(|| "Catalyst".to_string()); }
        if lower.contains("mojolicious") || lower.contains("mojo.js") { language.get_or_insert_with(|| "Perl".to_string()); framework.get_or_insert_with(|| "Mojolicious".to_string()); }
        if lower.contains("dancer") { language.get_or_insert_with(|| "Perl".to_string()); framework.get_or_insert_with(|| "Dancer2".to_string()); }

        // === Haskell ===
        if lower.contains("yesod") { language.get_or_insert_with(|| "Haskell".to_string()); framework.get_or_insert_with(|| "Yesod".to_string()); }
        if lower.contains("servant") && lower.contains("haskell") { language.get_or_insert_with(|| "Haskell".to_string()); framework.get_or_insert_with(|| "Servant".to_string()); }
        if lower.contains("scotty") { language.get_or_insert_with(|| "Haskell".to_string()); framework.get_or_insert_with(|| "Scotty".to_string()); }
        if lower.contains("ihp") { language.get_or_insert_with(|| "Haskell".to_string()); framework.get_or_insert_with(|| "IHP".to_string()); }

        // === Clojure ===
        if lower.contains("ring") && lower.contains("clojure") { language.get_or_insert_with(|| "Clojure".to_string()); framework.get_or_insert_with(|| "Ring".to_string()); }
        if lower.contains("luminus") { language.get_or_insert_with(|| "Clojure".to_string()); framework.get_or_insert_with(|| "Luminus".to_string()); }

        // === Swift ===
        if lower.contains("vapor") && lower.contains("swift") { language.get_or_insert_with(|| "Swift".to_string()); framework.get_or_insert_with(|| "Vapor".to_string()); }
        if lower.contains("kitura") { language.get_or_insert_with(|| "Swift".to_string()); framework.get_or_insert_with(|| "Kitura".to_string()); }

        // === Dart ===
        if lower.contains("aqueduct") || lower.contains("dart") && lower.contains("server") { language.get_or_insert_with(|| "Dart".to_string()); framework.get_or_insert_with(|| "Aqueduct".to_string()); }

        // === Zig ===
        if lower.contains("zap") && lower.contains("zig") { language.get_or_insert_with(|| "Zig".to_string()); framework.get_or_insert_with(|| "zap".to_string()); }

        // === Nim ===
        if lower.contains("jester") { language.get_or_insert_with(|| "Nim".to_string()); framework.get_or_insert_with(|| "Jester".to_string()); }
        if lower.contains("prologue") && lower.contains("nim") { language.get_or_insert_with(|| "Nim".to_string()); framework.get_or_insert_with(|| "Prologue".to_string()); }

        // === Crystal ===
        if lower.contains("kemal") { language.get_or_insert_with(|| "Crystal".to_string()); framework.get_or_insert_with(|| "Kemal".to_string()); }
        if lower.contains("lucky") && lower.contains("crystal") { language.get_or_insert_with(|| "Crystal".to_string()); framework.get_or_insert_with(|| "Lucky".to_string()); }
        if lower.contains("amber") && lower.contains("crystal") { language.get_or_insert_with(|| "Crystal".to_string()); framework.get_or_insert_with(|| "Amber".to_string()); }

        // === C/C++ ===
        if lower.contains("crow") && lower.contains("c++") { language.get_or_insert_with(|| "C++".to_string()); framework.get_or_insert_with(|| "Crow".to_string()); }
        if lower.contains("drogon") { language.get_or_insert_with(|| "C++".to_string()); framework.get_or_insert_with(|| "Drogon".to_string()); }

        // === Erlang ===
        if lower.contains("cowboy") { language.get_or_insert_with(|| "Erlang".to_string()); framework.get_or_insert_with(|| "Cowboy".to_string()); }
        if lower.contains("mochiweb") { language.get_or_insert_with(|| "Erlang".to_string()); framework.get_or_insert_with(|| "Mochiweb".to_string()); }

        // === R ===
        if lower.contains("shiny") && lower.contains("r-") { language.get_or_insert_with(|| "R".to_string()); framework.get_or_insert_with(|| "Shiny".to_string()); }

        // === Julia ===
        if lower.contains("genie") && lower.contains("julia") { language.get_or_insert_with(|| "Julia".to_string()); framework.get_or_insert_with(|| "Genie".to_string()); }

        // === Static Site Generators ===
        if lower.contains("hexo") || lower.contains("hexo-theme") { framework.get_or_insert_with(|| "Hexo".to_string()); }
        if lower.contains("jekyll") || lower.contains("github-pages") { framework.get_or_insert_with(|| "Jekyll/GitHub Pages".to_string()); }
        if lower.contains("gatsby") || lower.contains("___gatsby") { framework.get_or_insert_with(|| "Gatsby".to_string()); }
        if lower.contains("eleventy") || lower.contains("11ty") { framework.get_or_insert_with(|| "Eleventy (11ty)".to_string()); }
        if lower.contains("docusaurus") { framework.get_or_insert_with(|| "Docusaurus".to_string()); }
        if lower.contains("vuepress") { framework.get_or_insert_with(|| "VuePress".to_string()); }
        if lower.contains("mkdocs") { framework.get_or_insert_with(|| "MkDocs".to_string()); language.get_or_insert_with(|| "Python".to_string()); }
        if lower.contains("sphinx") && lower.contains("rst") { framework.get_or_insert_with(|| "Sphinx".to_string()); language.get_or_insert_with(|| "Python".to_string()); }
        if lower.contains("pelican") { framework.get_or_insert_with(|| "Pelican".to_string()); language.get_or_insert_with(|| "Python".to_string()); }
        if lower.contains("hugo") { framework.get_or_insert_with(|| "Hugo".to_string()); language.get_or_insert_with(|| "Go".to_string()); }
        if lower.contains("zola") { framework.get_or_insert_with(|| "Zola".to_string()); language.get_or_insert_with(|| "Rust".to_string()); }

        // === Headless CMS ===
        if lower.contains("contentful") { cms_details.push("CMS: Contentful".to_string()); }
        if lower.contains("sanity") && lower.contains("studio") { cms_details.push("CMS: Sanity".to_string()); }
        if lower.contains("prismic") { cms_details.push("CMS: Prismic".to_string()); }
        if lower.contains("strapi") { framework.get_or_insert_with(|| "Strapi".to_string()); language.get_or_insert_with(|| "Node.js".to_string()); }
        if lower.contains("directus") { cms_details.push("CMS: Directus".to_string()); }
        if lower.contains("keystonejs") || lower.contains("keystone") && lower.contains("keystone") { framework.get_or_insert_with(|| "KeystoneJS".to_string()); language.get_or_insert_with(|| "Node.js".to_string()); }

        // === E-commerce ===
        if lower.contains("magento") || lower.contains("mage/") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Magento".to_string()); }
        if lower.contains("shopify") { cms_details.push("Platform: Shopify".to_string()); }
        if lower.contains("woocommerce") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "WooCommerce".to_string()); }
        if lower.contains("prestashop") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "PrestaShop".to_string()); }
        if lower.contains("opencart") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "OpenCart".to_string()); }
        if lower.contains("bigcommerce") { cms_details.push("Platform: BigCommerce".to_string()); }

        // === Forum / Community ===
        if lower.contains("discourse") { language.get_or_insert_with(|| "Ruby".to_string()); framework.get_or_insert_with(|| "Discourse".to_string()); }
        if lower.contains("flarum") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "Flarum".to_string()); }
        if lower.contains("vbulletin") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "vBulletin".to_string()); }
        if lower.contains("phpbb") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "phpBB".to_string()); }
        if lower.contains("nodebb") { language.get_or_insert_with(|| "Node.js".to_string()); framework.get_or_insert_with(|| "NodeBB".to_string()); }

        // === Wiki ===
        if lower.contains("mediawiki") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "MediaWiki".to_string()); }
        if lower.contains("dokuwiki") { language.get_or_insert_with(|| "PHP".to_string()); framework.get_or_insert_with(|| "DokuWiki".to_string()); }
        if lower.contains("confluence") { cms_details.push("Platform: Confluence".to_string()); }
        if lower.contains("notion") && lower.contains("notion.so") { cms_details.push("Platform: Notion".to_string()); }

        // Generator meta tag
        if let Some(gen) = Self::extract_meta_content(html, lower, "generator") {
            cms_details.push(format!("generator: {}", gen));
            let gen_lower = gen.to_ascii_lowercase();
            if gen_lower.contains("wordpress") { framework.get_or_insert_with(|| format!("WordPress {}", gen)); }
            else if gen_lower.contains("drupal") { framework.get_or_insert_with(|| gen.clone()); }
            else if gen_lower.contains("joomla") { framework.get_or_insert_with(|| gen.clone()); }
            else if gen_lower.contains("hugo") { framework.get_or_insert_with(|| gen.clone()); }
            else if gen_lower.contains("hexo") { framework.get_or_insert_with(|| gen.clone()); }
            else if gen_lower.contains("gatsby") { framework.get_or_insert_with(|| gen.clone()); }
            else if gen_lower.contains("next") { framework.get_or_insert_with(|| gen.clone()); }
            else if gen_lower.contains("nuxt") { framework.get_or_insert_with(|| gen.clone()); }
        }

        // Database hints
        if lower.contains("mysql") || lower.contains("mysqli") || lower.contains("pdo_mysql") { db_hint.get_or_insert_with(|| "MySQL".to_string()); }
        if lower.contains("postgresql") || lower.contains("postgres") || lower.contains("pdo_pgsql") { db_hint.get_or_insert_with(|| "PostgreSQL".to_string()); }
        if lower.contains("mongodb") || lower.contains("mongoose") { db_hint.get_or_insert_with(|| "MongoDB".to_string()); }
        if lower.contains("sqlite") || lower.contains("pdo_sqlite") { db_hint.get_or_insert_with(|| "SQLite".to_string()); }
        if lower.contains("redis") { db_hint.get_or_insert_with(|| "Redis".to_string()); }
        if lower.contains("memcache") { db_hint.get_or_insert_with(|| "Memcached".to_string()); }
        if lower.contains("elasticsearch") || lower.contains("elastic") { db_hint.get_or_insert_with(|| "Elasticsearch".to_string()); }
        if lower.contains("cassandra") { db_hint.get_or_insert_with(|| "Cassandra".to_string()); }
        if lower.contains("couchdb") { db_hint.get_or_insert_with(|| "CouchDB".to_string()); }
        if lower.contains("neo4j") { db_hint.get_or_insert_with(|| "Neo4j".to_string()); }
        if lower.contains("mssql") || lower.contains("sqlserver") { db_hint.get_or_insert_with(|| "SQL Server".to_string()); }
        if lower.contains("oracle") && lower.contains("database") { db_hint.get_or_insert_with(|| "Oracle".to_string()); }

        PlatformInfo { language, framework, server, db_hint, cms_details }
    }

    /// Extract substring between prefix and suffix
    fn extract_between<'a>(text: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
        let start = text.find(prefix)? + prefix.len();
        let end = text[start..].find(suffix)? + start;
        Some(&text[start..end])
    }

    /// Platform-aware main content extraction
    fn extract_main_content(html: &str, platform: &PlatformInfo) -> String {
        let fw = platform.framework.as_deref().unwrap_or("");

        let selectors: &[&str] = if fw.contains("WordPress") {
            &[".entry-content", ".post-content", "article .content", ".article-content", "#content .post"]
        } else if fw.contains("Drupal") {
            &[".field-item", ".node-content", "#main-content"]
        } else if fw.contains("Joomla") {
            &[".item-page", ".item-content", "#article"]
        } else if fw.contains("Typecho") {
            &[".post-content", ".entry-content", "#main .post"]
        } else if fw.contains("Discuz") {
            &[".t_f", ".message", "#postmessage"]
        } else if fw.contains("Django") || fw.contains("Flask") || fw.contains("FastAPI") {
            &[".article-content", ".content", "#content", "main .body"]
        } else if fw.contains("Rails") || fw.contains("Sinatra") {
            &[".article-body", ".post-body", "#content", "main article"]
        } else if fw.contains("Spring") || fw.contains("Struts") {
            &[".article-content", ".content-body", "#content"]
        } else if fw.contains("Hexo") || fw.contains("Hugo") || fw.contains("Jekyll") || fw.contains("Hugo") {
            &[".post-body", ".article-content", ".entry-content", "#content"]
        } else if fw.contains("Laravel") || fw.contains("Yii") || fw.contains("Symfony") {
            &[".article-content", ".post-content", "#content"]
        } else if fw.contains("Ghost") {
            &[".post-content", ".gh-content", "article .content"]
        } else if fw.contains("MediaWiki") {
            &["#mw-content-text", "#bodyContent", ".mw-body-content"]
        } else if fw.contains("Discourse") {
            &[".cooked", "#post_1 .cooked", ".topic-body"]
        } else if fw.contains("phpBB") || fw.contains("vBulletin") {
            &[".content", ".postbody", ".message"]
        } else {
            &["article", "main", ".article-content", ".post-content", ".entry-content",
              "#content", "#main-content", ".content-body", ".page-content", ".post-body",
              ".article-body", ".gh-content", ".cooked"]
        };

        for selector in selectors {
            if let Some(content) = Self::extract_by_selector_pattern(html, selector) {
                let text = Self::html_to_text(&content);
                if text.len() > 100 {
                    return text;
                }
            }
        }

        Self::html_to_text(html)
    }

    /// Extract HTML content by CSS selector-like pattern matching
    fn extract_by_selector_pattern(html: &str, selector: &str) -> Option<String> {
        let is_class = selector.starts_with('.');
        let is_id = selector.starts_with('#');

        if !is_class && !is_id {
            let open = format!("<{}", selector);
            let close = format!("</{}>", selector);
            let lower = html.to_ascii_lowercase();
            if let Some(start) = lower.find(&open) {
                let tag_end = safe_find_char_past(html, '>', start);
                if let Some(close_pos) = lower[tag_end..].find(&close) {
                    return Some(html[tag_end + 1..tag_end + close_pos].to_string());
                }
            }
            return None;
        }

        let attr_name = if is_class { "class" } else { "id" };
        let attr_value = &selector[1..];
        let lower = html.to_ascii_lowercase();
        let attr_pattern = format!("{}=\"", attr_name);
        let mut search_from = 0;

        while let Some(pos) = lower[search_from..].find(&attr_pattern) {
            let abs = search_from + pos;
            let val_start = abs + attr_pattern.len();
            if let Some(val_end) = html[val_start..].find('"') {
                let attr_val = &html[val_start..val_start + val_end];
                if attr_val.split_whitespace().any(|c| c == attr_value) {
                    let tag_start = html[..abs].rfind('<').unwrap_or(0);
                    let tag_end_line = safe_find_char_past(html, '>', abs);
                    if tag_end_line > html.len() { continue; }
                    let tag = &html[tag_start..tag_end_line];
                    let tag_name = tag.split_whitespace().next()
                        .unwrap_or("")
                        .trim_start_matches('<')
                        .trim_start_matches('/');
                    let close_tag = format!("</{}>", tag_name);
                    let content_start = tag_end_line + 1;
                    let lower_rest = &lower[content_start..];
                    if let Some(end) = lower_rest.find(&close_tag) {
                        return Some(html[content_start..content_start + end].to_string());
                    }
                }
            }
            search_from = abs + 1;
        }

        None
    }

    /// Extract structured data: JSON-LD, OpenGraph, Twitter Cards, Schema.org microdata
    fn extract_structured_data(html: &str, lower: &str) -> StructuredData {
        let mut json_ld = Vec::new();
        let mut og = std::collections::HashMap::new();
        let mut twitter = std::collections::HashMap::new();
        let mut microdata = Vec::new();

        // JSON-LD: <script type="application/ld+json">
        let mut search = 0;
        while let Some(start) = lower[search..].find("application/ld+json") {
            let abs = search + start;
            // Find the script content
            if let Some(script_start) = html[abs..].find('>') {
                let content_start = abs + script_start + 1;
                if let Some(content_end) = lower[content_start..].find("</script>") {
                    let json_str = html[content_start..content_start + content_end].trim();
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
                        json_ld.push(val);
                    }
                }
            }
            search = abs + 20;
        }

        // OpenGraph: <meta property="og:..." content="...">
        let len = html.len();
        let mut search = 0;
        while let Some(pos) = lower[search..].find("property=\"og:") {
            let abs = search + pos;
            let prop_start = abs + "property=\"".len();
            if let Some(prop_end) = html[prop_start..].find('"') {
                let prop = &html[prop_start..prop_start + prop_end];
                // Find content attribute
                let tag_end = safe_find_char_past(html, '>', abs);
                if tag_end > len { break; }
                let tag = &html[abs..tag_end];
                let tag_lower = tag.to_ascii_lowercase();
                if let Some(c_start) = tag_lower.find("content=\"") {
                    let val_start = abs + c_start + 9;
                    if let Some(val_end) = html[val_start..].find('"') {
                        let key = prop.strip_prefix("og:").unwrap_or(prop);
                        og.insert(key.to_string(), html[val_start..val_start + val_end].to_string());
                    }
                }
            }
            search = abs + 1;
        }

        // Twitter Cards: <meta name="twitter:..." content="...">
        let mut search = 0;
        while let Some(pos) = lower[search..].find("name=\"twitter:") {
            let abs = search + pos;
            let name_start = abs + "name=\"".len();
            if let Some(name_end) = html[name_start..].find('"') {
                let name = &html[name_start..name_start + name_end];
                let tag_end = safe_find_char_past(html, '>', abs);
                if tag_end > len { break; }
                let tag = &html[abs..tag_end];
                let tag_lower = tag.to_ascii_lowercase();
                if let Some(c_start) = tag_lower.find("content=\"") {
                    let val_start = abs + c_start + 9;
                    if let Some(val_end) = html[val_start..].find('"') {
                        let key = name.strip_prefix("twitter:").unwrap_or(name);
                        twitter.insert(key.to_string(), html[val_start..val_start + val_end].to_string());
                    }
                }
            }
            search = abs + 1;
        }

        // Schema.org microdata: itemscope/itemtype/itemprop
        let len = html.len();
        let mut search = 0;
        while let Some(pos) = lower[search..].find("itemscope") {
            let abs = search + pos;
            let tag_end = safe_find_char_past(html, '>', abs);
            if tag_end > len { break; }
            let tag = &html[abs..tag_end];
            let tag_lower = tag.to_ascii_lowercase();

            if let Some(type_start) = tag_lower.find("itemtype=\"") {
                let val_start = abs + type_start + 10;
                if let Some(val_end) = html[val_start..].find('"') {
                    let item_type = &html[val_start..val_start + val_end];
                    // Find itemprop values within this scope
                    let scope_end = safe_find(lower, "</", abs);
                    let scope = &html[abs..scope_end];
                    let scope_lower = &lower[abs..scope_end];

                    let mut props = serde_json::Map::new();
                    props.insert("@type".to_string(), serde_json::Value::String(item_type.to_string()));

                    let mut prop_search = 0;
                    while let Some(p_pos) = scope_lower[prop_search..].find("itemprop=\"") {
                        let p_abs = prop_search + p_pos + 10;
                        if let Some(p_end) = scope[p_abs..].find('"') {
                            let prop_name = &scope[p_abs..p_abs + p_end];
                            // Try content attribute first
                            let p_tag_end = safe_find_char_past(scope_lower, '>', p_abs);
                            if p_tag_end > scope_lower.len() { break; }
                            let p_tag = &scope[p_abs..p_tag_end];
                            let p_tag_lower = p_tag.to_ascii_lowercase();
                            if let Some(c_start) = p_tag_lower.find("content=\"") {
                                let c_abs = p_abs + c_start + 9;
                                if let Some(c_end) = scope[c_abs..].find('"') {
                                    props.insert(prop_name.to_string(), serde_json::Value::String(scope[c_abs..c_abs + c_end].to_string()));
                                }
                            }
                        }
                        prop_search = p_abs + 1;
                    }

                    if !props.is_empty() {
                        microdata.push(serde_json::Value::Object(props));
                    }
                }
            }
            search = abs + 1;
        }

        StructuredData { json_ld, og, twitter, microdata }
    }

    /// Extract HTML tables into structured data
    fn extract_tables(html: &str, lower: &str) -> Vec<TableData> {
        let mut tables = Vec::new();
        let mut search = 0;

        while let Some(pos) = lower[search..].find("<table") {
            let abs = search + pos;
            let table_end = safe_find(lower, "</table>", abs);
            let table_html = safe_slice(html, abs, table_end);

            // Extract caption
            let caption = Self::extract_tag_content(table_html, "caption");

            // Extract headers from <thead> or first <tr> with <th>
            let mut headers = Vec::new();
            if let Some(thead_start) = table_html.to_ascii_lowercase().find("<thead") {
                let thead_end = safe_find(table_html, "</thead>", thead_start);
                let thead = &table_html[thead_start..thead_end];
                headers = Self::extract_cells(thead, "th");
            }
            if headers.is_empty() {
                // Try first <tr> with <th>
                if let Some(tr_start) = table_html.to_ascii_lowercase().find("<tr") {
                    let tr_end = safe_find(table_html, "</tr>", tr_start);
                    let tr = &table_html[tr_start..tr_end];
                    let th_cells = Self::extract_cells(tr, "th");
                    if !th_cells.is_empty() {
                        headers = th_cells;
                    }
                }
            }

            // Extract data rows from <tbody> or all <tr> with <td>
            let mut rows = Vec::new();
            let tbody_content = if let Some(tbody_start) = table_html.to_ascii_lowercase().find("<tbody") {
                let tbody_end = safe_find(table_html, "</tbody>", tbody_start);
                &table_html[tbody_start..tbody_end]
            } else {
                table_html
            };

            let mut tr_search = 0;
            let tr_lower = tbody_content.to_ascii_lowercase();
            while let Some(tr_pos) = tr_lower[tr_search..].find("<tr") {
                let tr_abs = tr_search + tr_pos;
                let tr_end = tr_lower[tr_abs..].find("</tr>").unwrap_or(tbody_content.len() - tr_abs) + tr_abs;
                let tr = &tbody_content[tr_abs..tr_end];
                let cells = Self::extract_cells(tr, "td");
                if !cells.is_empty() {
                    rows.push(cells);
                }
                tr_search = tr_end + 1;
            }

            if !headers.is_empty() || !rows.is_empty() {
                tables.push(TableData { caption, headers, rows });
            }

            search = table_end + 1;
        }

        tables
    }

    /// Extract cell contents from a row (finds all <th> or <td> elements)
    fn extract_cells(html: &str, cell_tag: &str) -> Vec<String> {
        let mut cells = Vec::new();
        let open = format!("<{}", cell_tag);
        let close = format!("</{}>", cell_tag);
        let lower = html.to_ascii_lowercase();
        let mut search = 0;

        while let Some(pos) = lower[search..].find(&open) {
            let abs = search + pos;
            let tag_end = safe_find_char_past(html, '>', abs);
            let content_start = tag_end;
            if let Some(content_end) = lower[content_start..].find(&close) {
                let cell_text = Self::strip_tags(&html[content_start..content_start + content_end]).trim().to_string();
                cells.push(cell_text);
            }
            search = abs + close.len();
        }

        cells
    }

    /// Extract heading hierarchy (h1-h6)
    fn extract_headings(html: &str, lower: &str) -> Vec<Heading> {
        let mut headings = Vec::new();

        for level in 1u8..=6 {
            let open = format!("<h{}", level);
            let close = format!("</h{}>", level);
            let mut search = 0;
            while let Some(pos) = lower[search..].find(&open) {
                let abs = search + pos;
                // Make sure it's exactly <hN (not <h1 inside <h10 etc.)
                let after_open = &lower[abs + open.len()..];
                if !after_open.is_empty() && after_open.as_bytes()[0] != b'0' {
                    let tag_end = safe_find_char_past(html, '>', abs);
                    let content_start = tag_end;
                    if let Some(content_end) = lower[content_start..].find(&close) {
                        let text = Self::strip_tags(&html[content_start..content_start + content_end]).trim().to_string();
                        if !text.is_empty() {
                            headings.push(Heading { level, text });
                        }
                    }
                }
                search = abs + 1;
            }
        }

        headings
    }

    /// Extract images with src, alt, and title
    fn extract_images(html: &str, lower: &str) -> Vec<ImageInfo> {
        let mut images = Vec::new();
        let mut search = 0;

        while let Some(pos) = lower[search..].find("<img") {
            let abs = search + pos;
            let tag_end = safe_find_char_past(html, '>', abs);
            if tag_end > html.len() { break; }
            let tag = &html[abs..tag_end];
            let tag_lower = &lower[abs..tag_end];

            let src = Self::extract_attr_value(tag, tag_lower, "src").unwrap_or_default();
            if src.is_empty() || src.starts_with("data:") {
                search = tag_end + 1;
                continue;
            }
            let alt = Self::extract_attr_value(tag, tag_lower, "alt").unwrap_or_default();
            let title = Self::extract_attr_value(tag, tag_lower, "title");

            // Skip tiny tracking pixels
            let width = Self::extract_attr_value(tag, tag_lower, "width").and_then(|w| w.parse::<u32>().ok());
            let height = Self::extract_attr_value(tag, tag_lower, "height").and_then(|h| h.parse::<u32>().ok());
            if let (Some(w), Some(h)) = (width, height) {
                if w <= 1 || h <= 1 {
                    search = tag_end + 1;
                    continue;
                }
            }

            images.push(ImageInfo { src, alt, title });
            search = tag_end + 1;
        }

        images
    }

    /// Extract an attribute value from a tag (case-insensitive attr name)
    fn extract_attr_value(tag: &str, tag_lower: &str, attr: &str) -> Option<String> {
        let pattern = format!("{}=\"", attr);
        let p_lower = pattern.to_ascii_lowercase();
        let pos = tag_lower.find(&p_lower)?;
        let val_start = pos + pattern.len();
        let val_end = tag[val_start..].find('"')?;
        Some(tag[val_start..val_start + val_end].to_string())
    }
}

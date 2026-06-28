//! 日期时间工具执行器
//!
//! 统一采用北京时间 (UTC+8)。支持获取当前时间、转换时区、计算时间差、格式化时间、时间戳转换。

use async_trait::async_trait;
use chrono::{Datelike, FixedOffset, TimeZone, Timelike};

use agent_teams_core::tool::{
    Tool, ToolBuilder, ToolCall, ToolExecutionContext, ToolExecutor, ToolResult, tool_error, tool_success,
};
use agent_teams_core::error::Result;

/// 北京时间固定偏移 UTC+8
fn beijing_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

/// 获取当前北京时间
fn now_beijing() -> chrono::DateTime<FixedOffset> {
    chrono::Utc::now().with_timezone(&beijing_offset())
}

/// 日期时间工具（合并了原 datetime + timestamp）
pub struct DateTimeTool;

impl DateTimeTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DateTimeTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutor for DateTimeTool {
    fn executor_id(&self) -> &str {
        "datetime"
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            ToolBuilder::new("datetime")
                .description(concat!(
                    "日期时间工具，支持获取当前时间、计算时间差、格式化时间、时间戳转换、时区转换。\n",
                    "所有时间默认北京时间 (UTC+8)。\n",
                    "action 参数：\n",
                    "  now — 获取当前北京时间（返回详细时间信息）\n",
                    "  diff — 计算两个时间的差值（需 from_time 和 to_time）\n",
                    "  format — 格式化时间（需 from_time，可选 format）\n",
                    "  to_unix — 时间转 Unix 时间戳（需 time 参数）\n",
                    "  from_unix — Unix 时间戳转北京时间（需 timestamp 参数）\n",
                    "  convert — 时区转换（需 from_time 和 timezone 参数）\n",
                    "  parse — 解析自然语言时间（如 '明天下午3点'、'下周二'，需 text 参数）\n\n",
                    "工具联动：\n",
                    "- 获取的时间可用 file(write) 保存到文件\n",
                    "- 可用于为 http_request 的请求添加时间参数\n",
                    "- 可用于记录 xxt 操作的时间戳",
                ))
                .executor("datetime")
                .tag("time")
                .tag("utility")
                .timeout(5_000)
                .param_enum("action", "操作类型", &["now", "diff", "format", "to_unix", "from_unix", "convert", "parse"], true)
                .param_string("format", "输出格式：iso/rfc2822/strftime格式(如'%Y-%m-%d %H:%M:%S')，默认iso", false)
                .param_string("from_time", "起始时间（ISO 8601格式，用于 diff/format/to_unix/convert）", false)
                .param_string("to_time", "结束时间（ISO 8601格式，用于 diff 计算）", false)
                .param_integer("timestamp", "Unix 时间戳（秒，用于 from_unix）", false)
                .param_string("timezone", "目标时区（用于 convert，如 'UTC'、'America/New_York'、'Asia/Tokyo'）", false)
                .param_string("text", "自然语言时间描述（用于 parse，如 '明天下午3点'）", false)
                .build(),
        ]
    }

    async fn execute(
        &self,
        call: &ToolCall,
        _ctx: &ToolExecutionContext,
    ) -> Result<ToolResult> {
        let start = std::time::Instant::now();
        let action = match call.arguments["action"].as_str() {
            Some(a) => a,
            None => {
                let ms = start.elapsed().as_millis() as u64;
                return Ok(tool_error(call,
                    "缺少必需参数 'action'",
                    "参数 'action' 未提供或不是字符串",
                    "请提供 action 参数，可选值：now、diff、format、to_unix、from_unix",
                    ms,
                ));
            }
        };

        let ms = start.elapsed().as_millis() as u64;
        match action {
            "now" => match self.execute_now(call) {
                Ok(output) => Ok(tool_success(call, output, ms)),
                Err(e) => Ok(tool_error(call, "获取时间失败", e.to_string(), "检查系统时间是否正常", ms)),
            },
            "diff" => match self.execute_diff(call) {
                Ok(output) => Ok(tool_success(call, output, ms)),
                Err(e) => Ok(tool_error(call, "计算时间差失败", e.to_string(),
                    "请确保 from_time 和 to_time 都是 ISO 8601 格式，如 2024-01-01T00:00:00+08:00", ms)),
            },
            "format" => match self.execute_format(call) {
                Ok(output) => Ok(tool_success(call, output, ms)),
                Err(e) => Ok(tool_error(call, "格式化时间失败", e.to_string(),
                    "请确保 from_time 是 ISO 8601 格式，format 是 strftime 格式或预设值（iso/rfc2822）", ms)),
            },
            "to_unix" => match self.execute_to_unix(call) {
                Ok(output) => Ok(tool_success(call, output, ms)),
                Err(e) => Ok(tool_error(call, "转时间戳失败", e.to_string(),
                    "请确保 time 是 ISO 8601 格式或 '2024-01-01 12:00:00'（北京时间）", ms)),
            },
            "from_unix" => match self.execute_from_unix(call) {
                Ok(output) => Ok(tool_success(call, output, ms)),
                Err(e) => Ok(tool_error(call, "时间戳转换失败", e.to_string(),
                    "请确保 timestamp 是有效的 Unix 时间戳（秒），如 1704067200", ms)),
            },
            "convert" => match self.execute_convert(call) {
                Ok(output) => Ok(tool_success(call, output, ms)),
                Err(e) => Ok(tool_error(call, "时区转换失败", e.to_string(),
                    "请确保 from_time 是 ISO 8601 格式，timezone 是有效时区如 'UTC'、'Asia/Tokyo'", ms)),
            },
            "parse" => match self.execute_parse(call) {
                Ok(output) => Ok(tool_success(call, output, ms)),
                Err(e) => Ok(tool_error(call, "时间解析失败", e.to_string(),
                    "请输入自然语言时间描述，如 '明天下午3点'、'下周二'、'2小时后'", ms)),
            },
            _ => Ok(tool_error(call,
                format!("未知操作 '{}'", action),
                format!("action='{}' 不是有效的操作类型", action),
                "请使用：now、diff、format、to_unix、from_unix",
                ms,
            )),
        }
    }
}

impl DateTimeTool {
    fn execute_now(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let format = call.arguments["format"].as_str().unwrap_or("iso");
        let now = now_beijing();
        let formatted = match format {
            "rfc2822" => now.to_rfc2822(),
            "iso" => now.to_rfc3339(),
            custom => now.format(custom).to_string(),
        };

        Ok(serde_json::json!({
            "timezone": "Asia/Shanghai (UTC+8)",
            "datetime": now.format("%Y-%m-%d %H:%M:%S").to_string(),
            "iso": now.to_rfc3339(),
            "timestamp": now.timestamp(),
            "timestamp_ms": now.timestamp_millis(),
            "formatted": formatted,
            "year": now.format("%Y").to_string(),
            "month": now.format("%m").to_string(),
            "day": now.format("%d").to_string(),
            "hour": now.format("%H").to_string(),
            "minute": now.format("%M").to_string(),
            "second": now.format("%S").to_string(),
            "weekday": now_weekday_chinese(&now),
        }))
    }

    fn execute_diff(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let from = call.arguments["from_time"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("from_time参数不能为空".to_string()))?;
        let to = call.arguments["to_time"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("to_time参数不能为空".to_string()))?;

        match (
            chrono::DateTime::parse_from_rfc3339(from),
            chrono::DateTime::parse_from_rfc3339(to),
        ) {
            (Ok(from_dt), Ok(to_dt)) => {
                let diff = to_dt.signed_duration_since(from_dt);
                Ok(serde_json::json!({
                    "total_seconds": diff.num_seconds(),
                    "total_minutes": diff.num_minutes(),
                    "total_hours": diff.num_hours(),
                    "total_days": diff.num_days(),
                    "total_weeks": diff.num_weeks(),
                    "days": diff.num_days(),
                    "hours": diff.num_hours() % 24,
                    "minutes": diff.num_minutes() % 60,
                    "seconds": diff.num_seconds() % 60,
                    "human_readable": format_duration(diff),
                }))
            }
            _ => Err(agent_teams_core::error::AgentTeamsError::NotFound(
                "时间格式无效，请使用ISO 8601格式：2024-01-01T00:00:00Z".to_string(),
            )),
        }
    }

    fn execute_format(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let time_str = call.arguments["from_time"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("from_time参数不能为空".to_string()))?;
        let format = call.arguments["format"].as_str().unwrap_or("%Y-%m-%d %H:%M:%S");

        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time_str) {
            let beijing = dt.with_timezone(&beijing_offset());
            Ok(serde_json::json!({
                "formatted": beijing.format(format).to_string(),
                "original": time_str,
                "timezone": "Asia/Shanghai (UTC+8)",
                "format": format,
            }))
        } else if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S") {
            let beijing = beijing_offset().from_local_datetime(&naive)
                .single()
                .ok_or_else(|| agent_teams_core::error::AgentTeamsError::Internal(
                    format!("无法确定时间 '{}' 的时区映射", time_str)
                ))?;
            Ok(serde_json::json!({
                "formatted": beijing.format(format).to_string(),
                "original": time_str,
                "timezone": "Asia/Shanghai (UTC+8)",
                "format": format,
            }))
        } else {
            Err(agent_teams_core::error::AgentTeamsError::NotFound(
                "无法解析时间，请使用ISO 8601格式".to_string(),
            ))
        }
    }

    fn execute_from_unix(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let ts = call.arguments["timestamp"]
            .as_i64()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("timestamp参数不能为空".to_string()))?;

        match chrono::DateTime::from_timestamp(ts, 0) {
            Some(utc) => {
                let beijing = utc.with_timezone(&beijing_offset());
                Ok(serde_json::json!({
                    "timezone": "Asia/Shanghai (UTC+8)",
                    "datetime": beijing.format("%Y-%m-%d %H:%M:%S").to_string(),
                    "iso": beijing.to_rfc3339(),
                    "timestamp": ts,
                    "weekday": now_weekday_chinese(&beijing),
                }))
            }
            None => Err(agent_teams_core::error::AgentTeamsError::NotFound("无效的时间戳".to_string())),
        }
    }

    fn execute_to_unix(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let time_str = call.arguments["time"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("time参数不能为空".to_string()))?;

        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time_str) {
            Ok(serde_json::json!({
                "timestamp": dt.timestamp(),
                "timestamp_ms": dt.timestamp_millis(),
                "original": time_str,
            }))
        } else if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S") {
            let beijing = beijing_offset().from_local_datetime(&naive)
                .single()
                .ok_or_else(|| agent_teams_core::error::AgentTeamsError::Internal(
                    format!("无法确定时间 '{}' 的时区映射", time_str)
                ))?;
            Ok(serde_json::json!({
                "timestamp": beijing.timestamp(),
                "timestamp_ms": beijing.timestamp_millis(),
                "original": time_str,
                "assumed_timezone": "Asia/Shanghai (UTC+8)",
            }))
        } else {
            Err(agent_teams_core::error::AgentTeamsError::NotFound(
                "无法解析时间，支持格式：ISO 8601 或 '2024-01-01 12:00:00'（北京时间）".to_string(),
            ))
        }
    }

    fn execute_convert(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let time_str = call.arguments["from_time"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("from_time参数不能为空".to_string()))?;
        let timezone = call.arguments["timezone"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("timezone参数不能为空".to_string()))?;

        let dt = chrono::DateTime::parse_from_rfc3339(time_str)
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S")
                .map(|naive| beijing_offset().from_local_datetime(&naive).single().unwrap().fixed_offset()))
            .map_err(|_| agent_teams_core::error::AgentTeamsError::NotFound("无法解析时间".to_string()))?;

        // Map common timezone names to offsets
        let offset_seconds = match timezone.to_lowercase().as_str() {
            "utc" | "gmt" => 0,
            "est" | "america/new_york" => -5 * 3600,
            "cst" | "america/chicago" => -6 * 3600,
            "pst" | "america/los_angeles" => -8 * 3600,
            "cet" | "europe/paris" | "europe/berlin" => 3600,
            "jst" | "asia/tokyo" => 9 * 3600,
            "kst" | "asia/seoul" => 9 * 3600,
            "ist" | "asia/kolkata" => 5 * 3600 + 1800,
            "aest" | "australia/sydney" => 10 * 3600,
            "sgt" | "asia/singapore" => 8 * 3600,
            "hkt" | "asia/hong_kong" => 8 * 3600,
            "cst_china" | "asia/shanghai" | "beijing" => 8 * 3600,
            _ => {
                // Try to parse as +HH:MM or -HH:MM
                if timezone.starts_with('+') || timezone.starts_with('-') {
                    let sign = if timezone.starts_with('-') { -1 } else { 1 };
                    let parts: Vec<&str> = timezone[1..].split(':').collect();
                    if parts.len() == 2 {
                        let hours: i32 = parts[0].parse().unwrap_or(0);
                        let minutes: i32 = parts[1].parse().unwrap_or(0);
                        sign * (hours * 3600 + minutes * 60)
                    } else {
                        return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                            format!("不支持的时区: {}，请使用标准时区名称或 +/-HH:MM 格式", timezone)
                        ));
                    }
                } else {
                    return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                        format!("不支持的时区: {}，请使用 UTC、Asia/Tokyo 等标准名称", timezone)
                    ));
                }
            }
        };

        let target_offset = chrono::FixedOffset::east_opt(offset_seconds)
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("无效时区偏移".to_string()))?;
        let converted = dt.with_timezone(&target_offset);

        Ok(serde_json::json!({
            "original": time_str,
            "original_timezone": "Asia/Shanghai (UTC+8)",
            "converted": converted.format("%Y-%m-%d %H:%M:%S").to_string(),
            "iso": converted.to_rfc3339(),
            "target_timezone": timezone,
            "offset_hours": offset_seconds as f64 / 3600.0,
        }))
    }

    fn execute_parse(&self, call: &ToolCall) -> Result<serde_json::Value> {
        let text = call.arguments["text"]
            .as_str()
            .ok_or_else(|| agent_teams_core::error::AgentTeamsError::NotFound("text参数不能为空".to_string()))?;

        let now = now_beijing();
        let lower = text.to_lowercase();

        // Simple relative time parsing
        let parsed = if lower.contains("明天") {
            now + chrono::Duration::days(1)
        } else if lower.contains("后天") {
            now + chrono::Duration::days(2)
        } else if lower.contains("昨天") {
            now - chrono::Duration::days(1)
        } else if lower.contains("前天") {
            now - chrono::Duration::days(2)
        } else if lower.contains("下周") {
            now + chrono::Duration::weeks(1)
        } else if lower.contains("上周") {
            now - chrono::Duration::weeks(1)
        } else if lower.contains("下个月") {
            now + chrono::Duration::days(30)
        } else if lower.contains("上个月") {
            now - chrono::Duration::days(30)
        } else if lower.contains("小时后") || lower.contains("小时以后") {
            let hours: i64 = lower.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(1);
            now + chrono::Duration::hours(hours)
        } else if lower.contains("分钟前") {
            let mins: i64 = lower.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(1);
            now - chrono::Duration::minutes(mins)
        } else if lower.contains("小时后") {
            let hours: i64 = lower.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(1);
            now + chrono::Duration::hours(hours)
        } else {
            return Err(agent_teams_core::error::AgentTeamsError::NotFound(
                format!("无法解析时间描述: '{}'，支持：明天/后天/昨天/下周/上周/N小时后/N分钟前", text)
            ));
        };

        // Parse time of day if mentioned
        let mut result = parsed;
        if lower.contains("早上") || lower.contains("上午") {
            result = result.with_hour(9).unwrap_or(result).with_minute(0).unwrap_or(result);
        } else if lower.contains("中午") {
            result = result.with_hour(12).unwrap_or(result).with_minute(0).unwrap_or(result);
        } else if lower.contains("下午") {
            // Try to extract hour
            let hour_str: String = lower.chars().filter(|c| c.is_ascii_digit()).collect();
            let hour = hour_str.parse::<u32>().unwrap_or(3);
            let hour = if hour < 12 { hour + 12 } else { hour };
            result = result.with_hour(hour.min(23)).unwrap_or(result).with_minute(0).unwrap_or(result);
        } else if lower.contains("晚上") {
            let hour_str: String = lower.chars().filter(|c| c.is_ascii_digit()).collect();
            let hour = hour_str.parse::<u32>().unwrap_or(8);
            let hour = if hour < 12 { hour + 12 } else { hour };
            result = result.with_hour(hour.min(23)).unwrap_or(result).with_minute(0).unwrap_or(result);
        }

        Ok(serde_json::json!({
            "input": text,
            "parsed": result.format("%Y-%m-%d %H:%M:%S").to_string(),
            "iso": result.to_rfc3339(),
            "timestamp": result.timestamp(),
            "weekday": now_weekday_chinese(&result),
            "timezone": "Asia/Shanghai (UTC+8)",
        }))
    }
}

fn now_weekday_chinese(dt: &chrono::DateTime<FixedOffset>) -> String {
    match dt.weekday() {
        chrono::Weekday::Mon => "周一", chrono::Weekday::Tue => "周二",
        chrono::Weekday::Wed => "周三", chrono::Weekday::Thu => "周四",
        chrono::Weekday::Fri => "周五", chrono::Weekday::Sat => "周六",
        chrono::Weekday::Sun => "周日",
    }.to_string()
}

fn format_duration(duration: chrono::Duration) -> String {
    let total_seconds = duration.num_seconds().abs();
    let days = total_seconds / 86400;
    let hours = (total_seconds % 86400) / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    let mut parts = Vec::new();
    if days > 0 { parts.push(format!("{}天", days)); }
    if hours > 0 { parts.push(format!("{}小时", hours)); }
    if minutes > 0 { parts.push(format!("{}分钟", minutes)); }
    if seconds > 0 || parts.is_empty() { parts.push(format!("{}秒", seconds)); }
    let sign = if duration.num_seconds() < 0 { "-" } else { "" };
    format!("{}{}", sign, parts.join(" "))
}

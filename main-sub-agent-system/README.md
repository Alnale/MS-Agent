# Agent Teams

基于 Rust 构建的多智能体编排系统，采用 **Main Agent + Sub Agent** 架构。中央协调器编排多个专业化 AI 智能体协作处理用户请求，通过 HTTP API（SSE 流式）对外提供服务，支持 Anthropic (Claude)、OpenAI、Ollama 等 LLM 提供商。

## 目录

- [核心特性](#核心特性)
- [系统架构](#系统架构)
- [技术栈](#技术栈)
- [项目结构](#项目结构)
- [快速开始](#快速开始)
- [配置说明](#配置说明)
- [API 文档](#api-文档)
- [智能体系统](#智能体系统)
- [工具系统](#工具系统)
- [记忆系统](#记忆系统)
- [缓存系统](#缓存系统)
- [安全机制](#安全机制)
- [部署方式](#部署方式)
- [开发指南](#开发指南)
- [测试](#测试)
- [CI/CD](#cicd)
- [常见问题](#常见问题)
- [许可证](#许可证)

---

## 核心特性

| 特性 | 说明 |
|------|------|
| 多智能体编排 | Main Agent 统筹调度，Sub Agent 各司其职（情感分析、任务规划、摘要等） |
| SSE 流式响应 | 支持 `simple`（纯文本）和 `full`（完整事件）两种流式模式 |
| 扩展思考 | 可配置的 Thinking 预算，支持 Auto/Always/Never 策略 |
| Critic 质量审查 | 自动评审回复质量，支持多轮精炼 |
| 多级记忆系统 | 工作记忆、短期记忆、长期记忆，基于向量嵌入的检索与去重 |
| L1/L2/L3 统一缓存 | 热缓存、温缓存、共享缓存三级缓存体系 |
| 伴侣模式 | 情感状态追踪（心情、好感度、能量、耐心、信任） |
| 预设人格 | 内置猫娘、程序员、故事讲述者等角色预设 |
| 工具执行 | HTTP、文件操作、日期时间、DocFlow 文档处理、媒体处理等 |
| 工具发现 | 支持 MCP（Model Context Protocol）和 OpenAPI 规范自动发现工具 |
| 自适应重规划 | 关键 Sub Agent 失败时自动尝试备选方案 |
| 成本优化 | 简单查询自动跳过 Thinking 和 Critic，降低 Token 消耗 |
| 安全防护 | API Key 常量时间比较、请求验证、注入检测、IP 限流 |
| OpenTelemetry | 分布式追踪，集成 Jaeger |
| 热重载 | 配置文件热重载，无需重启 |
| Docker 部署 | 多阶段构建，开箱即用 |
| Swagger UI | 自动生成 OpenAPI 文档 |

---

## 系统架构

### 主-子智能体架构

系统采用 **Main-Sub Agent** 架构，由中央协调器 `MainAgentCoordinator` 统一调度多个专业化 Sub Agent：

```
用户请求
    │
    ▼
┌─────────────────────────────────────────────────────┐
│                MainAgentCoordinator                  │
│                                                     │
│  1. PreRun Hooks（生命周期钩子）                      │
│  2. 记忆初始化（加载工作记忆）                         │
│  3. 上下文填充（领域状态、实体信息、历史、系统指令）     │
│  4. 计划生成（MainAgent 通过 LLM 生成执行计划）       │
│  5. 基线执行（Sub Agent 并行运行）                    │
│  6. 路由分发（task_planner 决定额外调用哪些 Agent）    │
│  7. 工具执行（task_planner 执行工具调用）              │
│  8. 自适应重规划（失败时尝试备选 Agent）               │
│  9. 效果聚合（合并所有 Agent 结果）                    │
│  10. 综合响应（MainAgent 生成最终回复）                │
│  11. Critic 评审（可选质量审查）                       │
│  12. 记忆同步（结果存储到长期记忆）                     │
│                                                     │
│  ┌──────────┐  ┌───────────────┐  ┌──────────────┐ │
│  │ Sentiment │  │ TaskPlanner   │  │   Summary    │ │
│  │ Sub Agent │  │ Sub Agent     │  │   Sub Agent  │ │
│  │ (情感分析)│  │ (路由+工具)    │  │   (摘要)     │ │
│  └──────────┘  └───────────────┘  └──────────────┘ │
│       │               │                    │         │
│       ▼               ▼                    ▼         │
│  ┌──────────────────────────────────────────────┐   │
│  │           LLM Provider (Anthropic/OpenAI)     │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

### 流式架构

- `handle_request_stream` 启动任务通过无界通道发送进度事件
- 心跳任务在长时间操作期间保持 SSE 连接活跃
- 工具状态事件通过专用通道从 task_planner 实时转发

### 核心设计模式

| 模式 | 应用 |
|------|------|
| 策略模式 | `ExecutionPolicy` 控制 Sub Agent 调用行为 |
| 钩子系统 | `HookRegistry` + `HookPoint`（PreRun/PrePlan/PostPlan/PreCritic/PostCritic 等） |
| 事件总线 | `SystemEvent` 发布/订阅解耦组件 |
| 熔断器 | `CircuitBreakerProvider` 包装 LLM 提供商 |
| 指数退避重试 | `RetryProvider` 装饰器 |
| 管道模式 | 可配置的阶段（Sequential/Parallel） |
| 上下文提供者 | 可组合的 `ContextProvider` trait 组装系统提示 |
| 统一记忆总线 | 跨 Agent 缓存协调，L1/L2/L3 层级 |

---

## 技术栈

| 类别 | 技术 |
|------|------|
| 语言 | Rust (edition 2021) |
| Web 框架 | Axum 0.8 + Tower 0.5 + Tower-HTTP 0.6 |
| 异步运行时 | Tokio |
| HTTP 客户端 | Reqwest 0.12（支持 JSON、流式、gzip、brotli、zstd、multipart） |
| 序列化 | Serde + Serde JSON |
| 流处理 | Futures 0.3、Tokio-Stream、Async-Stream |
| OpenAPI | Utoipa 5 + Utoipa-Swagger-UI 9 |
| 并发 | DashMap 6、LRU 0.12 |
| 可观测性 | Tracing + Tracing-Subscriber + Tracing-OpenTelemetry + OpenTelemetry-Jaeger |
| 缓存/状态 | Redis 0.27（支持 tokio-comp、connection-manager、json） |
| 加密 | Secrecy 0.8 |
| 配置 | Dotenvy 0.15，支持 `${VAR_NAME}` 环境变量插值 |
| UUID | UUID 1（v4） |
| 时间 | Chrono 0.4 |
| 压缩 | Flate2、Encoding-RS |
| 模板 | Include-Dir（编译时嵌入前端 dist） |
| 容器化 | Docker 多阶段构建（Rust 1.82-bookworm → Debian bookworm-slim） |

---

## 项目结构

```
main-sub-agent-system/
├── Cargo.toml                 # 工作区根配置
├── config.json                # 主配置文件
├── .env                       # 环境变量（API Key、模型配置）
├── Dockerfile                 # 多阶段 Docker 构建
├── docker-compose.yml         # Docker Compose 服务定义
│
├── core/                      # agent-teams-core：共享类型、trait、记忆系统
│   └── src/
│       ├── lib.rs             # 公共类型重导出
│       ├── config.rs          # AppConfig 及所有配置结构体
│       ├── message.rs         # AgentMessage、StreamChunk、TaskType
│       ├── boxed_agent.rs     # AgentInput、AgentOutput、BoxedAgent trait
│       ├── memory.rs          # MemoryEntry、MemoryKind、MemoryConfig
│       ├── memory_store.rs    # MemoryStore trait、EmbeddingProvider
│       ├── tool.rs            # Tool、ToolBuilder、UnifiedToolRegistry、ToolPolicyEngine
│       ├── hook.rs            # HookPoint、HookRegistry（生命周期钩子）
│       ├── pipeline.rs        # PipelineDef、PipelineStage、StageMode
│       ├── plan.rs            # ExecutionPlan、PlanNode、PlanStage
│       ├── provider.rs        # LlmProvider trait、CompletionRequest/Response
│       ├── registry.rs        # AgentRegistry
│       ├── context.rs         # AgentContext、上下文提供者
│       ├── bus.rs             # AgentBus、AgentEnvelope
│       ├── effect.rs          # AgentEffect 枚举
│       ├── event.rs           # EventBus、SystemEvent
│       ├── state.rs           # StateStore trait
│       ├── routing.rs         # RoutingTable、RoutingRule
│       ├── companion.rs       # CompanionState、CompanionDelta（情感状态）
│       ├── unified_cache_manager.rs
│       └── unified_memory_bus.rs
│
├── provider/                  # agent-teams-provider：LLM 提供商实现
│   └── src/
│       ├── anthropic.rs       # Anthropic/Claude 提供商（SSE 流式）
│       ├── openai.rs          # OpenAI 提供商
│       ├── ollama.rs          # Ollama 本地提供商
│       ├── http_provider.rs   # HTTP 提供商基类
│       ├── circuit_breaker.rs # 熔断器模式
│       ├── retry.rs           # 指数退避重试
│       ├── sse_buffer.rs      # SSE 事件缓冲
│       ├── embedding.rs       # 嵌入提供商
│       ├── cached_embedding.rs # 缓存嵌入
│       └── registry.rs        # 提供商注册表
│
├── agents/                    # agent-teams-agents：智能体实现与工具
│   └── src/
│       ├── main_agent.rs      # MainAgent（综合、规划）
│       ├── response_agent.rs  # 响应智能体
│       ├── prompt.rs          # 提示词构建
│       ├── quality.rs         # 质量评估
│       ├── agent_factory.rs   # 智能体工厂
│       ├── tool_engine.rs     # 工具执行引擎
│       ├── tool_param_infer.rs # 工具参数推断
│       ├── extractor.rs       # 提取器
│       ├── change_detector.rs # 变更检测
│       ├── decision_store.rs  # 决策存储
│       ├── domain_cs.rs       # 领域上下文
│       ├── sub_agents/
│       │   ├── sentiment.rs        # 情感分析 Sub Agent
│       │   ├── task_planner.rs     # 任务规划 Agent（路由+工具执行）
│       │   ├── script_exec.rs      # 脚本工具执行器
│       │   ├── knowledge.rs        # 知识 Sub Agent
│       │   ├── tool_agent.rs       # 工具 Agent
│       │   └── summary/            # 摘要子智能体（含链优化器+质量检查器）
│       ├── tools/
│       │   ├── http.rs        # HTTP 工具
│       │   ├── file.rs        # 文件操作工具
│       │   ├── datetime.rs    # 日期时间工具
│       │   ├── xxt.rs         # XxtToolExecutor
│       │   ├── docflow.rs     # DocFlow 文档处理工具
│       │   ├── docreader.rs   # 文档读取器
│       │   └── media.rs       # 媒体工具
│       └── tool_discovery/
│           ├── mcp.rs         # MCP（Model Context Protocol）服务器支持
│           └── openapi.rs     # OpenAPI 规范工具发现
│
├── coordinator/               # agent-teams-coordinator：编排逻辑
│   └── src/
│       ├── main_coordinator.rs   # 主协调器（~1800 行）
│       ├── plan_executor.rs      # 管道执行器
│       ├── orchestrator.rs       # 编排器
│       ├── synthesizer.rs        # 结果综合
│       ├── critic.rs             # Critic 智能体（质量审查）
│       ├── aggregator.rs         # 效果聚合器
│       ├── fanout.rs             # 扇出调度
│       ├── cache.rs              # 响应缓存
│       ├── plan_cache.rs         # 计划缓存
│       ├── sub_agent_cache.rs    # Sub Agent 缓存
│       ├── memory_manager.rs     # 记忆管理器
│       ├── memory_context_provider.rs
│       ├── memory_helpers.rs
│       ├── memory_metrics.rs
│       ├── compression_evaluator.rs
│       ├── bus_dispatcher.rs     # 总线调度器
│       ├── state_applier.rs      # 状态应用器
│       ├── summary_background.rs # 后台摘要
│       ├── unified_cache_manager.rs
│       └── cache_metrics.rs
│
├── storage/                   # agent-teams-storage：持久化后端
│   └── src/
│       ├── lib.rs             # InMemoryStateStore
│       ├── memory.rs          # InMemoryMemoryStore
│       ├── redis.rs           # Redis 状态存储
│       └── redis_memory.rs    # Redis 记忆存储
│
├── runtime/                   # agent-teams-runtime：HTTP 服务器与运行时
│   └── src/
│       ├── bin/server.rs      # 二进制入口（main 函数）
│       ├── http.rs            # Axum 路由与所有 HTTP 处理器
│       ├── events.rs          # SSE 事件类型
│       ├── runtime.rs         # RuntimeBuilder
│       ├── sessions.rs        # 会话管理端点
│       ├── validation.rs      # 请求验证
│       ├── rate_limit.rs      # IP 限流器
│       ├── hot_reload.rs      # 配置热重载
│       ├── telemetry.rs       # OpenTelemetry + Jaeger 集成
│       └── lib.rs             # 环境变量解析辅助
│
├── Test/                      # 测试数据（JSON 消息样本）
├── examples/                  # 使用示例
│   └── system_instructions.rs # 系统指令 API 用法示例
├── tools/DocFlow/             # DocFlow 文档处理工具（Node.js 子项目）
├── docs/                      # 文档目录
├── logs/                      # 日志目录
└── .github/workflows/
    └── ci.yml                 # GitHub Actions CI 流水线
```

---

## 快速开始

### 前置条件

- Rust 工具链（stable）
- Redis（可选，用于持久化记忆/状态）
- LLM API Key（Anthropic、OpenAI 或 Ollama）

### 安装与运行

**方式一：从源码构建**

```bash
# 1. 克隆仓库
git clone <repo-url>
cd main-sub-agent-system

# 2. 配置环境变量（在 .env 文件中或直接 export）
export ANTHROPIC_API_KEY=your_key_here
export ANTHROPIC_BASE_URL=https://api.anthropic.com
export DEFAULT_MODEL=claude-3-5-sonnet-20241022

# 3. 构建工作区
cargo build --release

# 4. 启动服务
cargo run --release -p agent-teams-runtime --bin agent_server
```

**方式二：Docker 部署**

```bash
docker-compose up
# 服务将在 0.0.0.0:3000 启动
```

### 验证服务

```bash
curl http://localhost:3000/health
```

---

## 配置说明

主配置文件为 `config.json`，支持 `${VAR_NAME}` 语法引用环境变量。

### 提供商配置（providers）

```json
{
  "providers": {
    "anthropic": {
      "enabled": true,
      "base_url": "${ANTHROPIC_BASE_URL}",
      "api_key": "${ANTHROPIC_API_KEY}",
      "model": "${DEFAULT_MODEL}",
      "timeout_secs": 300,
      "max_retries": 3,
      "circuit_breaker": {
        "failure_threshold": 5,
        "reset_timeout_secs": 60
      }
    },
    "openai": {
      "enabled": false,
      "base_url": "https://api.openai.com/v1",
      "api_key": "${OPENAI_API_KEY}",
      "model": "gpt-4o"
    },
    "ollama": {
      "enabled": false,
      "base_url": "http://localhost:11434",
      "model": "llama3"
    }
  }
}
```

### 记忆系统配置（memory）

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `working_memory_limit` | - | 工作记忆条目上限 |
| `short_term_ttl_secs` | - | 短期记忆存活时间（秒） |
| `compression_threshold` | - | 压缩触发阈值 |
| `similarity_threshold` | 0.7 | 向量相似度阈值 |
| `daily_decay_factor` | - | 每日衰减因子 |
| `duplicate_similarity_threshold` | - | 重复检测相似度阈值 |
| `enable_contradiction_detection` | true | 启用矛盾检测 |
| `embedding_cache_size` | - | 嵌入缓存大小 |

### 统一缓存配置（unified_cache）

- **L1 热缓存**：高频访问数据
- **L2 温缓存**：中频访问数据
- **L3 共享缓存**：跨会话共享数据
- 可配置容量、TTL、清理间隔

### Main Agent 配置

| 参数 | 说明 |
|------|------|
| `extended_thinking` | 扩展思考配置（token 预算、策略：Auto/Always/Never） |
| `critic.enabled` | Critic 质量审查开关 |
| `critic.max_refinement_rounds` | 最大精炼轮数 |
| `plan_cache` | 计划缓存（TTL 和容量） |
| `timeout_secs` | 超时时间（默认 300 秒） |
| `max_tokens` | 最大输出 token 数 |
| `temperature` | 温度参数 |

### Sub Agent 配置

每个 Sub Agent 包含以下配置项：

| 参数 | 说明 |
|------|------|
| `expertise` | 专业领域描述 |
| `message_types` | 处理的消息类型 |
| `requires_llm` | 是否需要 LLM |
| `priority` | 优先级（数值越大越优先） |
| `optional` | 是否可选（失败不影响主流程） |
| `thinking` | 扩展思考配置 |

内置 Sub Agent：

| Agent | 优先级 | 功能 |
|-------|--------|------|
| `task_planner` | 110 | 路由分发与工具执行 |
| `sentiment` | 70 | 情感分析 |
| `summary` | 50 | 对话摘要（可选） |

### 运行时配置（runtime）

```json
{
  "runtime": {
    "port": 3000,
    "host": "0.0.0.0",
    "log_level": "info",
    "cors": true,
    "max_concurrent_requests": 100,
    "rate_limit": {
      "window_secs": 60,
      "max_requests": 60
    }
  }
}
```

### 功能开关（features）

```json
{
  "features": {
    "streaming": true,
    "thinking": true,
    "critic": true,
    "caching": true,
    "hot_reload": true,
    "grpc": false
  }
}
```

### 预设人格（presets）

| 名称 | 说明 |
|------|------|
| `catgirl` | 小猫娘角色 |
| `programmer` | 编程专家 |
| `storyteller` | 故事讲述者 |

---

## API 文档

所有路由同时支持根路径（`/`）和版本化路径（`/v1/`）。

### 端点列表

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/chat` | 主聊天端点（SSE 流式） |
| `GET` | `/health` | 健康检查 |
| `GET` | `/tools` | 列出可用工具 |
| `GET` | `/presets` | 列出预设人格 |
| `GET` | `/sessions/{session_id}` | 获取会话指令 |
| `PUT` | `/sessions/{session_id}` | 设置会话指令 |
| `DELETE` | `/sessions/{session_id}` | 删除会话指令 |
| `GET` | `/v1/swagger-ui` | Swagger UI 文档 |
| `GET` | `/v1/openapi.json` | OpenAPI 规范 |
| `*` | `/*` | 静态文件服务（SPA 回退） |

### POST /chat

**请求体：**

```json
{
  "message": "你好，请帮我分析一下这段代码",
  "session_id": "可选的会话ID",
  "recent_history": [],
  "system_instructions": ["你是一个编程助手"],
  "stream_mode": "simple",
  "force_tool": null,
  "companion_mode": false
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message` | string | 是 | 用户消息内容 |
| `session_id` | string | 否 | 会话标识，不传则自动生成 |
| `recent_history` | array | 否 | 最近对话历史 |
| `system_instructions` | array[string] | 否 | 系统指令/人格设定 |
| `stream_mode` | string | 否 | `"simple"`（纯文本）或 `"full"`（完整事件） |
| `force_tool` | string | 否 | 强制使用的工具名称 |
| `companion_mode` | bool | 否 | 是否启用伴侣模式 |

**SSE 事件类型：**

| 事件 | 说明 |
|------|------|
| `delta` | 文本增量（流式输出片段） |
| `done` | 生成完成 |
| `error` | 错误信息 |
| `sub_agent_results` | Sub Agent 执行结果 |
| `tool_status` | 工具执行状态 |
| `agent_progress` | 智能体处理进度 |
| `companion_state` | 伴侣情感状态更新 |

### 示例请求

```bash
# 基础聊天
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "你好"}'

# 流式聊天（simple 模式）
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "解释一下 Rust 的所有权机制", "stream_mode": "simple"}'

# 带会话指令
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{
    "message": "写一个快速排序",
    "session_id": "my-session",
    "system_instructions": ["你是一个资深 Rust 开发者", "回答时给出完整代码示例"]
  }'

# 伴侣模式
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "今天心情不好", "companion_mode": true}'

# 会话指令管理
curl -X PUT http://localhost:3000/sessions/my-session \
  -H "Content-Type: application/json" \
  -d '{"instructions": ["你是一个编程助手", "使用中文回答"]}'

curl http://localhost:3000/sessions/my-session
curl -X DELETE http://localhost:3000/sessions/my-session
```

---

## 智能体系统

### MainAgent

主智能体负责：
- **计划生成**：通过 LLM 生成 `ExecutionPlan`，经 `PlanCache` 缓存复用
- **综合响应**：综合所有 Sub Agent 输出生成最终回复
- **自适应重规划**：关键 Agent 失败时尝试备选方案

### Sub Agents

| Agent | 文件 | 功能 |
|-------|------|------|
| SentimentSubAgent | `agents/src/sub_agents/sentiment.rs` | 情感分析，始终参与基线执行 |
| TaskPlannerAgent | `agents/src/sub_agents/task_planner.rs` | 路由分发与工具执行，优先级最高（110） |
| SummarySubAgent | `agents/src/sub_agents/summary/` | 对话摘要，含链优化器和质量检查器 |
| KnowledgeSubAgent | `agents/src/sub_agents/knowledge.rs` | 知识检索 |
| ScriptToolExecutor | `agents/src/sub_agents/script_exec.rs` | 脚本执行 |
| ToolAgent | `agents/src/sub_agents/tool_agent.rs` | 工具调用 |

### Critic Agent

- 对 MainAgent 的输出进行质量评审
- 支持多轮精炼（可配置最大轮数）
- 简单查询自动跳过（成本优化）

### 管道执行流程

```
阶段 1: baseline（并行）
  └── Sentiment Sub Agent（始终运行）

阶段 2: task_planner_routed（动态填充）
  └── TaskPlanner 决定调用的 Agent + 执行工具

阶段 3: respond（顺序）
  └── MainAgent 综合所有结果生成回复
```

---

## 工具系统

### 内置工具

| 工具 | 文件 | 功能 |
|------|------|------|
| HTTP | `agents/src/tools/http.rs` | HTTP 请求执行 |
| File | `agents/src/tools/file.rs` | 文件读写操作 |
| DateTime | `agents/src/tools/datetime.rs` | 日期时间查询与格式化 |
| DocFlow | `agents/src/tools/docflow.rs` | 文档处理（Markdown 转思维导图等） |
| DocReader | `agents/src/tools/docreader.rs` | 文档内容读取 |
| Media | `agents/src/tools/media.rs` | 媒体处理 |
| Xxt | `agents/src/tools/xxt.rs` | XxtToolExecutor |

### 工具发现

- **MCP（Model Context Protocol）**：`agents/src/tool_discovery/mcp.rs`，支持动态发现 MCP 服务器提供的工具
- **OpenAPI 规范**：`agents/src/tool_discovery/openapi.rs`，从 OpenAPI 规范自动提取工具定义

### 工具引擎

`ToolExecutionEngine`（`agents/src/tool_engine.rs`）负责：
- 工具参数推断与解析
- 工具策略引擎（`ToolPolicyEngine`）
- 工具调用执行与结果处理

### 查询复杂度检测

系统内置简单查询检测机制，通过加权评分判断是否跳过 Thinking 和 Critic，降低简单问题的 Token 消耗。

---

## 记忆系统

### 记忆层级

| 层级 | 说明 | 特性 |
|------|------|------|
| 工作记忆 | 当前会话上下文 | 有条目上限，最快速访问 |
| 短期记忆 | 跨会话临时记忆 | 有 TTL，自动过期 |
| 长期记忆 | 持久化知识存储 | 基于向量嵌入检索，支持衰减和矛盾检测 |

### 核心能力

- **向量嵌入检索**：基于 `EmbeddingProvider` 的语义相似度搜索
- **记忆衰减**：`daily_decay_factor` 控制记忆随时间衰减
- **矛盾检测**：`enable_contradiction_detection` 自动识别矛盾信息
- **去重**：`duplicate_similarity_threshold` 防止重复记忆
- **压缩评估**：`CompressionEvaluator` 评估何时压缩记忆

### 持久化后端

| 后端 | 说明 |
|------|------|
| InMemory | 内存存储（默认，开发/测试用） |
| Redis | Redis 持久化（生产推荐） |

---

## 缓存系统

### 三级缓存

```
L1 热缓存（Hot）    ← 最近高频访问，容量最小，速度最快
    ↓ 未命中
L2 温缓存（Warm）   ← 中频访问，容量适中
    ↓ 未命中
L3 共享缓存（Shared）← 跨会话共享，容量最大
```

### 缓存类型

| 缓存 | 说明 |
|------|------|
| ResponseCache | 响应结果缓存 |
| PlanCache | 执行计划缓存（复用相似查询的计划） |
| SubAgentCache | Sub Agent 结果缓存 |
| EmbeddingCache | 嵌入向量缓存 |
| CrossSessionCache | 跨会话共享缓存 |

### 指标监控

`CacheMetrics` 提供缓存命中率、未命中率等监控指标。

---

## 安全机制

### API 认证

- API Key 使用常量时间比较（防止时序攻击）
- 通过环境变量配置

### 请求验证

- 输入清洗与消毒
- 注入检测（SQL 注入、XSS 等）
- 请求体大小限制

### 限流

- 基于 IP 的速率限制
- 可配置时间窗口和最大请求数

### 安全配置

```json
{
  "security": {
    "api_key": "${API_KEY}",
    "rate_limit": {
      "window_secs": 60,
      "max_requests": 60
    }
  }
}
```

---

## 部署方式

### Docker 部署

```bash
# 构建并启动
docker-compose up -d

# 查看日志
docker-compose logs -f

# 停止
docker-compose down
```

Dockerfile 采用多阶段构建：
- **构建阶段**：Rust 1.82-bookworm
- **运行阶段**：Debian bookworm-slim（最小化镜像）

### 生产环境建议

1. 配置 Redis 作为持久化后端
2. 设置合理的 `max_concurrent_requests`
3. 配置 IP 限流防止滥用
4. 启用 OpenTelemetry 追踪
5. 使用反向代理（Nginx/Caddy）处理 TLS
6. 配置日志收集

---

## 开发指南

### 本地开发

```bash
# 开发模式构建（更快编译，调试信息）
cargo build

# 运行测试
cargo test --workspace

# 代码格式化
cargo fmt

# 代码检查
cargo clippy --workspace -- -D warnings
```

### 新增 Sub Agent

1. 在 `agents/src/sub_agents/` 下创建新的 Agent 文件
2. 实现 `BoxedAgent` trait
3. 在 `config.json` 的 `sub_agents` 配置中注册
4. 在管道配置中添加到对应阶段

### 新增工具

1. 在 `agents/src/tools/` 下创建新的工具文件
2. 实现 `Tool` trait（通过 `ToolBuilder` 构建）
3. 在 `UnifiedToolRegistry` 中注册
4. 工具会被 TaskPlanner 自动调用

### 扩展 LLM 提供商

1. 在 `provider/src/` 下创建新的提供商文件
2. 实现 `LlmProvider` trait
3. 在提供商注册表中注册
4. 在 `config.json` 的 `providers` 中添加配置

### 热重载

启用 `features.hot_reload = true` 后，修改 `config.json` 无需重启服务即可生效。

---

## 测试

### 运行测试

```bash
# 运行全部测试
cargo test --workspace

# 运行特定 crate 测试
cargo test -p agent-teams-core
cargo test -p agent-teams-agents
```

### 测试覆盖

| 模块 | 测试内容 |
|------|----------|
| `core/src/config.rs` | 14 个配置验证测试（空版本、零端口、无效日志级别、Thinking 设置、Critic 设置、计划缓存、超时、安全） |
| `agents/src/tests/` | 新架构集成测试 |
| `Test/` | JSON 消息样本测试数据 |

### 开发测试辅助

- `HashEmbeddingProvider`（`runtime/src/runtime.rs`）：基于特征哈希的 TF-IDF 嵌入，无需外部 API 即可开发测试

---

## CI/CD

### GitHub Actions

`.github/workflows/ci.yml` 定义了 4 个 CI 任务：

| 任务 | 说明 |
|------|------|
| `cargo check` | 编译检查 |
| `cargo test` | 运行测试套件 |
| `cargo fmt` | 代码格式检查 |
| `cargo clippy` | 代码质量检查 |

---

## 常见问题

### Q: 如何切换 LLM 提供商？

在 `.env` 文件中修改对应的环境变量，并在 `config.json` 中启用对应的提供商、禁用其他提供商。

### Q: 不使用 Redis 可以吗？

可以。默认使用内存存储（InMemory），适合开发和测试。生产环境建议使用 Redis 以获得持久化能力。

### Q: 如何调整智能体行为？

修改 `config.json` 中对应智能体的配置：
- 调整 `priority` 改变执行优先级
- 设置 `optional: true` 使 Agent 失败不影响主流程
- 修改 `expertise` 改变专业领域描述
- 配置 `thinking` 调整思考深度

### Q: SSE 流式模式如何选择？

- `simple`：仅输出文本增量，适合外部消费者
- `full`：输出所有事件（进度、工具状态、Sub Agent 结果等），适合内部前端

### Q: 如何添加自定义人格预设？

在 `config.json` 的 `presets` 部分添加新条目，定义 `name` 和 `instructions`（系统指令数组）。

### Q: 成本如何优化？

1. 启用 `cost_optimization` 自动跳过简单查询的 Thinking 和 Critic
2. 使用 PlanCache 复用相似查询的执行计划
3. 配置合理的缓存 TTL 减少重复调用
4. 选择合适的模型（简单任务用小模型）

---

## 许可证

请参阅项目根目录下的 LICENSE 文件。

# 后端重构方案：提取可复用工具/Agent SDK

> **Status: Phase 0-3, 5 已完成 (4/6 phases)**。Phase 4 (crate 重命名) 为可选，当前不影响使用。

## 目标

将当前单体后端拆分为 **3 层独立 crate**，使外部项目只需一个 `Cargo.toml` 依赖即可：
- 注册自定义工具
- 接入任意 LLM 后端
- 运行工具循环（ReAct pattern）
- 无需关心多 Agent 编排、HTTP、SSE 等上层逻辑

## 当前依赖图（干净 DAG，无循环）

```
core (leaf, 0 reverse deps)
  ↑
provider (depends only on core)
  ↑
storage (depends on core only, phantom dep on provider 需移除)
agents (depends on core only, 288 imports)
coordinator (depends on core + agents, 耦合面极窄: 仅 MainAgent + MemoryQualityInspector)
  ↑
runtime (depends on all above, AppState 是 god-struct)
```

## 重构后目标架构

```
┌─────────────────────────────────────────────────┐
│  应用层 (保持不变)                                │
│  runtime + coordinator + agents (app-specific)   │
└───────────────────┬─────────────────────────────┘
                    │ depends on
┌───────────────────┴─────────────────────────────┐
│  agent-toolkit (新 crate, 可独立发布)             │
│  ToolExecutionEngine + AgentToolLoop             │
│  + ParameterInferrer trait                       │
└───────┬───────────────────┬─────────────────────┘
        │ depends on        │ depends on
┌───────┴───────┐   ┌──────┴──────────┐
│  agent-llm    │   │  agent-core     │
│  (provider)   │   │  (现有 core)     │
│  OpenAI/      │   │  Tool/ToolCall  │
│  Anthropic/   │   │  LlmProvider    │
│  Ollama       │   │  AgentOutput    │
└───────────────┘   └─────────────────┘
```

### 3 层 crate 定义

| Crate | 发布名 | 内容 | 外部依赖 |
|-------|--------|------|----------|
| `core` | `agent-core` | 类型定义 + trait（Tool, LlmProvider, AgentOutput, ToolExecutor, UnifiedToolRegistry） | serde, tokio, dashmap |
| `provider` | `agent-llm` | LlmProvider 实现（OpenAI, Anthropic, Ollama）+ circuit breaker + retry | agent-core, reqwest |
| **新** `toolkit` | `agent-toolkit` | ToolExecutionEngine + AgentToolLoop + ParameterInferrer trait | agent-core, agent-llm |

外部项目用法：
```toml
[dependencies]
agent-toolkit = "0.1"
agent-llm = "0.1"       # 可选，自定义 provider 可不用
```

```rust
use agent_core::tool::{ToolBuilder, ToolExecutor, ToolCall, ToolResult, tool_success};
use agent_toolkit::{ToolExecutionEngine, AgentToolLoop};
use agent_llm::openai::OpenAiProvider;

// 1. 定义工具
struct MyTool;
#[async_trait]
impl ToolExecutor for MyTool {
    fn executor_id(&self) -> &str { "my_tool" }
    fn list_tools(&self) -> Vec<Tool> { vec![ToolBuilder::new("my_tool").description("...").build()] }
    async fn execute(&self, call: &ToolCall, _ctx: &ToolExecutionContext) -> Result<ToolResult> {
        Ok(tool_success(call, serde_json::json!({"result": "ok"}), 0))
    }
}

// 2. 注册 + 运行
let registry = UnifiedToolRegistry::new();
registry.register_executor(Arc::new(MyTool));
let provider = OpenAiProvider::new("https://api.openai.com/v1", &key, "gpt-4o");
let engine = ToolExecutionEngine::new(registry.clone());
let mut loop = AgentToolLoop::new(Arc::new(provider), Arc::new(engine));
let (output, history) = loop.run(messages, registry.list_tools(), &ctx).await?;
```

---

## 分步实施计划

### Phase 0: 清理（0.5 天）

| # | 改动 | 文件 | 风险 |
|---|------|------|------|
| 0.1 | 移除 storage 对 provider 的 phantom dependency | `storage/Cargo.toml` | 无 |
| 0.2 | 将 `agents/src/tool_engine.rs` 拆为两个文件 | `tool_engine.rs` → `tool_engine/mod.rs` + `tool_engine/engine.rs` + `tool_engine/agent_loop.rs` | 低，纯重命名 |

### Phase 1: 提取 ToolExecutionEngine 到 core（1 天）

**前提**：ToolExecutionEngine 的所有字段类型已经在 core 中定义。

| # | 改动 | 文件 |
|---|------|------|
| 1.1 | 将 `ToolExecutionEngine` + `CircuitBreakerState` + `ToolMetrics` + `PerToolMetrics` 从 `agents/src/tool_engine/engine.rs` 移到 `core/src/tool_engine.rs` | core/src/tool_engine.rs (新), agents/src/tool_engine/engine.rs (删除) |
| 1.2 | 在 `core/src/lib.rs` 添加 `pub mod tool_engine;` 并 re-export | core/src/lib.rs |
| 1.3 | 将 `RetryPolicy` 字段类型从 agents 内部类型改为 core 已有的 `RetryPolicy`（已存在） | 无额外改动 |
| 1.4 | 更新 agents/coordinator/runtime 中所有 `use agent_teams_agents::tool_engine::ToolExecutionEngine` 为 `use agent_teams_core::tool_engine::ToolExecutionEngine` | agents/ (2处), runtime/ (2处) |

**验证**：`cargo check` 通过，现有功能不受影响。

### Phase 2: 提取 ParameterInferrer trait 到 core（1 天）

**当前问题**：AgentToolLoop 依赖 `ParameterInferrer`（具体实现），阻止了它独立于 agents crate。

| # | 改动 | 文件 |
|---|------|------|
| 2.1 | 在 core 中定义 trait：`core/src/tool_param_infer.rs` | core/src/tool_param_infer.rs (新) |
| 2.2 | 将 agents 中的 `ParameterInferrer` 改为实现该 trait | agents/src/tool_param_infer.rs |
| 2.3 | AgentToolLoop 的 `param_inferrer` 字段类型改为 `Option<Arc<dyn ParamInferrer>>` | agents/src/tool_engine/agent_loop.rs |

**trait 定义**：
```rust
// core/src/tool_param_infer.rs
#[async_trait]
pub trait ParamInferrer: Send + Sync {
    async fn infer(&self, call: &ToolCall, history: &[(ToolCall, ToolResult)]) -> ToolCall;
}
```

### Phase 3: 提取 AgentToolLoop 到新 crate `toolkit`（1 天）

| # | 改动 | 文件 |
|---|------|------|
| 3.1 | 创建 `toolkit/Cargo.toml`，依赖 agent-core | toolkit/Cargo.toml (新) |
| 3.2 | 将 `AgentToolLoop` 移到 `toolkit/src/agent_loop.rs` | toolkit/src/agent_loop.rs (新) |
| 3.3 | 创建 `toolkit/src/lib.rs`，re-export | toolkit/src/lib.rs (新) |
| 3.4 | workspace Cargo.toml 添加 `"toolkit"` 成员 | Cargo.toml |
| 3.5 | agents 中改为 `use agent_toolkit::AgentToolLoop` | agents/ (3处: tool_agent.rs, task_planner.rs) |

**验证**：`cargo check`，所有现有测试通过。

### Phase 4: 重命名 crate 为发布名（0.5 天）

| 当前 | 发布名 | 改动 |
|------|--------|------|
| `agent-teams-core` | `agent-core` | Cargo.toml name 字段 |
| `agent-teams-provider` | `agent-llm` | Cargo.toml name 字段 |
| `agent-toolkit` (新) | `agent-toolkit` | 保持 |

**注意**：所有内部 `use agent_teams_core::` 引用需要同步更新为 `use agent_core::`。这是批量替换，可以用 sed 完成。

### Phase 5: 精简 core 导出（可选，1 天）

当前 core 导出 34 个模块，其中很多是应用特化的（memory lifecycle, companion, domain 等）。建议：

| 改动 | 说明 |
|------|------|
| 将 `companion`, `domain`, `dedup_engine`, `memory_lifecycle`, `memory_reranker`, `tag_extractor` 等移动到 `agents` crate | 这些是应用逻辑，不属于通用 SDK |
| core 只保留：tool, provider, error, boxed_agent, context, message, plan, pipeline, effect, registry, routing, hook, event, state, config | 通用基础设施 |

这样 core 的公共 API 从 34 个模块降到 ~15 个，外部用户认知负担大幅降低。

---

## 风险评估

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| 拆文件导致 git blame 丢失 | 高 | 低 | 用 `git mv` 保留历史 |
| Phase 4 批量重命名引入编译错误 | 中 | 中 | 分 crate 逐步替换，每步 cargo check |
| 外部项目的 tool 嵌入到现有 Agent 系统时类型不兼容 | 低 | 高 | 保持 Tool/ToolExecutor trait 不变，这是稳定 API |
| core 精简时误删被 agents 使用的模块 | 中 | 中 | 先 grep 确认使用情况，逐个迁移 |

## 工期估算

| 阶段 | 工作量 | 产出 |
|------|--------|------|
| Phase 0 清理 | 0.5 天 | 干净的文件结构 |
| Phase 1 ToolExecutionEngine | 1 天 | core 包含工具执行引擎 |
| Phase 2 ParamInferrer trait | 1 天 | trait 解耦 |
| Phase 3 toolkit crate | 1 天 | 独立可发布的 toolkit crate |
| Phase 4 重命名 | 0.5 天 | 发布级 crate 名 |
| Phase 5 精简 core | 1 天 | 清晰的公共 API |
| **总计** | **5 天** | 3 个独立可发布的 crate |

## 验收标准

1. 外部项目只需 `agent-toolkit` + `agent-llm` 两个依赖即可运行工具循环
2. `cargo test` 全部通过
3. 现有 HTTP/SSE 功能不受影响
4. `cargo publish --dry-run` 对 3 个 crate 均通过

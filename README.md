# Agent Teams - 多智能体协作系统

> 基于 Rust + React 的多智能体对话系统，支持流式响应、工具调用、情感分析、记忆系统和丰富的媒体交互。

---

## 目录

- [项目简介](#项目简介)
- [系统架构](#系统架构)
- [功能特性](#功能特性)
- [快速开始](#快速开始)
- [环境配置](#环境配置)
- [API 接口](#api-接口)
- [内置工具](#内置工具)
- [预设人格](#预设人格)
- [前端功能详解](#前端功能详解)
- [后端架构详解](#后端架构详解)
- [配置说明](#配置说明)
- [构建与发布](#构建与发布)
- [项目结构](#项目结构)

---

## 项目简介

Agent Teams 是一个功能完备的多智能体协作对话系统，采用前后端分离架构，最终打包为单个可执行文件。系统通过多个 AI 智能体协同工作，实现情感分析、任务规划、工具调用和高质量对话生成。

### 核心亮点

- **多智能体协作**：情感分析、任务规划、主对话生成三个智能体并行/串行协同工作
- **流式响应**：基于 SSE 的实时流式输出，支持思考过程展示
- **工具调用**：内置 HTTP 请求、文件操作、时间处理、媒体控制、文档处理等工具
- **记忆系统**：工作记忆、短期记忆、相似度检索、矛盾检测、记忆衰减
- **情感陪伴**：实时情感状态追踪，影响回复风格和内容
- **媒体交互**：音乐播放器、背景视频/图片、歌词显示、波形可视化
- **Liquid Glass UI**：基于液态玻璃效果的现代化界面设计

---

## 系统架构

```
┌─────────────────────────────────────────────────────────┐
│                    Agent Teams 架构                       │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  ┌──────────────┐    SSE/WebSocket    ┌──────────────┐  │
│  │   React 前端  │ ◄────────────────► │  Rust 后端   │  │
│  │  (Vite 构建)  │    HTTP REST API   │  (Axum 服务) │  │
│  └──────────────┘                     └──────┬───────┘  │
│                                              │           │
│                        ┌─────────────────────┼────────┐  │
│                        │     智能体协调器      │        │  │
│                        │    (Coordinator)     │        │  │
│                        └─────────┬───────────┘        │  │
│                                  │                     │  │
│              ┌───────────────────┼───────────────────┐  │
│              │                   │                   │  │
│     ┌────────▼────────┐ ┌───────▼───────┐ ┌────────▼┐ │
│     │  情感分析智能体   │ │ 任务规划智能体  │ │ 主智能体 │ │
│     │   (Sentiment)    │ │ (TaskPlanner) │ │  (Main)  │ │
│     └─────────────────┘ └───────┬───────┘ └─────────┘ │
│                                 │                      │
│                    ┌────────────┼────────────┐         │
│                    │            │            │         │
│              ┌─────▼───┐ ┌─────▼───┐ ┌─────▼───┐     │
│              │ HTTP 工具 │ │ 文件工具 │ │ 媒体工具 │     │
│              └─────────┘ └─────────┘ └─────────┘     │
│              ┌─────────┐ ┌─────────┐ ┌─────────┐     │
│              │时间工具  │ │文档工具  │ │学习通   │     │
│              └─────────┘ └─────────┘ └─────────┘     │
│                                                        │
│  ┌──────────────────────────────────────────────────┐  │
│  │              记忆系统 & 缓存总线                    │  │
│  │   工作记忆 │ 短期记忆 │ 相似度检索 │ 统一缓存      │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### 技术栈

| 层级 | 技术 | 说明 |
|------|------|------|
| **前端** | React 19 + TypeScript + Vite 8 | 现代化前端构建 |
| **后端** | Rust + Axum | 高性能异步 HTTP 服务 |
| **LLM** | Anthropic API 兼容 | 支持多种模型 |
| **存储** | 内置内存 + Redis（可选） | 会话持久化 |
| **工具** | Python（嵌入式） | 文档处理、自动化 |

---

## 功能特性

### 智能对话

- **流式响应**：实时输出 AI 回复，支持思考过程展示
- **多会话管理**：创建、切换、删除对话会话，批量操作
- **预设人格**：内置小猫娘、编程猫、故事大王等角色，支持自定义
- **上下文记忆**：基于相似度的上下文检索，自动管理对话记忆

### 工具系统

| 工具 | 功能 |
|------|------|
| **HTTP** | 网页搜索（百度/必应/谷歌）、URL 访问、内容提取、反爬虫检测 |
| **文件** | 读写、列表、复制、移动、删除、glob 模式匹配 |
| **时间** | 当前时间、时区转换、自然语言解析（如"明天下午3点"） |
| **媒体** | 背景图片/视频切换、音乐播放控制、音量调节 |
| **文档转换** | DOC/DOCX ↔ PDF、PDF → Markdown、OCR 识别 |
| **文档阅读** | 读取 PDF/DOCX/DOC 文本内容 |
| **学习通** | 超星学习通自动答题 |

### 媒体交互

- **音乐播放器**：播放列表、音量控制、波形可视化、歌词同步显示
- **背景定制**：图片/视频背景、透明度/模糊度调节、背景切换动画
- **自适应文字**：根据视频背景自动调整文字颜色，保证可读性
- **素材库**：统一管理图片、视频、音乐素材

### 情感陪伴

- **实时情感追踪**：心情、亲密度、活力、耐心、信任度
- **情感驱动回复**：情感状态注入系统提示词，影响 AI 回复风格
- **情绪面板**：可视化展示当前情感状态

### Liquid Glass UI

- **液态玻璃效果**：基于 SVG 滤镜的折射、色散、模糊效果
- **多模式支持**：标准、极地、突出、着色器四种渲染模式
- **分类配置**：遮罩层、信息卡片、按钮分别独立配置
- **亮色适配**：自动适配浅色/深色背景

---

## 快速开始

### 方式一：直接运行（推荐）

1. 从 `release/` 目录运行 `agent-server.exe`
2. 浏览器访问 `http://localhost:3000`
3. 开始对话

### 方式二：开发模式

**启动后端：**

```bash
cd main-sub-agent-system
cargo run --release
```

**启动前端：**

```bash
cd frontend
npm install
npm run dev
```

前端开发服务器运行在 `http://localhost:5173`，自动代理 API 请求到后端。

---

## 环境配置

### 环境变量（`.env`）

```bash
# Anthropic API 配置
ANTHROPIC_API_KEY=your_api_key_here
ANTHROPIC_BASE_URL=https://api.anthropic.com
DEFAULT_MODEL=claude-sonnet-4-20250514

# Python 路径（用于工具执行）
PYTHON_PATH=C:\path\to\python.exe
```

### 支持的 LLM 模型

系统使用 Anthropic API 兼容接口，支持：

- Claude Opus 4 / Sonnet 4 / Haiku 系列
- 任何 Anthropic API 兼容的第三方服务

---

## API 接口

### 聊天（流式 SSE）

```
POST /chat
Content-Type: application/json

{
  "messages": [...],
  "session_id": "optional-session-id",
  "system_instructions": ["可选的系统指令"],
  "preset_id": "可选的预设人格ID"
}
```

响应：`text/event-stream` 格式的 SSE 流

### 系统状态

```
GET /health
```

返回版本号、提供商、模型等信息。

### 工具列表

```
GET /tools
```

返回所有可用工具及其参数 Schema。

### 预设人格

```
GET /presets
```

返回内置和自定义人格列表。

### 会话管理

```
GET    /v1/sessions/{id}    # 获取会话指令
PUT    /v1/sessions/{id}    # 设置会话指令
DELETE /v1/sessions/{id}    # 删除会话
```

### Swagger UI

访问 `/v1/swagger-ui` 查看完整的 API 文档。

---

## 内置工具

### HTTP 工具

```json
{
  "action": "search",
  "query": "搜索关键词",
  "engine": "bing",
  "max_results": 5
}
```

支持的操作：`search`（搜索）、`fetch`（抓取单个 URL）、`batch`（批量抓取）

内容提取模式：`text`、`links`、`meta`、`jsdata`、`all`

### 文件工具

```json
{
  "action": "read",
  "path": "/path/to/file.txt"
}
```

支持的操作：`read`、`write`、`list`、`exists`、`delete`、`info`、`copy`、`move`、`glob`

### 时间工具

```json
{
  "action": "now"
}
```

支持的操作：`now`（当前时间）、`diff`（时间差）、`format`（格式化）、`parse`（自然语言解析）

时区：默认北京时间（UTC+8）

### 媒体工具

```json
{
  "action": "play_music",
  "file_name": "song.mp3"
}
```

支持的操作：`import_and_set_bg_image`、`import_and_play_music`、`play_music`、`pause_music`、`set_volume` 等

### 文档工具

```json
{
  "action": "convert",
  "input_path": "/path/to/doc.docx",
  "conversion_type": "doc_to_pdf"
}
```

支持的转换：`doc_to_pdf`、`pdf_to_docx`、`to_markdown`

---

## 预设人格

### 内置人格

| ID | 名称 | 图标 | 描述 |
|----|------|------|------|
| `catgirl` | 小猫娘 | 🐱 | 香香软软的小猫娘，说话带喵~ |
| `programmer` | 编程猫 | 💻 | 精通编程的技术专家 |
| `storyteller` | 故事大王 | 📖 | 擅长讲故事的叙述者 |

### 自定义人格

通过前端界面创建自定义人格：

1. 点击状态栏的「人格」按钮
2. 点击「创建自定义人格」
3. 填写名称、图标、描述和系统指令
4. 保存后即可在对话中使用

---

## 前端功能详解

### 模块化架构

前端采用模块化设计，主要分为：

```
src/
├── hooks/           # 自定义 Hooks
│   ├── useChat.ts         # 聊天逻辑
│   ├── useSession.ts      # 会话管理
│   ├── usePreset.ts       # 预设人格
│   ├── useBackground.ts   # 背景管理
│   ├── useMusic.ts        # 音乐播放
│   ├── usePlaylist.ts     # 播放列表
│   ├── useMediaLibrary.ts # 素材库
│   ├── useSettings.ts     # 设置管理
│   └── useLyrics.ts       # 歌词解析
├── components/      # UI 组件
│   ├── settings/    # 设置面板子组件
│   ├── music/       # 音乐播放器子组件
│   ├── tools/       # 工具选择器子组件
│   └── presets/     # 预设选择器子组件
├── api/             # API 客户端
└── utils/           # 工具函数
```

### 主要组件

| 组件 | 功能 |
|------|------|
| `MessageBubble` | 聊天气泡，支持 Markdown 渲染 |
| `InputBar` | 输入栏，集成工具选择器 |
| `StatusBar` | 顶部状态栏，集成预设选择 |
| `SettingsPanel` | 设置面板，双栏布局 |
| `MusicPlayer` | 音乐播放器，支持胶囊/展开两种视图 |
| `CompanionPanel` | 情感陪伴面板 |
| `SessionList` | 会话列表侧边栏 |
| `WelcomeScreen` | 欢迎页，带建议提示 |

### 媒体功能

**音乐播放器：**
- 播放/暂停/上一首/下一首
- 播放列表管理（添加/删除/选择）
- 音量控制和静音
- 波形可视化（Web Audio API）
- 歌词同步显示和时间偏移调节
- 素材库集成

**背景系统：**
- 图片/视频背景切换
- 透明度和模糊度调节
- 背景切换过渡动画
- 视频自动播放和解锁
- 自适应文字颜色提取

---

## 后端架构详解

### 智能体管线

```
用户输入
    │
    ▼
┌─────────────────┐
│  Baseline 阶段   │  ← 情感分析（并行）
│  (Sentiment)     │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Task Planner   │  ← 任务规划 + 工具执行（动态并行）
│  阶段            │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Respond 阶段    │  ← 主智能体生成回复（串行）
│  (Main Agent)    │
└────────┬────────┘
         │
         ▼
      流式输出
```

### 记忆系统

- **工作记忆**：当前对话的上下文，限制 50 条，最大 120000 tokens
- **短期记忆**：TTL 24 小时，基于相似度检索（阈值 0.7）
- **记忆衰减**：每日衰减因子 0.95，半衰期 7 天
- **矛盾检测**：自动检测并处理矛盾信息
- **嵌入缓存**：10000 条嵌入向量缓存

### 缓存系统

| 缓存层级 | 容量 | 用途 |
|----------|------|------|
| 热缓存 | 100 | 高频访问数据 |
| 温缓存 | 500 | 中频访问数据 |
| 共享缓存 | 2000 | 跨会话共享数据 |

### 评分系统（Critic）

- 最多 1 轮优化
- 自动评估回复质量
- 必要时触发重新生成

---

## 配置说明

### config.json 主要配置项

```json
{
  "version": "1.3.0",
  
  "providers": {
    "anthropic": {
      "base_url": "${ANTHROPIC_BASE_URL}",
      "api_key": "${ANTHROPIC_API_KEY}",
      "default_model": "${DEFAULT_MODEL}",
      "timeout_ms": 300000,
      "max_retries": 3
    }
  },
  
  "main_agent": {
    "thinking": {
      "enabled": true,
      "budget_tokens": 16384
    },
    "critic": {
      "enabled": true,
      "max_refinement_rounds": 1
    },
    "config": {
      "max_tokens": 128000,
      "temperature": 0.7
    }
  },
  
  "runtime": {
    "port": 3000,
    "host": "0.0.0.0",
    "cors_enabled": true,
    "max_concurrent_requests": 100
  }
}
```

### 功能开关

```json
{
  "features": {
    "streaming": true,
    "thinking": true,
    "critic": true,
    "caching": true,
    "hot_reload": true
  }
}
```

---

## 构建与发布

### 一键构建

```bash
# Windows
build.bat
```

构建流程：

1. **构建前端**：`npm run build` → 生成 `frontend/dist/`
2. **构建后端**：`cargo build --release` → 生成 `target/release/agent-server.exe`
3. **打包发布**：复制所有文件到 `release/` 目录
4. **构建嵌入式 Python**：下载 Python 3.11.9 嵌入式版本，安装工具依赖

### 发布目录结构

```
release/
├── agent-server.exe    # 主程序（前端+后端）
├── config.json         # 系统配置
├── .env                # API 密钥
├── start.bat           # 启动脚本
└── tools/              # 工具目录
    ├── DocFlow/        # 文档转换工具
    │   ├── python/     # 嵌入式 Python
    │   ├── server.py   # Flask 服务
    │   └── ...
    ├── DocReader/      # 文档阅读工具
    ├── xxt/            # 学习通工具
    │   └── python/     # 嵌入式 Python
    └── screenshots/    # 截图工具
```

### 运行

双击 `release/start.bat` 或直接运行 `agent-server.exe`。

服务启动后访问 `http://localhost:3000`。

---

## 项目结构

```
MS Agent/
├── frontend/                  # React 前端
│   ├── src/
│   │   ├── components/        # UI 组件
│   │   │   ├── settings/      # 设置面板子组件
│   │   │   ├── music/         # 音乐播放器子组件
│   │   │   ├── tools/         # 工具选择器子组件
│   │   │   └── presets/       # 预设选择器子组件
│   │   ├── hooks/             # 自定义 Hooks
│   │   ├── api/               # API 客户端和类型定义
│   │   ├── utils/             # 工具函数
│   │   ├── App.tsx            # 主应用组件
│   │   └── main.tsx           # 入口文件
│   ├── package.json
│   └── vite.config.ts
│
├── main-sub-agent-system/     # Rust 后端
│   ├── core/                  # 核心类型和 trait
│   ├── provider/              # LLM 提供商集成
│   ├── agents/                # 智能体和工具实现
│   ├── coordinator/           # 协调器和管线
│   ├── storage/               # 持久化层
│   ├── runtime/               # HTTP 服务
│   ├── tools/                 # 外部工具脚本
│   │   ├── DocFlow/           # 文档转换
│   │   ├── DocReader/         # 文档阅读
│   │   └── xxt/               # 学习通
│   ├── config.json            # 系统配置
│   └── Cargo.toml             # Rust 工作空间
│
├── tools_build/               # 工具构建脚本
├── release/                   # 发布目录
├── build.bat                  # 一键构建脚本
└── README.md                  # 本文档
```

---

## 许可证

本项目为私有项目，未经授权禁止传播或商业化使用。

# MS Agent Frontend

一个现代化的 AI 助手聊天界面，基于 React 19 + TypeScript 7 + Vite 8 构建。支持流式对话、多媒体背景、音乐播放、Liquid Glass 视觉效果等丰富功能。

## 目录

- [功能特性](#功能特性)
- [技术栈](#技术栈)
- [项目结构](#项目结构)
- [快速开始](#快速开始)
- [环境变量](#环境变量)
- [核心功能详解](#核心功能详解)
  - [聊天系统](#聊天系统)
  - [会话管理](#会话管理)
  - [背景系统](#背景系统)
  - [音乐播放器](#音乐播放器)
  - [媒体库](#媒体库)
  - [Liquid Glass 效果](#liquid-glass-效果)
  - [预设系统](#预设系统)
  - [设置面板](#设置面板)
  - [伴侣面板](#伴侣面板)
- [组件架构](#组件架构)
- [Hooks 详解](#hooks-详解)
- [API 集成](#api-集成)
- [构建与部署](#构建与部署)
- [常见问题](#常见问题)
- [更新日志](#更新日志)
- [贡献指南](#贡献指南)
- [许可证](#许可证)

---

## 功能特性

### 核心聊天功能
- **流式响应**：实时显示 AI 回复，支持打字机效果
- **消息重发**：支持重新发送失败的消息
- **工具调用可视化**：实时显示工具执行状态和进度
- **Markdown 渲染**：完整支持 Markdown 语法高亮
- **LaTeX 数学公式**：通过 KaTeX 渲染数学公式
- **Mermaid 图表**：支持流程图、时序图、甘特图等

### 视觉与交互
- **Liquid Glass 效果**：iOS 风格的毛玻璃视觉效果
- **动态背景**：支持图片和视频作为背景
- **背景过渡动画**：平滑的背景切换效果
- **自定义气泡样式**：可配置消息气泡颜色和透明度
- **自动文字颜色**：根据视频背景自动调整文字颜色

### 多媒体功能
- **音乐播放器**：完整的播放控制、播放列表、歌词显示
- **媒体库管理**：统一管理图片、视频、音乐资源
- **音频可视化**：音乐频谱分析和显示

### 工具与扩展
- **预设系统**：内置和自定义 AI 预设配置
- **伴侣模式**：AI 伴侣互动面板
- **媒体工具**：支持通过工具控制背景、音乐等

---

## 技术栈

| 技术 | 版本 | 说明 |
|------|------|------|
| React | 19.x | 用户界面框架 |
| TypeScript | 7.x | 类型安全的 JavaScript |
| Vite | 8.x | 构建工具和开发服务器 |
| KaTeX | 0.17.x | 数学公式渲染 |
| Mermaid | 11.x | 图表可视化 |
| DOMPurify | 3.x | HTML 内容安全过滤 |
| ESLint | 10.x | 代码规范检查 |

---

## 项目结构

```
frontend/
├── public/                    # 静态资源
│   └── icons.svg             # 图标文件
├── src/
│   ├── api/                  # API 相关
│   │   ├── client.ts         # API 客户端
│   │   └── types.ts          # 类型定义
│   ├── components/           # React 组件
│   │   ├── settings/         # 设置面板子组件
│   │   ├── music/            # 音乐播放器子组件
│   │   ├── tools/            # 工具输入组件
│   │   ├── presets/          # 预设管理组件
│   │   ├── MessageBubble.tsx # 消息气泡
│   │   ├── InputBar.tsx      # 输入栏
│   │   ├── SessionList.tsx   # 会话列表
│   │   ├── WelcomeScreen.tsx # 欢迎页面
│   │   ├── SettingsPanel.tsx # 设置面板
│   │   ├── MusicPlayer.tsx   # 音乐播放器
│   │   ├── CompanionPanel.tsx # 伴侣面板
│   │   └── ...               # 其他组件
│   ├── hooks/                # 自定义 Hooks
│   │   ├── useChat.ts        # 聊天逻辑
│   │   ├── useSession.ts     # 会话管理
│   │   ├── useBackground.ts  # 背景管理
│   │   ├── useMusic.ts       # 音乐控制
│   │   ├── useMediaLibrary.ts # 媒体库
│   │   ├── usePreset.ts      # 预设管理
│   │   └── ...               # 其他 Hooks
│   ├── utils/                # 工具函数
│   │   ├── renderContent.ts  # 内容渲染
│   │   ├── clipboard.ts      # 剪贴板操作
│   │   └── ...               # 其他工具
│   ├── App.tsx               # 主应用组件
│   ├── App.css               # 全局样式
│   ├── main.tsx              # 入口文件
│   └── config.ts             # 配置文件
├── plugins/                  # 插件目录
│   └── siyuan-plugin-text-process/ # 思源笔记文本处理插件
├── package.json              # 项目配置
├── vite.config.ts            # Vite 配置
├── tsconfig.json             # TypeScript 配置
└── eslint.config.js          # ESLint 配置
```

---

## 快速开始

### 环境要求

- Node.js >= 18.x
- npm 或 yarn

### 安装依赖

```bash
# 使用 npm
npm install

# 或使用 yarn
yarn install
```

### 启动开发服务器

```bash
npm run dev
```

开发服务器将在 `http://localhost:5173` 启动，后端 API 代理到 `http://127.0.0.1:3000`。

### 构建生产版本

```bash
npm run build
```

构建产物将输出到 `dist/` 目录。

### 预览生产版本

```bash
npm run preview
```

---

## 环境变量

| 变量名 | 说明 | 默认值 |
|--------|------|--------|
| `VITE_API_URL` | 后端 API 地址 | `''` (空字符串，使用相对路径) |

在项目根目录创建 `.env` 文件进行配置：

```env
VITE_API_URL=http://localhost:3000
```

---

## 核心功能详解

### 聊天系统

聊天系统是应用的核心，支持以下特性：

- **流式响应**：AI 回复以打字机效果实时显示
- **消息状态**：显示消息加载、成功、失败状态
- **错误处理**：网络错误和 API 错误的优雅处理
- **工具事件**：实时显示工具调用状态和结果
- **代理进度**：显示 AI 代理的执行进度

消息类型支持：
- 文本消息
- Markdown 格式
- LaTeX 数学公式
- Mermaid 图表
- 代码块（语法高亮）
- 工具调用结果

### 会话管理

- **创建会话**：自动或手动创建新会话
- **保存会话**：消息自动保存到本地存储
- **切换会话**：在多个会话间快速切换
- **删除会话**：支持单个或批量删除
- **会话预设**：每个会话可绑定独立的预设配置

### 背景系统

支持多种背景类型：

1. **默认背景**：内置的动态装饰效果（光晕、圆环、火花等）
2. **图片背景**：支持自定义图片，可调节透明度和模糊度
3. **视频背景**：支持视频循环播放，可静音控制

背景特性：
- **过渡动画**：背景切换时的平滑过渡效果
- **模糊控制**：可调节背景模糊程度
- **透明度调节**：可调节背景显示强度
- **自动播放**：视频背景支持自动播放

### 音乐播放器

完整的音乐播放解决方案：

- **播放控制**：播放/暂停、上一曲/下一曲
- **音量调节**：支持音量滑块控制
- **静音切换**：一键静音/取消静音
- **播放列表**：管理多个音乐文件
- **歌词显示**：支持 LRC 格式歌词同步显示
- **音频可视化**：音乐频谱分析和波形显示
- **媒体库集成**：从媒体库导入音乐文件

### 媒体库

统一的媒体资源管理：

- **图片管理**：导入、预览、删除图片
- **视频管理**：导入、预览、删除视频
- **音乐管理**：导入、播放、删除音乐
- **文件夹管理**：支持按文件夹组织媒体
- **冲突处理**：导入时的文件冲突解决
- **IndexedDB 存储**：本地持久化存储

### Liquid Glass 效果

iOS 风格的视觉效果：

- **遮罩效果**：顶部和输入栏的毛玻璃效果
- **卡片效果**：消息卡片的玻璃质感
- **按钮效果**：交互按钮的玻璃效果
- **伴层面板**：伴侣面板的玻璃效果

可配置参数：
- `displacementScale`：位移缩放
- `blurAmount`：模糊程度
- `saturation`：饱和度
- `aberrationIntensity`：色差强度
- `elasticity`：弹性
- `cornerRadius`：圆角半径

### 预设系统

AI 行为配置管理：

- **内置预设**：预设的 AI 配置
- **自定义预设**：用户自定义配置
- **会话绑定**：每个会话可绑定独立预设
- **系统指令**：配置 AI 的系统提示词

### 设置面板

全面的应用配置：

- **背景设置**：背景图片/视频、透明度、模糊度
- **气泡颜色**：自定义消息气泡样式
- **Liquid Glass**：视觉效果参数调节
- **伴侣设置**：伴侣模式和情感面板配置
- **界面设置**：隐藏玻璃效果、隐藏欢迎提示等

### 伴侣面板

AI 伴侣互动功能：

- **情感状态**：显示 AI 的情感状态
- **互动反馈**：根据对话内容更新状态
- **可视化展示**：动态的表情和动画

---

## 组件架构

### 核心组件

| 组件 | 说明 |
|------|------|
| `App.tsx` | 主应用组件，管理全局状态和布局 |
| `MessageBubble.tsx` | 消息气泡，支持用户和 AI 消息 |
| `InputBar.tsx` | 输入栏，支持多行输入和工具状态 |
| `StatusBar.tsx` | 顶部状态栏，包含导航和控制按钮 |
| `SessionList.tsx` | 会话列表，支持选择和管理 |
| `SettingsPanel.tsx` | 设置面板，包含所有配置选项 |
| `WelcomeScreen.tsx` | 欢迎页面，显示建议和预设 |
| `MusicPlayer.tsx` | 音乐播放器，完整的播放控制 |
| `CompanionPanel.tsx` | 伴侣面板，AI 互动界面 |

### 辅助组件

| 组件 | 说明 |
|------|------|
| `SplashScreen.tsx` | 启动画面 |
| `BgTransition.tsx` | 背景过渡动画 |
| `ChangelogPanel.tsx` | 更新日志面板 |
| `ImportDialog.tsx` | 导入对话框 |
| `LgGlass*.tsx` | Liquid Glass 系列组件 |
| `MermaidDiagram.tsx` | Mermaid 图表渲染 |
| `HtmlPreview.tsx` | HTML 内容预览 |

---

## Hooks 详解

### 核心 Hooks

| Hook | 说明 |
|------|------|
| `useChat` | 聊天核心逻辑，管理消息流和 API 交互 |
| `useSession` | 会话管理，CRUD 操作和状态持久化 |
| `useSettings` | 应用设置，配置读写和状态管理 |
| `usePreset` | 预设管理，内置和自定义预设 |

### 媒体 Hooks

| Hook | 说明 |
|------|------|
| `useBackground` | 背景管理，图片/视频/透明度/模糊度 |
| `useMusic` | 音乐控制，播放/暂停/音量/静音 |
| `useMediaLibrary` | 媒体库管理，IndexedDB 存储和操作 |
| `usePlaylist` | 播放列表，曲目管理和播放控制 |
| `useVideoBackground` | 视频背景，播放控制和自动播放解锁 |
| `useVideoDominantColor` | 视频主色提取，用于自动文字颜色 |
| `useAudioAnalyser` | 音频分析，频谱数据提取 |
| `useLyrics` | 歌词管理，LRC 解析和同步 |

---

## API 集成

### 后端接口

应用通过代理连接到后端服务（默认 `http://127.0.0.1:3000`）：

| 端点 | 说明 |
|------|------|
| `POST /chat` | 发送聊天消息 |
| `GET /health` | 健康检查 |
| `GET /tools` | 获取可用工具列表 |
| `GET /presets` | 获取预设列表 |

### 数据流

1. 用户输入消息
2. 前端发送到 `/chat` 端点
3. 后端流式返回响应
4. 前端实时渲染消息
5. 工具调用结果通过 WebSocket 或轮询获取

---

## 构建与部署

### 开发模式

```bash
npm run dev
```

### 生产构建

```bash
npm run build
```

### 构建优化

Vite 配置已优化：
- **代码分割**：KaTeX 和 Mermaid 单独分包
- ** chunk 大小警告**：限制为 600KB
- **路径别名**：配置了 Liquid Glass 相关别名

### 部署

构建产物在 `dist/` 目录，可部署到任何静态文件服务器：

```bash
# 使用 nginx
cp -r dist/* /usr/share/nginx/html/

# 使用 serve
npx serve dist
```

---

## 常见问题

### Q: 开发服务器启动后无法访问后端 API？

A: 确保后端服务在 `http://127.0.0.1:3000` 运行。Vite 已配置代理，会自动转发 `/chat`、`/health`、`/tools`、`/presets` 请求。

### Q: 视频背景不自动播放？

A: 浏览器策略限制自动播放。点击播放按钮或与页面交互后即可启用自动播放。

### Q: 音乐播放器不显示？

A: 点击状态栏的音乐图标即可展开音乐播放器面板。

### Q: Liquid Glass 效果不生效？

A: 确保在设置面板中启用了 Liquid Glass 效果，并配置了相应的参数。

### Q: 如何自定义 AI 预设？

A: 在状态栏点击预设选择器，选择"管理预设"即可创建、编辑或删除自定义预设。

---

## 更新日志

详细的更新日志请查看 [ChangelogPanel](src/components/ChangelogPanel.tsx) 或在应用内点击更新日志按钮查看。

---

## 贡献指南

欢迎贡献代码！请遵循以下步骤：

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建 Pull Request

### 开发规范

- 使用 TypeScript 进行类型安全开发
- 遵循 ESLint 代码规范
- 组件使用函数式组件和 Hooks
- 保持组件职责单一
- 编写清晰的类型定义

---

## 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件。

---

## 致谢

- [React](https://react.dev/) - 用户界面框架
- [Vite](https://vitejs.dev/) - 现代构建工具
- [KaTeX](https://katex.org/) - 数学公式渲染
- [Mermaid](https://mermaid.js.org/) - 图表可视化
- [Liquid Glass](https://github.com/nickcen/liquid-glass-react) - iOS 风格视觉效果

---

**作者**: MS Agent  
**仓库**: [GitHub](https://github.com/your-username/ms-agent-frontend)

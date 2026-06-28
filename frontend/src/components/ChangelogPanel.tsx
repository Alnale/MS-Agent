import { useState, useCallback, useMemo } from 'react';

interface Props {
  onClose: () => void;
}

interface ChangelogEntry {
  version: string;
  date: string;
  latest?: boolean;
  items: { category: string; text: string }[];
}

const changelog: ChangelogEntry[] = [
  {
    version: 'V1.5',
    date: '2026-06-24',
    latest: true,
    items: [
      { category: '新功能', text: 'ASR 歌词自动生成：接入 MiMo-V2.5-ASR 语音识别 API，自动将音乐文件转写为带时间戳的 LRC 歌词，支持智能分词（中英文）、前奏检测、段落优化、重试机制，歌词自动保存到 IndexedDB' },
      { category: '新功能', text: '音乐歌词实时同步显示：新增 useLyrics hook，解析 LRC 格式歌词并在播放时实时高亮当前行，显示下一行预览、逐行进度条、当前行播放进度' },
      { category: '新功能', text: '歌词时间偏移调节：播放器展开面板新增 +/-0.5s 步进的歌词偏移控制按钮，支持重置，每首曲目的偏移值独立持久化到 localStorage' },
      { category: '新功能', text: 'LRC 歌词文件导入：音乐播放器支持导入 .lrc 和 .txt 格式的歌词文件，导入后自动保存到 IndexedDB 并绑定到当前曲目' },
      { category: '新功能', text: 'Liquid Glass 毛玻璃效果系统：全新视觉效果引擎，支持三种独立分类（遮罩层/信息卡片/按钮），每种可独立开关和配置，提供四种折射模式（标准/极坐标/突出/着色器），支持折射强度、模糊量、饱和度、色散等 7 个参数调节，基于 SVG feDisplacementMap 实现' },
      { category: '新功能', text: 'Liquid Glass 鼠标交互：LgGlassInteractive 组件为按钮等元素添加鼠标跟踪弹性位移和方向缩放效果，悬停时元素跟随鼠标微动，离开时弹性回弹' },
      { category: '新功能', text: 'Liquid Glass 参数说明卡片：设置面板新增参数说明信息面板，详细解释每个参数的含义、取值范围和典型值' },
      { category: '新功能', text: 'HTML 代码实时预览：代码块中识别 html/htm 语言标记，新增「预览」按钮，点击弹出仿 macOS 窗口样式的 iframe 预览面板，支持刷新和在新窗口打开' },
      { category: '新功能', text: 'MermaidDiagram 懒加载：Mermaid 图表组件改为 React.lazy() 懒加载，避免在无图表消息中加载 mermaid 库' },
      { category: '新功能', text: '预设人格搜索功能：人格选择面板当预设数量 >6 时自动显示搜索框，支持按名称和描述过滤' },
      { category: '新功能', text: '预设人格键盘导航：人格选择列表支持上下箭头键导航和 Enter 键选中' },
      { category: '新功能', text: '复制预设人格：自定义人格列表项新增复制按钮，一键克隆为「(副本)」' },
      { category: '新功能', text: '人格锁定机制：会话有消息后自动锁定当前人格选择，防止误操作切换，状态栏显示锁定图标' },
      { category: '新功能', text: '会话-人格绑定：每个会话独立绑定人格预设，切换会话时自动同步对应的人格' },
      { category: '新功能', text: '设置面板两栏布局：设置面板重构为左右两栏布局，左侧为界面/气泡颜色/高级选项，右侧为背景设置' },
      { category: '新功能', text: '快速配色主题：气泡颜色设置新增 8 套一键配色方案（樱花/薄荷/日落/深海/薰衣草/极光/白瓷/墨染），一键切换整套气泡颜色和文字颜色' },
      { category: '新功能', text: '展开面板音乐素材库：音乐播放器展开面板新增「素材库」按钮，弹出内嵌的音乐素材库面板，支持从库中选择曲目添加到播放列表' },
      { category: '新功能', text: '会话搜索功能：会话列表新增搜索框（>2 条记录时显示），支持按标题和消息内容搜索' },
      { category: '新功能', text: '会话批量管理模式：会话列表新增「管理」按钮进入批量选择模式，支持全选/反选/批量删除' },
      { category: '新功能', text: '智能日期格式化：会话列表日期显示优化——今天显示时间，昨天显示"昨天"，更早显示月日' },
      { category: '新功能', text: '媒体库文件夹分组：媒体库按文件夹分组显示，每个文件夹可折叠/展开，折叠状态持久化到 localStorage' },
      { category: '新功能', text: '媒体库拖放导入：媒体库面板支持拖放文件到面板区域进行导入' },
      { category: '新功能', text: '音乐条目行紧凑布局：音乐类型媒体库改为紧凑的列表行布局（而非卡片网格），显示曲目名和文件大小' },
      { category: '新功能', text: '背景设置素材库切换：背景设置支持「上传」/「素材库」两种来源切换，带滑动指示器动画，素材库面板始终保持挂载用于预加载' },
      { category: '新功能', text: '欢迎页棱镜光效装饰：欢迎页新增多层玻璃折射层、棱镜光束、折射环等视觉装饰' },
      { category: '新功能', text: 'Splash Screen SVG 笔画动画：启动动画重构为 SVG 文本笔画绘制效果（stroke-dasharray 动画），使用 Caveat 字体，等待字体加载后再测量文字长度' },
      { category: '新功能', text: '流式计时器：助手消息流式输出时显示实时计时器，格式化为 X.Xs 或 M:SS' },
      { category: '新功能', text: '问题定位按钮：助手消息底部新增「提问」按钮，点击可平滑滚动到对应的用户消息并高亮，通过预计算 questionIdMap（O(n)）实现' },
      { category: '新功能', text: '剪贴板智能提示增强：输入框聚焦时读取剪贴板内容，空输入时显示剪贴板预览提示，点击提示或双击 Enter 快速粘贴发送，带发送脉冲动画' },
      { category: '新功能', text: '媒体工具新增命令：activate_bg_video/activate_bg_image（激活已存储的背景）、clear_bg（清除背景）、get_status（获取状态）' },
      { category: '新功能', text: 'HTTP 参考来源展示：当助手调用 http_request 工具（网页搜索/抓取）时，消息气泡底部自动展示「参考来源」折叠面板，列出所有访问过的网页链接，点击可在新窗口打开' },
      { category: '优化', text: '搜索质量优化：http_request 搜索默认结果数从 5 提升至 10，新增关键词相关性评分（URL 匹配 +2、链接文本匹配 +3），优先抓取与查询最相关的结果页' },
      { category: '优化', text: '搜索结果过滤增强：新增反爬虫引擎检测（自动跳过返回验证码/Cloudflare 的引擎），排除金融/股票类网站（雪球、东方财富、新浪财经等），URL 去重规范化' },
      { category: '修复', text: '修复工具事件丢失问题：AgentToolLoop 现在正确发射 Executing/Completed 事件到 SSE 流，前端可实时获取 http_request 等工具的执行状态和结果' },
      { category: '修复', text: '修复工具输出过大导致 SSE 事件丢失：截断 Completed 事件中的大型字段（crawled_content、text 等），避免 100KB+ 的事件阻塞流' },
      { category: '优化', text: 'UI 渲染节流：流式消息更新采用 33ms 节流（~30fps），批量合并 SSE chunk 后统一 flush，减少 React 重渲染次数' },
      { category: '优化', text: '会话保存防抖：useSession 的 localStorage 写入增加 500ms 防抖，避免流式消息频繁触发写入' },
      { category: '优化', text: '智能自动滚动：用户手动上滚后停止自动滚动，用户滚回底部附近（<150px）时恢复' },
      { category: '优化', text: '会话 ID 变更精确检测：useChat 的会话切换 effect 仅在 session ID 变化时触发，避免级联更新' },
      { category: '优化', text: '全屏隐藏玻璃层优化：hide-glass 模式下 status-bar 和 input-bar 收缩 padding 并改为 absolute 定位，全屏时不再有顶部和底部空隙' },
      { category: '优化', text: '自动取消静音：浏览器自动播放策略下，首次用户交互自动取消视频静音（仅一次），跳过静音按钮本身的点击以避免冲突' },
      { category: '优化', text: '背景切换过渡动画：切换背景时使用 BgTransition 组件实现两段式过渡动画（色彩洗刷 + 扩展环 + SVG 滴墨 + 微粒）' },
      { category: '优化', text: '音视频频率数据合并：音乐频谱和视频频谱统一合并为 mergedFreq，InputBar 频谱可视化同时响应两种音源' },
      { category: '优化', text: 'Media Library 增量刷新：文件夹导入时仅刷新受影响的媒体类型（touchedTypes），而非全量刷新三种类型' },
      { category: '优化', text: 'importFilesByType 按类型导入：新增按指定类型过滤导入的函数，音乐播放器素材库导入不再混入图片/视频' },
      { category: '优化', text: 'MediaItem 去扩展名存储：存入 IndexedDB 时自动去除文件扩展名，便于匹配和显示' },
      { category: '优化', text: '视频缩略图延迟加载：视频素材库卡片的 Blob 在悬浮时才加载，避免初始渲染时加载所有视频 Blob 阻塞 UI' },
      { category: '优化', text: 'Mermaid 自定义主题：Mermaid 图表使用应用主题配色（樱花粉主色、薰衣草线条色），初始化仅一次' },
      { category: '优化', text: 'ESC 快捷键关闭层级：ESC 按键按优先级关闭——音乐面板 > 更新日志 > 设置面板 > 侧边栏' },
      { category: '优化', text: '侧边栏触摸手势：侧边栏支持左滑关闭（移动超过 80px 且主要水平方向）' },
      { category: '优化', text: '音乐播放器状态持久化：音乐钉住状态持久化到 localStorage，刷新后保持' },
      { category: '优化', text: '音乐入口按钮脉冲动画：状态栏音乐按钮在播放中显示脉冲动画' },
      { category: '优化', text: '音乐频谱可视化增强：InputBar 频谱改为棱镜色带（Prismatic Ribbon）填充形状，空闲时三层叠加正弦波 + 呼吸脉冲 + 中心强调' },
      { category: '修复', text: '歌词 ASR 转写后自动刷新：音乐播放器监听 music-lyrics-ready 自定义事件，ASR 转写完成后自动重新加载歌词' },
      { category: '修复', text: '歌词前奏时间校准：ASR 转写的歌词自动检测前奏延迟（>2s），调整所有时间戳减去前奏偏移' },
      { category: '修复', text: 'ImportDialog 空值安全：ImportDialog 组件 folders 属性未定义时显示空列表而非崩溃' },
      { category: '修复', text: 'useMediaLibrary resolver 重渲染修复：冲突解决器和子文件夹解决器使用 ref 注入，避免 effect 依赖数组中的 resolver 引用变化导致无限循环' },
      { category: '修复', text: '视频素材库遮挡修复：视频元素添加 pointer-events: none，删除按钮添加 z-index: 10，修复悬浮预览和操作按钮被遮挡' },
      { category: '修复', text: '工具栏状态动画重置：新对话轮次开始时自动清除上一轮的工具状态动画' },
      { category: '修复', text: '视频播放状态重置：背景视频被移除时自动将 videoPlaying 设为 false，防止残留播放状态' },
      { category: '修复', text: '视频静音状态持久化：视频静音状态持久化到 localStorage' },
      { category: '修复', text: '设置持久化完善：hideGlass、hideWelcomePrompt、useSolidBubble、bubbleTextColor、userBubbleColor/Alpha、assistantBubbleColor/Alpha、autoTextEnabled 等设置均持久化到 localStorage' },
    ],
  },
  {
    version: 'V1.4',
    date: '2026-06-19',
    items: [
      { category: '新功能', text: 'media 媒体控制工具：LLM 可通过自然语言控制图片背景、视频背景、音乐播放，支持 14 种操作（导入并设置背景、播放/暂停/切歌、音量调节等），前端自动拦截工具结果并执行' },
      { category: '新功能', text: 'media 工具关键词检测：在 TOOL_KEYWORD_REGISTRY 注册媒体相关关键词（背景、壁纸、播放音乐、下一首等），协调器自动识别媒体控制意图并路由到 task_planner' },
      { category: '新功能', text: '视频素材库悬浮预览：鼠标不悬浮时静止显示视频首帧（poster），悬浮时自动从头播放预览，离开时暂停并回到首帧，采用 document.mousemove + getBoundingClientRect 精确检测' },
      { category: '新功能', text: '音乐播放器完整重构：胶囊视图 + 展开面板双模式，固定定位（右下角），悬浮展开/收起，支持钉住展开面板常驻显示' },
      { category: '新功能', text: '音乐播放列表功能：展开面板显示完整播放列表，支持从素材库选择音乐添加、点击切歌、删除曲目，列表可滚动（最大 240px）' },
      { category: '新功能', text: '导入冲突解决对话框：导入文件时自动检测重名文件，弹出对话框选择跳过/覆盖/取消，支持「后续都应用此选择」记忆选项' },
      { category: '新功能', text: '子文件夹遍历对话框：导入文件夹时检测子文件夹，弹出对话框选择是否遍历子文件夹，支持记忆选项' },
      { category: '新功能', text: '媒体库批量删除：操作栏新增「清空」按钮（一键删除当前类型所有文件），文件夹标题新增删除按钮（悬浮显示，删除该文件夹下所有文件）' },
      { category: '新功能', text: '状态栏统一静音控制：静音按钮同时控制视频背景和音乐的静音/取消静音，任一音源未静音时显示未静音图标' },
      { category: '新功能', text: '展开面板上一首/下一首按钮：播放控制栏新增上一首和下一首切换按钮，布局为 静音 | 上一首 | 播放 | 下一首 | 移除' },
      { category: '优化', text: '音乐播放器动画优化：胶囊和展开面板均使用 CSS transition 控制显隐（opacity + transform），展开面板 180ms、胶囊 150ms + 80ms 延迟，视觉连贯无断裂' },
      { category: '优化', text: '音乐播放器展开面板加大：宽度从 268px 增至 340px，padding 和 gap 增大，canvas 波形从 228px 扩到 300px，素材库选择更方便' },
      { category: '优化', text: '视频背景持久化修复：从素材库选择视频作为背景时，保存实际 Blob 到 IndexedDB 而非临时对象 URL，刷新后不再丢失' },
      { category: '优化', text: '全屏隐藏玻璃层优化：hide-glass 模式下 status-bar 和 input-bar 收缩 padding 并改为 absolute 定位，全屏时不再有顶部和底部空隙' },
      { category: '优化', text: 'useMediaLibrary 重构：冲突解决器和子文件夹解决器作为参数传入（非内部状态），内部用 ref 存储避免重渲染，消除无限循环' },
      { category: '修复', text: '修复视频素材库悬浮时浏览器原生控件/PiP 按钮遮挡预览，视频元素添加 pointer-events: none' },
      { category: '修复', text: '修复视频素材库删除按钮被视频元素遮挡无法点击，删除按钮添加 z-index: 10' },
      { category: '修复', text: '修复音乐播放器拖拽不跟手问题，拖拽 effect 依赖从 [dragging, pos] 改为 [dragging]，位置追踪改用 ref' },
      { category: '修复', text: '修复音乐播放器展开面板播放列表点击选歌后高亮不更新，新增 onSelectTrack 回调同步更新 playlistIndex' },
      { category: '修复', text: '修复 ImportDialog 组件 folders 属性未定义时崩溃，添加空值安全检查' },
      { category: '修复', text: '修复 useMediaLibrary resolver 注入导致无限重渲染，移除 mediaLibrary 从 effect 依赖数组' },
    ],
  },
  {
    version: 'V1.3',
    date: '2026-06-13',
    items: [
      { category: '新功能', text: 'task_planner 统一工具执行：吸收原 tool_agent 全部职责，引入 ReAct 模式工具循环（AgentToolLoop），支持 LLM 迭代推理 + 多步工具调用，系统从 5 个内置 Agent 精简为 4 个' },
      { category: '新功能', text: '四级路由策略：L0 决策缓存 → L1 关键词/工具意图检测（TOOL_KEYWORD_REGISTRY 静态注册表）→ L2 路由表直连 → L3 LLM 分类，简单查询完全跳过 LLM 分类' },
      { category: '新功能', text: 'UnifiedMemoryBus 跨 Agent 缓存协调：MemoryEventBus 事件广播自动触发缓存失效，session 结束时自动清理防泄漏' },
      { category: '新功能', text: '自适应重规划：关键 SubAgent 失败或质量过低时，自动寻找能力相似的替代 Agent 重试，最多 2 个替代方案' },
      { category: '新功能', text: '动态路由决策：task_planner 的 routing_decision effect 决定追加调用哪些 Agent，skip_others=true 跳过闲聊场景的额外调用' },
      { category: '新功能', text: '子 Agent 工具请求处理：检测 SubAgent 输出中的 [[tool:name]] 语法，自动委托 task_planner 执行' },
      { category: '新功能', text: '工具执行引擎升级：断路器（连续失败 5 次熔断 60s）、指数退避重试、Semaphore 并发控制（默认 10）、结果 TTL 缓存' },
      { category: '新功能', text: 'Per-tool 指标监控：调用次数、成功率、avg/p95 延迟、错误率，暴露于 /tools/metrics 端点' },
      { category: '新功能', text: '参数推断器 ParameterInferrer：从对话上下文自动补全工具调用缺失参数' },
      { category: '新功能', text: '脚本工具动态发现：自动扫描 tools/ 目录，将 Python/JS/Shell 脚本注册为工具，支持 description.txt + schema.json 元数据' },
      { category: '新功能', text: 'SSE 心跳机制：每 3 秒发送 thinking_delta 防止前端超时，工具状态事件和 Agent 进度事件实时透传' },
      { category: '新功能', text: '子 Agent 结果摘要去重：同 Agent 多次执行时保留最高质量结果' },
      { category: '新功能', text: 'cost_optimization 配置：skip_thinking_for_simple / skip_critic_for_simple，加权评分算法（长度、词数、社交模式、问号密度、中文语气词）' },
      { category: '新功能', text: 'degradation 降级策略配置，支持 LLM 不可用时的回退行为' },
      { category: '新功能', text: 'unified_cache 统一缓存配置：统一缓存总线开关、跨 session 缓存共享、各级容量和 TTL 可配置' },
      { category: '优化', text: '优化记忆质量控制，最低质量阈值（MIN_QUALITY_THRESHOLD=0.5）过滤低质量 Agent 输出' },
      { category: '优化', text: '优化异步事实提取（extract_and_store_facts），非阻塞主流程' },
      { category: '优化', text: '优化 Prompt Injection 防御，三级风险评估（High 拒绝 / Medium 警告），sanitize_user_input 输入清洗' },
      { category: '优化', text: '优化 API 认证，Bearer Token / X-API-Key 双模式，constant_time_eq 防时序攻击' },
      { category: '优化', text: '优化数组字段验证：system_instructions ≤ 100 条 / 500KB，recent_history ≤ 200 条' },
      { category: '优化', text: '优化 Prometheus 指标端点 /metrics/prometheus，20+ 指标含 per-agent 执行时长和记忆同步延迟' },
      { category: '优化', text: '优化工具健康状态评估：Healthy / Degraded / Unhealthy / Unknown 四级' },
      { category: '修复', text: '修复大型工具输出导致上下文溢出问题，task_planner 自动压缩 >8KB 输出（头尾保留 + 中间省略）' },
    ],
  },
  {
    version: 'V1.2',
    date: '2026-06-11',
    items: [
      { category: '新功能', text: '接入多LLM提供商架构，支持 Anthropic Claude / OpenAI兼容 / Ollama 本地模型，通过 ProviderRegistry 统一管理' },
      { category: '新功能', text: '增加断路器保护（Circuit Breaker），LLM调用连续失败时自动熔断，半开状态探测恢复' },
      { category: '新功能', text: '增加指数退避重试机制，LLM调用失败自动重试，避免瞬时故障影响用户体验' },
      { category: '新功能', text: '增加三级降级策略（L0→L1→L2），根据连续失败次数、错误率、延迟阈值自动降级' },
      { category: '新功能', text: '增加热重载机制，修改 config.json 自动生效（日志级别、超时时间、成本优化参数），无需重启服务' },
      { category: '新功能', text: '接入 OpenTelemetry + Jaeger 分布式追踪，支持全链路请求追踪' },
      { category: '新功能', text: '接入 Prometheus 指标导出（/metrics/prometheus），支持外部监控系统对接' },
      { category: '新功能', text: '增加 API Key 认证中间件，使用常量时间比较防止时序攻击' },
      { category: '新功能', text: '增加 Prompt 注入检测，识别高/中风险注入模式，自动拦截恶意输入' },
      { category: '新功能', text: '增加输入安全层，100KB 大小限制 + 数组字段校验 + 输入消毒' },
      { category: '新功能', text: '接入 pgvector 向量相似度搜索，支持基于语义的记忆检索' },
      { category: '新功能', text: '增加记忆关系系统，支持矛盾、支持、替代、关联四种关系类型' },
      { category: '新功能', text: '增加记忆去重引擎（DedupEngine），基于余弦相似度自动去重' },
      { category: '新功能', text: '增加记忆重排序器（MemoryReranker），检索后按相关性二次排序' },
      { category: '新功能', text: '增加用户画像演进追踪（UserEvolution），记录用户偏好随时间的变化' },
      { category: '新功能', text: '增加跨会话主题追踪（CrossSessionTopic），跨对话关联用户关注的话题' },
      { category: '新功能', text: '增加工具发现机制，支持从 MCP Server 和 OpenAPI 规范动态注册工具' },
      { category: '新功能', text: '增加运行时工具热注册/注销（POST /tools/register, DELETE /tools/{name}），无需重启即可扩展工具' },
      { category: '新功能', text: '增加工具执行指标（/tools/metrics），追踪调用次数、成功率、平均耗时' },
      { category: '新功能', text: '增加工具策略引擎，支持通配符模式、代理/用户白名单、速率限制' },
      { category: '新功能', text: '增加参数推断器（ParameterInferrer），从对话上下文自动推断缺失的工具参数' },
      { category: '新功能', text: '增加上下文提供者系统，MultiTurn / DomainState / Entity / SystemInstruction / Memory 五类上下文自动注入' },
      { category: '新功能', text: '增加生命周期钩子系统，支持 13 个钩子点（PreIntent → PostRun），可扩展自定义处理逻辑' },
      { category: '新功能', text: '增加规则路由表（RoutingTable），基于规则的消息路由分发' },
      { category: '新功能', text: '增加三级统一缓存架构（L1热缓存 / L2温缓存 / L3共享缓存），带缓存指标监控' },
      { category: '新功能', text: '增加压缩评估器（CompressionEvaluator），自动判断何时压缩记忆以节省空间' },
      { category: '新功能', text: '增加自适应重规划，关键 SubAgent 失败时自动尝试替代 Agent' },
      { category: '新功能', text: '增加 Critic 质量审查，合成结果自动评审，发现严重问题触发重新合成' },
      { category: '新功能', text: '增加并发控制，基于信号量的 Agent 并发限制，防止资源过载' },
      { category: '新功能', text: '增加会话级系统指令管理（GET/PUT/DELETE /sessions/{id}/instructions），支持自定义系统提示词' },
      { category: '新功能', text: '增加工具浏览器 UI，支持搜索过滤、参数表单、插入调用和直接执行两种模式' },
      { category: '新功能', text: '增加工具执行状态动画，工具调用成功/失败时按钮状态实时反馈，带辉光效果' },
      { category: '新功能', text: '增加 ThinkingIndicator 思考指示器，流式阶段标签切换、进度步进器、Agent 药丸实时展示' },
      { category: '新功能', text: '增加 SubAgent 结果可视化，时间线展示每个子 Agent 的质量环、思考内容折叠、执行耗时' },
      { category: '新功能', text: '增加响应时间追踪，每条消息显示精确响应耗时（ms/s/m 自适应格式）' },
      { category: '新功能', text: '增加剪贴板智能集成，输入框聚焦时检测剪贴板内容并提示，双 Enter 快速粘贴发送' },
      { category: '新功能', text: '接入 KaTeX 数学公式渲染，支持行内和块级 LaTeX 公式，带复制功能' },
      { category: '新功能', text: '增加会话导出为 Markdown 功能' },
      { category: '新功能', text: '增加全屏背景切换动画，两段式过渡（色彩洗刷 + 扩展环 + SVG 滴墨 + 微粒）' },
      { category: '新功能', text: '增加 ErrorBoundary 全局错误边界，渲染异常时优雅降级，支持重试恢复' },
      { category: '新功能', text: '增加更新日志面板，带多层玻璃质感和棱镜折射效果' },
      { category: '新功能', text: '增加音频可视化频谱分析器，64 条棱镜色彩均衡器，视频播放时实时响应' },
      { category: '优化', text: '优化记忆存储架构，PostgresMemoryStore 支持批量事务操作、向量检索 SQL 下推、内容哈希去重' },
      { category: '优化', text: '优化记忆压缩策略，Summary Agent 支持增量模式和双向记忆同步' },
      { category: '优化', text: '优化统一缓存管理器，响应缓存 + 计划缓存 + 子Agent缓存三层缓存协同工作' },
      { category: '优化', text: '优化 Pipeline 执行架构，8 个阶段钩子点（PreRun → PostRun）全面可观测' },
      { category: '优化', text: '优化 MainAgent 路由策略，四级路由（缓存命中 → 工具意图检测 → 路由表 → LLM 分类）' },
      { category: '优化', text: '优化成本控制，简单查询自动跳过 Thinking 和 Critic 阶段，节省 Token 消耗' },
      { category: '优化', text: '优化 Web Audio API 音频管线，source → analyser → gain → destination 四节点链路' },
      { category: '优化', text: '优化视频主色采样算法，64×48 降采样 + 亮度排序截断 + 高斯中心加权 + WCAG 对比度计算' },
      { category: '优化', text: '优化渲染管线，多阶段 Markdown 解析（代码块 → 块级LaTeX → 表格 → 行内LaTeX → 行内Markdown）' },
      { category: '优化', text: '优化 XSS 防护，链接 URL 净化拦截 javascript:/data: 协议，Mermaid SVG 通过 DOMPurify 消毒' },
      { category: '优化', text: '优化事件委托机制，代码块和表格的复制按钮通过 data 属性事件委托实现，减少 DOM 节点' },
      { category: '修复', text: '修复跨编码网页抓取乱码问题，新增 GBK/GB2312/GB18030/Big5/Shift_JIS/EUC-KR 自动检测解码' },
      { category: '修复', text: '修复大型工具输出导致上下文溢出问题，tool_agent 自动压缩 >8KB 输出' },
    ],
  },
  {
    version: 'V1.1',
    date: '2026-06-08',
    items: [
      { category: '新功能', text: '支持根据自定义视频背景，实时调整气泡字体颜色' },
      { category: '新功能', text: '增加主动陪伴功能，Agent会主动和用户互动，热情随时间递减，最后收敛稳定' },
      { category: '新功能', text: '增加tool Sub Agent，将所有的tool调用功能解耦成一个单独模块，使用子Agent进行辅助判断。tool注册信息存在于所有Agent的上下文中' },
      { category: '新功能', text: '支持Agent工具链的调用' },
      { category: '新功能', text: '支持LLM根据自然语言，自动填入tool所需参数并调用' },
      { category: '新功能', text: '接入Mermaid方案，优化消息气泡对特殊格式，如表格，流程图，代码块等的渲染' },
      { category: '新功能', text: '支持自定义字体颜色，接入RGB选择器' },
      { category: '新功能', text: '增加了深度思考，代码块，表格，流程图的复制功能' },
      { category: '新功能', text: '增加了历史记录批量删除功能' },
      { category: '新功能', text: '增加了半透明消息气泡背景UI' },
      { category: '优化', text: '统一按钮UI，统一滚动条UI' },
      { category: '优化', text: '优化记忆系统和上下文管理，优化压缩上下文的能力' },
      { category: '优化', text: '优化深度思考整合能力，优化Main Agent和Sub Agent的协作能力' },
      { category: '优化', text: '优化了玻璃遮罩层的消失和出现动画，采用棱镜折射两段式' },
      { category: '优化', text: '优化流光装饰条的UI，接入了音频可视化功能' },
      { category: '优化', text: '优化了欢迎页的背景UI，更改了默认选项' },
      { category: '优化', text: '优化了背景切换页的UI' },
      { category: '优化', text: '重新设计了启动动画的UI' },
      { category: '优化', text: '优化了设置面板，图片和视频选项卡的切换动画' },
      { category: '修复', text: '修复视频静音需要点击两次bug' },
    ],
  },
];

const categoryMeta: Record<string, { color: string; icon: string }> = {
  '新功能': { color: '#6366f1', icon: '＋' },
  '优化': { color: '#0ea5e9', icon: '↑' },
  '修复': { color: '#f59e0b', icon: '✓' },
};

const categoryOrder = ['新功能', '优化', '修复'];

export function ChangelogPanel({ onClose }: Props) {
  const [closing, setClosing] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);

  const handleClose = useCallback(() => {
    setClosing(true);
  }, []);

  const handleAnimationEnd = useCallback(() => {
    if (closing) onClose();
  }, [closing, onClose]);

  const groupedItems = useMemo(() => {
    return changelog.map((entry) => ({
      ...entry,
      stats: categoryOrder.map((cat) => ({
        category: cat,
        count: entry.items.filter((item) => item.category === cat).length,
      })),
      groups: categoryOrder
        .filter((cat) => entry.items.some((item) => item.category === cat))
        .map((cat) => ({
          category: cat,
          items: entry.items.filter((item) => item.category === cat),
        })),
    }));
  }, []);

  return (
    <div
      className={`changelog-overlay${closing ? ' closing' : ''}`}
      onClick={handleClose}
      onAnimationEnd={handleAnimationEnd}
    >
      <div className={`changelog-panel${closing ? ' closing' : ''}`} onClick={(e) => e.stopPropagation()}>
        {/* Header */}
        <div className="changelog-header-bar">
          <h3>更新日志</h3>
          <div className="changelog-tabs">
            {groupedItems.map((entry, i) => (
              <button
                key={entry.version}
                className={`changelog-tab${i === activeIndex ? ' active' : ''}`}
                onClick={() => setActiveIndex(i)}
              >
                <span className="changelog-tab-version">{entry.version}</span>
                {entry.latest && <span className="changelog-tab-latest">Latest</span>}
              </button>
            ))}
          </div>
          <button className="changelog-close" onClick={handleClose}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        {/* Card stage */}
        <div className="changelog-stage">
          {groupedItems.map((entry, i) => {
            const isActive = i === activeIndex;
            const offset = i - activeIndex;
            const depth = Math.min(Math.abs(offset), 3);
            const side = offset <= 0 ? 'left' : 'right';

            return (
              <div
                key={entry.version}
                className="changelog-card-wrapper"
                data-depth={depth}
                data-side={side}
                onClick={() => { if (!isActive) setActiveIndex(i); }}
              >
              <div className={`changelog-card${isActive ? ' active' : ''}`}>
                {/* Card glass */}
                <div className={`changelog-card-glass${isActive ? ' active' : ''}`} />
                {isActive && <div className="changelog-card-prism" />}

                {/* Card banner */}
                <div className="changelog-card-banner">
                  {isActive && <div className="changelog-card-banner-decor" />}
                  <div className="changelog-card-banner-content">
                    <span className="changelog-card-version">{entry.version}</span>
                    <span className="changelog-card-date">{entry.date}</span>
                    {entry.latest && <span className="changelog-card-latest">Latest</span>}
                  </div>
                  <div className="changelog-card-stats">
                    {entry.stats.filter((s) => s.count > 0).map((s, si) => (
                      <span key={s.category} className="changelog-card-stat">
                        {si > 0 && <span className="changelog-card-stat-div" />}
                        <span className="changelog-card-stat-num">{s.count}</span>
                        <span className="changelog-card-stat-label">{s.category}</span>
                      </span>
                    ))}
                  </div>
                  {isActive && <div className="changelog-card-rainbow" />}
                </div>

                {/* Card body */}
                <div className="changelog-card-body">
                  {entry.groups.map((group) => {
                    const meta = categoryMeta[group.category] || { color: '#888', icon: '•' };
                    return (
                      <div key={group.category} className="changelog-group">
                        <div className="changelog-group-header">
                          <span
                            className="changelog-group-dot"
                            style={{ background: meta.color, '--dot-c': meta.color } as React.CSSProperties}
                          />
                          <span className="changelog-group-title">{group.category}</span>
                          <span className="changelog-group-count">{group.items.length}</span>
                        </div>
                        <div className="changelog-group-items">
                          {group.items.map((item, idx) => (
                            <div key={idx} className="changelog-item">
                              <span
                                className="changelog-item-bar"
                                style={{ background: meta.color, '--bar-c': meta.color } as React.CSSProperties}
                              />
                              <span className="changelog-item-icon" style={{ color: meta.color } as React.CSSProperties}>
                                {meta.icon}
                              </span>
                              <span className="changelog-text">{item.text}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
              </div>
            );
          })}
        </div>

        {/* Navigation dots */}
        <div className="changelog-nav">
          {groupedItems.map((entry, i) => (
            <button
              key={entry.version}
              className={`changelog-nav-dot${i === activeIndex ? ' active' : ''}`}
              onClick={() => setActiveIndex(i)}
              aria-label={entry.version}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

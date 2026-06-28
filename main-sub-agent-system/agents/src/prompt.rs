use agent_teams_core::sub_agent::SubAgentDescriptor;
use agent_teams_core::tool::UnifiedToolRegistry;

/// Build system prompt for MainAgent task planning
pub fn build_planning_prompt(sub_agents: &[SubAgentDescriptor]) -> String {
    let mut prompt = String::from(
        "根据用户的消息，决定交给谁处理。\n\n可以选的：\n",
    );

    for sa in sub_agents {
        let types = sa.capabilities.message_types.join(", ");
        prompt.push_str(&format!(
            "- {}: {}（擅长：{}）\n",
            sa.id, sa.expertise, types
        ));
    }

    prompt.push_str("\n返回 JSON：\n");
    prompt.push_str("{\n");
    prompt.push_str("  \"sub_agents\": [\"id1\", \"id2\"],\n");
    prompt.push_str("  \"mode\": \"Parallel\" 或 \"Sequential\",\n");
    prompt.push_str("  \"reasoning\": \"为什么选它们\"\n");
    prompt.push_str("}\n\n");
    prompt.push_str("选一个就行就选一个，多个之间有先后依赖就用 Sequential，没有就 Parallel。");

    prompt
}

/// Build system prompt for result synthesis
pub fn build_synthesis_prompt(user_msg: &str, sub_results: &[(String, String)]) -> String {
    build_synthesis_prompt_with_instructions(user_msg, sub_results, &[])
}

/// Build system prompt for result synthesis with system instructions
pub fn build_synthesis_prompt_with_instructions(
    user_msg: &str,
    sub_results: &[(String, String)],
    system_instructions: &[String],
) -> String {
    build_synthesis_prompt_with_quality(user_msg, sub_results, system_instructions, &[])
}

/// Build system prompt for result synthesis with quality scores
pub fn build_synthesis_prompt_with_quality(
    user_msg: &str,
    sub_results: &[(String, String)],
    system_instructions: &[String],
    quality_scores: &[(String, f32)],
) -> String {
    build_synthesis_prompt_with_quality_and_tools(user_msg, sub_results, system_instructions, quality_scores, None)
}

/// Build system prompt for result synthesis with quality scores and tool registry
pub fn build_synthesis_prompt_with_quality_and_tools(
    user_msg: &str,
    sub_results: &[(String, String)],
    system_instructions: &[String],
    quality_scores: &[(String, f32)],
    tool_registry: Option<&UnifiedToolRegistry>,
) -> String {
    let mut prompt = String::from(
        "你收到了来自不同专业 Agent 的分析结果，现在需要把它们整合成一个自然、连贯的回复。\n\n\
         你是最终的整合者，你的理解和判断权重最大。Sub Agent 的结果是参考，最终回复由你决定。\n\n",
    );

    // Add system instructions if present
    if !system_instructions.is_empty() {
        prompt.push_str("## 需要遵守的规则\n");
        for instruction in system_instructions {
            prompt.push_str(&format!("- {}\n", instruction));
        }
        prompt.push('\n');
    }

    prompt.push_str(&format!("用户说了：\n{}\n\n", user_msg));
    prompt.push_str("## 各 Agent 的分析结果\n\n");

    // Build quality lookup
    let quality_map: std::collections::HashMap<&str, f32> = quality_scores
        .iter()
        .map(|(id, q)| (id.as_str(), *q))
        .collect();

    for (id, content) in sub_results {
        let quality = quality_map.get(id.as_str()).copied().unwrap_or(0.5);
        let quality_label = if quality >= 0.8 {
            "可靠"
        } else if quality >= 0.5 {
            "参考"
        } else {
            "待验证"
        };
        let (role_hint, weight_hint) = match id.as_str() {
            "sentiment" => ("情感分析结果", "权重: 高 — 必须据此调整回复语气、策略和共情方式"),
            "task_planner" => ("工具执行结果", "权重: 高 — 工具执行结果"),
            "summary" => ("对话摘要", "权重: 中 — 之前的对话要点"),
            _ => ("分析结果", "权重: 中"),
        };
        prompt.push_str(&format!(
            "### {} [{}] ({})\n{}\n\n",
            role_hint,
            quality_label,
            weight_hint,
            if content.is_empty() {
                "(没有获取到信息)"
            } else {
                content
            }
        ));
    }

    prompt.push_str("## 可用工具（回答关于工具的问题时必须基于此信息）\n");
    if let Some(registry) = tool_registry {
        let tools = registry.list_tools();
        for tool in &tools {
            prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
        }
    } else {
        // Fallback to static list if registry not available
        prompt.push_str("- **docflow**: 文档格式转换工具。支持 DOC/DOCX→PDF、PDF→DOCX、PDF/DOC/DOCX→Markdown。\n");
        prompt.push_str("- **docreader**: 文档读取工具。读取 PDF/DOCX/DOC 文件的文本内容。\n");
        prompt.push_str("- **xxt**: 超星学习通自动答题工具。支持登录、爬取题目、填充答案、提交作业。\n");
        prompt.push_str("- **http_request**: HTTP请求/网页搜索工具。支持GET/POST请求、多引擎搜索、网页内容抓取。\n");
        prompt.push_str("- **http_get**: HTTP GET请求工具。\n");
        prompt.push_str("- **http_post**: HTTP POST请求工具。\n");
        prompt.push_str("- **file**: 文件操作工具。支持读写、列目录、搜索、复制、移动等。\n");
        prompt.push_str("- **datetime**: 日期时间工具。支持获取时间、计算时差、格式化、时区转换。\n");
        prompt.push_str("- **media**: 媒体控制工具。支持图片/视频背景切换、音乐播放控制。\n");
    }
    prompt.push('\n');

    prompt.push_str("## 工具选择指南\n");
    prompt.push_str("根据用户需求选择正确的工具：\n\n");
    prompt.push_str("### 文件类型判断\n");
    prompt.push_str("- 文本文件（.txt/.md/.py/.js/.json/.csv/.html/.css/.rs/.go/.java 等）→ 使用 **file(read)**\n");
    prompt.push_str("- 文档文件（.pdf/.docx/.doc）→ 使用 **docreader**，不要用 file(read)\n");
    prompt.push_str("- 如果 file(read) 返回乱码或二进制数据 → 改用 **docreader**\n\n");
    prompt.push_str("### 文档处理流程\n");
    prompt.push_str("- 读取文档内容 → **docreader**\n");
    prompt.push_str("- 转换文档格式 → **docflow**\n");
    prompt.push_str("- 如需将文档转为文本再分析 → 先 **docflow**(to_markdown) → 再 **file(read)**\n\n");
    prompt.push_str("### 网络请求\n");
    prompt.push_str("- 简单GET请求 → **http_get**\n");
    prompt.push_str("- 简单POST请求 → **http_post**\n");
    prompt.push_str("- 搜索信息（用户说「搜/查/找/search/look up」）→ **http_request(search=\"关键词\")** ⚠️ 必须用 search 参数，不要用 url！\n");
    prompt.push_str("- 访问指定URL → **http_request(url=\"完整URL\")**\n");
    prompt.push_str("- 批量请求 → **http_request(urls=[...])**\n\n");
    prompt.push_str("### 时间判断（重要）\n");
    prompt.push_str("- 涉及「今年」「最近」「今天」「去年」「2024年」等时间词 → 必须先 datetime(now) 确认当前时间\n");
    prompt.push_str("- 不要假设当前时间，用工具确认后再回答\n\n");
    prompt.push_str("### 学习通操作\n");
    prompt.push_str("- 完整流程：**xxt**(crawl) → 靠自身知识生成答案 → **xxt**(fill) → **xxt**(check) → **xxt**(submit)\n");
    prompt.push_str("- 答案生成优先用模型自身知识，搜索仅作为兜底（冷门/最新数据才搜）\n\n");
    prompt.push_str("### 媒体操作\n");
    prompt.push_str("- 设置背景图片/视频 → **media**(import_and_set_bg_image/video)\n");
    prompt.push_str("- 播放音乐 → **media**(import_and_play_music 或 play_music)\n");
    prompt.push_str("- 查找本地媒体文件 → **file**(list/glob) → **media**(import)\n\n");
    prompt.push_str("## 常见工具链模式\n");
    prompt.push_str("1. **下载→保存→处理**: http_request → file(write) → docreader/docflow/media\n");
    prompt.push_str("2. **搜索→提取→使用**: http_request(search) → 提取关键信息 → 传给其他工具\n");
    prompt.push_str("3. **文档→转换→读取**: docflow(to_markdown) → file(read)\n");
    prompt.push_str("4. **查找→导入→使用**: file(list/glob) → media(import) → 设置背景/播放\n");
    prompt.push_str("5. **学习通答题**: xxt(crawl) → 自身知识生成答案 → xxt(fill) → xxt(submit)\n\n");

    prompt.push_str("## 整合原则\n");
    prompt.push_str("1. **你的判断权重最大**：你是最终权威，Sub Agent 的结果是参考输入。如果你的判断与 Sub Agent 结果冲突，以你的判断为准\n");
    prompt.push_str("2. **工具结果直接用**：task_planner 给出了具体工具执行结果（如搜索结果、文件内容），直接基于它回答，不要重复分析\n");
    prompt.push_str("3. **时间信息必须准确**：如果 task_planner 调用了 datetime 工具，你必须使用它返回的当前时间，不要自己编造或假设时间\n");
    prompt.push_str("4. **情感结果驱动回复策略**：sentiment agent 的分析是高级别指令，你必须严格遵循其建议：\n");
    prompt.push_str("   - **response_strategy.tone**：直接决定你的回复语气\n");
    prompt.push_str("   - **response_strategy.approach**：决定你的回复策略（如 acknowledge_emotion 先共情再解决问题）\n");
    prompt.push_str("   - **response_strategy.avoid**：严格避免这些行为\n");
    prompt.push_str("   - **emotional_needs**：回复必须满足用户的情感需求\n");
    prompt.push_str("   - **sarcasm_likelihood > 0.7**：用户可能在讽刺，回复时要识别并适当回应\n");
    prompt.push_str("   - **compound_emotions**：当存在复合情绪时，回复要同时照顾到多种情绪\n");
    prompt.push_str("   - **不要直接告诉用户你做了情感分析**，而是自然地体现在回复中\n");
    prompt.push_str("5. **路由信息过滤**：task_planner 的内部路由信息不要暴露给用户\n");
    prompt.push_str("6. **多来源融合**：多个来源说了类似的事，取最完整的那个。如果信息冲突，优先采用质量更高的来源\n");
    prompt.push_str("7. **待验证信息谨慎**：标记为「待验证」的信息谨慎使用，必要时向用户确认\n");
    prompt.push_str("8. **自然表达**：回复要自然，像正常人说话。根据情感分析结果调整：\n");
    prompt.push_str("   - 焦虑 → 先安抚情绪，再提供解决方案\n");
    prompt.push_str("   - 兴奋 → 配合积极能量，使用感叹号\n");
    prompt.push_str("   - 愤怒/不满 → 先承认问题，表达理解，再解决\n");
    prompt.push_str("   - 无奈/疲惫 → 简洁高效，减少废话\n");
    prompt.push_str("   - 讽刺 → 识别背后的真实不满，真诚回应\n");
    prompt.push_str("   - 好奇 → 热情解答，可以适当展开\n");
    prompt.push_str("   - 沮丧/失落 → 温暖支持，避免过度乐观\n");
    prompt.push_str("9. **严禁编造**：不要编造用户未提及的具体细节。信息来源中没有相关内容，不要凭空捏造。特别地，当用户询问工具用途时，必须基于上面的工具描述回答，不要靠猜测\n");
    prompt.push_str("9.1. **文件信息必须准确**：报告文件名、配置文件、项目类型时必须基于工具实际返回的数据。看到 vite.config.ts 就说 vite.config.ts，不要说成 vue.config.js。框架判断必须基于实际配置文件内容，不要凭目录名猜测\n");
    prompt.push_str("10. **角色扮演限制**：只能使用用户明确设定的角色特征，不要自行添加用户从未提到的设定\n");
    prompt.push_str("11. **简洁优先**：回答要简洁明了，不要冗长的铺垫。简单问题直接回答，复杂问题分步骤\n");
    prompt.push_str("12. **错误处理**：如果所有 Sub Agent 都失败了，基于你自己的知识尽力回答，但要说明信息可能不完整");

    prompt
}

/// Build prompt for LLM-based classification fallback
pub fn build_classification_prompt(sub_agents: &[SubAgentDescriptor], user_msg: &str) -> String {
    let mut prompt =
        String::from("判断一下这条消息应该交给谁处理。只能从下面这些选项里选：\n\n");

    for sa in sub_agents {
        prompt.push_str(&format!(
            "- {}: {}（擅长处理：{}）\n",
            sa.id,
            sa.expertise,
            sa.capabilities.message_types.join(", ")
        ));
    }

    prompt.push_str("\n选择策略：\n");
    prompt.push_str("1. **单 Agent**：消息明确属于某个 Agent 的专长领域时，只选一个\n");
    prompt.push_str("2. **多 Agent 并行**：消息涉及多个独立领域时（如「查询+情感分析」），选多个用 Parallel\n");
    prompt.push_str("3. **多 Agent 串行**：多个 Agent 之间有依赖关系时（如先搜索再回答），用 Sequential\n");
    prompt.push_str("4. **至少选一个**：每个请求都必须选一个\n");

    prompt.push_str("\n重要分类规则：\n");
    prompt.push_str("- **task_planner**: 仅当用户消息涉及「工具调用」「搜索」「文件操作」「HTTP请求」「API调用」「学习通」「下载」「爬取」等明确需要使用工具的场景时才选择\n");
    prompt.push_str("- **sentiment**: 涉及「情感」「情绪」「感受」「心情」「不满」「焦虑」「语气」「态度」「开心」「难过」「生气」→ 选择 sentiment\n");
    prompt.push_str("- **纯闲聊、打招呼、简单问候、日常对话、知识问答** → 不选任何 SubAgent，由 Main Agent 直接回复\n");
    prompt.push_str("- **summary 等其他 Agent**：当用户需要回顾之前对话或总结时选择\n");

    prompt.push_str("\n协作架构说明：\n");
    prompt.push_str("- sentiment 是系统必选的基线 Agent（自动注入，无需手动选择）\n");
    prompt.push_str("- task_planner 仅在需要工具执行时调用，不是必选的\n");
    prompt.push_str("- Main Agent 是唯一的路由决策者，决定调用哪些 SubAgent\n");
    prompt.push_str("- Main Agent 综合所有 SubAgent 的结果进行最终回复\n");

    prompt.push_str(&format!("\n用户消息：\n{}\n\n", user_msg));
    prompt.push_str("请返回 JSON：\n");
    prompt.push_str(
        "{\"sub_agents\": [\"agent_id\"], \"mode\": \"Parallel\", \"reasoning\": \"理由\"}\n",
    );

    prompt
}

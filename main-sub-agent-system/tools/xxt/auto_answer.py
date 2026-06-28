"""
超星学习通自动答题工具 — 守护进程版本

支持两种运行模式:
  1. CLI 模式: python auto_answer.py <command> --url <URL>  (一次性执行)
  2. 守护进程: python auto_answer.py daemon --port 9222      (保持浏览器常驻，后续调用复用)

子命令:
  login     — 打开浏览器等待登录
  crawl     — 爬取作业页面，提取题目
  fill      — 根据答案填充作业
  submit    — 暂存并提交作业
  screenshot — 对当前页面截图并保存
  check     — 检查页面上的填充状态
  daemon    — 启动守护进程（保持浏览器常驻）

用法:
  python auto_answer.py crawl --url <URL>
  python auto_answer.py fill --url <URL> --answers '{"1":"C","2":"A"}'
  python auto_answer.py daemon --port 9222
"""

import argparse
import asyncio
import io
import json
import logging
import sys
import os
import socket
import threading

# Force UTF-8 output on Windows
if sys.platform == 'win32':
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding='utf-8')

# ============================================================
# 配置
# ============================================================

_USER_DATA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".browser_profile")
_DEFAULT_DAEMON_PORT = 9222
_SCREENSHOT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "screenshots")

# ============================================================
# 浏览器管理（守护进程模式下保持常驻）
# ============================================================

_browser_state = {
    "playwright": None,
    "context": None,
    "page": None,
}


async def ensure_browser(url: str, headless: bool = False):
    """确保浏览器已启动并导航到目标页面"""
    try:
        from playwright.async_api import async_playwright
    except ImportError:
        return _error("playwright 未安装，请运行: pip install playwright && playwright install chromium")

    state = _browser_state

    if state["page"] is None:
        pw = async_playwright()
        try:
            pw_instance = await pw.__aenter__()
        except Exception as e:
            return _error(f"Playwright 启动失败: {e}")

        try:
            context = await pw_instance.chromium.launch_persistent_context(
                _USER_DATA_DIR,
                channel='msedge',
                headless=headless,
                no_viewport=True,
                args=['--start-maximized', '--no-first-run', '--disable-extensions'],
            )
        except Exception as e:
            # 关闭 playwright 实例避免泄漏
            try:
                await pw.__aexit__(None, None, None)
            except Exception:
                pass
            return _error(f"浏览器启动失败: {e}。可能原因：1) 未安装 Edge 2) 配置文件被锁定 3) 浏览器进程残留")

        page = context.pages[0] if context.pages else await context.new_page()
        state["playwright"] = pw_instance
        state["context"] = context
        state["page"] = page

    page = state["page"]

    # 导航到目标页面（如果需要）
    # 跳过无效占位符 URL（如 "current_page"、"current"），直接使用当前页面
    _placeholder_urls = {"current_page", "current", "current_page_url", "页面", "当前页面", ""}
    is_placeholder = url in _placeholder_urls or not url.startswith("http")
    if is_placeholder:
        print(f"[xxt] URL '{url}' 为占位符或无效，使用当前浏览器页面", file=sys.stderr)

    if not is_placeholder:
        current_url = page.url
        if current_url == "about:blank" or url not in current_url:
            try:
                await page.goto(url, wait_until='domcontentloaded', timeout=30000)
            except Exception as e:
                return _error(f"页面导航失败: {e}")

    # 检查登录状态
    try:
        content = await page.content()
    except Exception as e:
        # 页面可能已关闭，重置状态
        state["page"] = None
        state["context"] = None
        state["playwright"] = None
        return _error(f"页面已断开连接: {e}。请重试")

    if '用户登录' in content or 'passport2' in page.url:
        return {
            "success": False,
            "error": "login_required",
            "message": "需要登录，请先执行 login 命令或在浏览器中手动登录",
            "current_url": page.url,
        }

    return {"success": True, "page": page, "current_url": page.url}


async def close_browser():
    """关闭浏览器（安全清理所有资源）"""
    state = _browser_state
    # 先关闭页面
    if state["page"]:
        try:
            await state["page"].close()
        except Exception:
            pass
    # 再关闭上下文（这会关闭浏览器进程）
    if state["context"]:
        try:
            await state["context"].close()
        except Exception:
            pass
    # 最后关闭 playwright
    if state["playwright"]:
        try:
            await state["playwright"].stop()
        except Exception:
            pass
    state["playwright"] = None
    state["context"] = None
    state["page"] = None


def _error(msg: str) -> dict:
    return {"success": False, "error": msg}


def _success(**kwargs) -> dict:
    return {"success": True, **kwargs}


# ============================================================
# login — 打开浏览器等待用户登录
# ============================================================

async def cmd_login(args):
    """打开浏览器并等待用户登录，使用持久化 profile 保持会话"""
    try:
        from playwright.async_api import async_playwright
    except ImportError:
        return _error("playwright 未安装")

    async with async_playwright() as pw:
        context = await pw.chromium.launch_persistent_context(
            _USER_DATA_DIR,
            channel='msedge',
            headless=args.headless,
            no_viewport=True,
            args=['--start-maximized'],
        )
        page = context.pages[0] if context.pages else await context.new_page()
        try:
            await page.goto(args.url, wait_until='domcontentloaded', timeout=30000)
        except Exception as e:
            await context.close()
            return _error(f"页面导航失败: {e}")

        # 轮询等待登录完成，最长 30 秒
        for _ in range(10):
            await asyncio.sleep(3)
            try:
                current_url = page.url
                if 'passport2' not in current_url and 'login' not in current_url.lower():
                    content = await page.content()
                    if '用户登录' not in content:
                        await context.close()
                        return _success(message="登录成功，会话已保存", current_url=current_url)
            except Exception:
                await asyncio.sleep(2)

        await context.close()
        return _error("登录超时（30秒），请重新执行 login 命令")


# ============================================================
# crawl — 爬取作业页面，提取题目
# ============================================================

async def cmd_crawl(args):
    """爬取作业页面，提取所有题目信息"""
    result = await ensure_browser(args.url, getattr(args, 'headless', False))
    if not result["success"]:
        return result

    page = result["page"]

    try:
        await page.wait_for_load_state('networkidle', timeout=15000)
    except Exception:
        pass
    await asyncio.sleep(2)

    # 统一提取所有题目（选择题 + 填空题）
    questions_info = await page.evaluate(r'''() => {
        const questions = [];
        const qElements = document.querySelectorAll('.mark_name');
        qElements.forEach((el, idx) => {
            const text = el.innerText.trim();
            const match = text.match(/^(\d+)\./);
            if (!match) return;

            const qNum = parseInt(match[1]);
            const container = el.closest('.questionLi') || el.closest('.mark_item') || el.parentElement?.parentElement;
            if (!container) return;

            // 检测题型
            let qType = 'unknown';
            const hasEditor = !!(container.querySelector('textarea[id^="answer"]') || container.querySelector('iframe[id^="ueditor"]'));
            const hasOptions = !!container.querySelector('.answerBg, .clearfix.answerBg');

            if (hasEditor) {
                qType = 'fill_blank';
            } else if (hasOptions) {
                // 根据选项判断是单选、多选还是判断
                const optionCount = container.querySelectorAll('.answerBg, .clearfix.answerBg').length;
                if (optionCount === 2) {
                    const optionTexts = Array.from(container.querySelectorAll('.answerBg')).map(o => o.innerText.trim());
                    if (optionTexts.some(t => t === '√' || t === '对' || t === '正确') ||
                        optionTexts.some(t => t === '×' || t === '错' || t === '错误')) {
                        qType = 'judgment';
                    } else {
                        qType = 'single';
                    }
                } else {
                    // 根据题目文本检测多选题
                    if (text.includes('多选') || text.includes('(多选题)') || text.includes('（多选题）')) {
                        qType = 'multi';
                    } else {
                        qType = 'single';
                    }
                }
            }

            // 提取选项
            const options = [];
            if (hasOptions) {
                const optElements = container.querySelectorAll('.answerBg, .clearfix.answerBg');
                optElements.forEach(opt => {
                    const span = opt.querySelector('span[data]');
                    const label = opt.innerText.trim();
                    if (span) {
                        options.push({
                            key: span.getAttribute('data'),
                            text: label.substring(0, 100)
                        });
                    }
                });
            }

            // 提取题目文本（去掉题号前缀）
            let questionText = text.replace(/^\d+\.\s*/, '').substring(0, 500);

            questions.push({
                num: qNum,
                index: idx,
                text: questionText,
                type: qType,
                options: options,
            });
        });
        return questions;
    }''')

    questions_info.sort(key=lambda q: q['num'])

    # 生成 answers 模板 — Agent 直接填充答案后传给 fill 命令
    answers_template = {str(q['num']): "" for q in questions_info}

    return _success(
        total_questions=len(questions_info),
        questions=questions_info,
        answers_template=answers_template,
        fill_hint="将 answers_template 中的空字符串填入答案，然后直接调用 xxt fill 命令传入 answers 参数",
        fill_example_format='{"1":"A","2":"B","3":"答案文本"}',
        page_url=page.url,
        message=f"成功提取 {len(questions_info)} 道题目，请根据题目生成答案后直接调用 fill 命令",
    )


# ============================================================
# fill — 根据答案填充作业页面
# ============================================================

async def cmd_fill(args):
    """根据提供的答案填充作业页面"""
    result = await ensure_browser(args.url, getattr(args, 'headless', False))
    if not result["success"]:
        return result

    page = result["page"]

    # 解析答案
    answers = _parse_answers(args)
    if isinstance(answers, dict) and "error" in answers:
        return answers

    try:
        await page.wait_for_load_state('networkidle', timeout=15000)
    except Exception:
        pass
    await asyncio.sleep(2)

    # 获取题目信息
    questions_info = await page.evaluate(r'''() => {
        const questions = [];
        const qElements = document.querySelectorAll('.mark_name');
        qElements.forEach((el, idx) => {
            const text = el.innerText.trim();
            const match = text.match(/^(\d+)\./);
            if (match) {
                const container = el.closest('.questionLi') || el.closest('.mark_item') || el.parentElement?.parentElement;
                const hasEditor = container ? !!(container.querySelector('textarea[id^="answer"]') || container.querySelector('iframe[id^="ueditor"]')) : false;
                questions.push({
                    num: parseInt(match[1]),
                    index: idx,
                    is_blank: hasEditor,
                });
            }
        });
        return questions;
    }''')

    filled = 0
    skipped = 0
    errors = []

    for q in questions_info:
        q_num = q['num']
        if q_num not in answers:
            skipped += 1
            continue

        answer = str(answers[q_num])

        if q['is_blank'] or _looks_like_blank_answer(answer):
            # 填空题
            ok = await _fill_blank(page, q['index'], answer)
        else:
            # 选择题
            ok = await _fill_choice(page, q['index'], q_num, answer)

        if ok:
            filled += 1
        else:
            errors.append(f"题目 {q_num}")

        # 填空题(UEditor)需要更长等待，选择题较短
        if q['is_blank'] or _looks_like_blank_answer(answer):
            await asyncio.sleep(0.5)
        else:
            await asyncio.sleep(0.15)

    # 填充后验证：重新检查已填充题目的实际状态
    verify_result = None
    if filled > 0:
        await asyncio.sleep(0.5)
        try:
            verify_result = await page.evaluate(r'''() => {
                const results = [];
                const qElements = document.querySelectorAll('.mark_name');
                qElements.forEach((el, idx) => {
                    const text = el.innerText.trim();
                    const match = text.match(/^(\d+)\./);
                    if (!match) return;
                    const qNum = parseInt(match[1]);
                    const container = el.closest('.questionLi') || el.closest('.mark_item') || el.parentElement?.parentElement;
                    if (!container) return;

                    let answered = false;
                    // 选择题
                    const selected = container.querySelector('.check_answerColor, .onBg, .selected, [aria-checked="true"]');
                    if (selected) answered = true;
                    // 填空题
                    if (!answered) {
                        const ta = container.querySelector('textarea[id^="answer"]');
                        if (ta && ta.value.trim()) answered = true;
                    }
                    if (!answered) {
                        const iframe = container.querySelector('iframe[id^="ueditor"]');
                        if (iframe) {
                            try {
                                const doc = iframe.contentDocument;
                                if (doc && doc.body && doc.body.innerText.trim()) answered = true;
                            } catch (e) {}
                        }
                    }
                    results.push({num: qNum, answered});
                });
                return results;
            }''')
        except Exception:
            pass

    # 填充完成后自动暂存（防止浏览器关闭后丢失答案）
    saved = False
    if filled > 0:
        try:
            save_result = await page.evaluate(r'''() => {
                const btns = document.querySelectorAll('a, button, input[type="button"], div[onclick], span[onclick]');
                for (const btn of btns) {
                    const text = (btn.innerText || btn.value || '').trim();
                    if (text.includes('暂存') || text === '保存' || text.includes('暂存作业')) {
                        btn.click();
                        return {clicked: true, text: text};
                    }
                }
                return {clicked: false};
            }''')
            if save_result.get('clicked'):
                saved = True
                await asyncio.sleep(2)  # 等待暂存完成
        except Exception as e:
            logging.warning(f"auto-save after fill failed: {e}")

    # 统计验证结果
    verified_count = 0
    if verify_result:
        answered_nums = {q['num'] for q in verify_result if q['answered']}
        target_nums = {q['num'] for q in questions_info if q['num'] in answers}
        verified_count = len(answered_nums & target_nums)

    msg = f"已填充 {filled} 道，跳过 {skipped} 道"
    if verify_result and verified_count < filled:
        msg += f"，验证通过 {verified_count} 道"
    if errors:
        msg += f"，失败: {', '.join(errors)}"
    if saved:
        msg += "，已自动暂存"
    elif filled > 0:
        msg += "，暂存失败（可能需要手动暂存）"

    return _success(
        filled=filled, skipped=skipped, errors=errors,
        total_answers=len(answers), saved=saved,
        verified=verified_count if verify_result else None,
        message=msg,
    )


def _parse_answers(args) -> dict:
    """解析答案参数，支持 JSON 字符串、环境变量、文件路径和 Agent 上下文注入（优先级：--answers > XXT_ANSWERS > --answers-file > 上下文注入）"""
    answers_raw = getattr(args, 'answers', '') or ''
    if not answers_raw:
        answers_raw = os.environ.get('XXT_ANSWERS', '')
    if not answers_raw:
        answers_file = getattr(args, 'answers_file', '') or ''
        if answers_file:
            try:
                with open(answers_file, 'r', encoding='utf-8') as f:
                    answers_raw = f.read()
            except FileNotFoundError:
                return {"success": False, "error": f"答案文件不存在: {answers_file}"}
            except Exception as e:
                return {"success": False, "error": f"读取答案文件失败: {e}"}

    # 尝试从 Agent 上下文注入的 working_memory / recent_history 中提取答案
    if not answers_raw:
        answers_raw = _extract_answers_from_context()

    if not answers_raw:
        return {"success": False, "error": "未提供答案。请通过 --answers 参数、XXT_ANSWERS 环境变量或 --answers-file 传入"}

    try:
        parsed = json.loads(answers_raw)
    except json.JSONDecodeError as e:
        return {"success": False, "error": f"答案 JSON 解析失败: {e}"}

    if isinstance(parsed, list):
        answers = {}
        for item in parsed:
            if isinstance(item, dict):
                qid = item.get("questionId") or item.get("question_id") or item.get("num") or item.get("id")
                ans = item.get("answer") or item.get("value") or item.get("ans")
                if qid is not None and ans is not None:
                    answers[int(qid)] = ans
        return answers if answers else {"success": False, "error": "数组格式中未提取到有效答案"}
    elif isinstance(parsed, dict):
        return {int(k): v for k, v in parsed.items()}
    else:
        return {"success": False, "error": f"不支持的答案格式: {type(parsed).__name__}"}


def _extract_answers_from_context() -> str:
    """从 Agent 上下文注入的环境变量/文件中提取答案数据

    优先级：XXT_CONTEXT_FILE > XXT_TOOL_HISTORY_FILE > XXT_WORKING_MEMORY
    扫描 working_memory、recent_history、tool_history，查找包含答案 JSON 的条目。
    答案格式: {"1":"A","2":"C","3":"答案文本"} 或包含 answers_template/answers 字段的对象。
    """
    import re

    # 收集所有要扫描的数据源（列表 of entries）
    sources = []

    # 1. XXT_CONTEXT_FILE — Rust 端写入的完整上下文 JSON 文件
    context_file = os.environ.get('XXT_CONTEXT_FILE', '')
    if context_file and os.path.isfile(context_file):
        try:
            with open(context_file, 'r', encoding='utf-8') as f:
                ctx_data = json.load(f)
            # Extract entries from working_memory and recent_history
            for key in ('working_memory', 'recent_history'):
                val = ctx_data.get(key)
                if isinstance(val, list):
                    sources.append(val)
        except (json.JSONDecodeError, OSError):
            pass

    # 2. XXT_TOOL_HISTORY_FILE — tool history from previous calls
    history_file = os.environ.get('XXT_TOOL_HISTORY_FILE', '')
    if history_file and os.path.isfile(history_file):
        try:
            with open(history_file, 'r', encoding='utf-8') as f:
                hist_data = json.load(f)
            if isinstance(hist_data, list):
                sources.append(hist_data)
        except (json.JSONDecodeError, OSError):
            pass

    # 3. Legacy env vars (fallback)
    for env_key in ('XXT_WORKING_MEMORY',):
        raw = os.environ.get(env_key, '')
        if not raw:
            continue
        try:
            entries = json.loads(raw)
            if isinstance(entries, list):
                sources.append(entries)
        except json.JSONDecodeError:
            continue

    # Scan all sources
    for entries in sources:
        for entry in reversed(entries):
            data = entry.get('data') if isinstance(entry, dict) else None
            if isinstance(data, dict):
                result = _try_extract_from_dict(data)
                if result:
                    return result

            content = entry.get('content', '') if isinstance(entry, dict) else ''
            if not isinstance(content, str) or not content:
                continue

            try:
                obj = json.loads(content)
                if isinstance(obj, dict):
                    result = _try_extract_from_dict(obj)
                    if result:
                        return result
            except json.JSONDecodeError:
                pass

            for m in re.finditer(r'\{[^{}]*\}', content):
                try:
                    obj = json.loads(m.group())
                    if isinstance(obj, dict) and _looks_like_answers(obj):
                        return json.dumps(obj, ensure_ascii=False)
                except json.JSONDecodeError:
                    continue

    return ''


def _try_extract_from_dict(d: dict) -> str:
    """尝试从字典中提取答案数据"""
    # 直接是答案格式 {"1":"A","2":"C"}
    if _looks_like_answers(d):
        return json.dumps(d, ensure_ascii=False)

    # 包含 answers / answers_template 字段
    for key in ('answers', 'answers_template', 'answer_data'):
        val = d.get(key)
        if isinstance(val, dict) and _looks_like_answers(val):
            return json.dumps(val, ensure_ascii=False)

    return ''


def _looks_like_answers(d: dict) -> bool:
    """判断字典是否像答案数据（键为题号，值为答案）"""
    if not d:
        return False
    # 至少一半的键是纯数字
    numeric_keys = sum(1 for k in d if str(k).isdigit())
    return numeric_keys >= len(d) * 0.5 and numeric_keys >= 2


def _looks_like_blank_answer(answer: str) -> bool:
    """判断答案是否像填空题答案（非选择题选项字母）"""
    answer = answer.strip()
    if not answer:
        return False
    # 纯字母序列（如 "A"、"ABE"、"ABEFG"）都是选择题答案
    if all(c in 'ABCDEFGHabcdefgh' for c in answer):
        return False
    return True


async def _fill_blank(page, index: int, answer: str) -> bool:
    """填充填空题（支持 UEditor 富文本编辑器），带 HTML 转义和填充验证"""
    try:
        result = await page.evaluate('''(answerText, idx) => {
            // HTML 转义，防止 innerHTML 注入
            function escapeHtml(text) {
                const div = document.createElement('div');
                div.textContent = text;
                return div.innerHTML;
            }
            const safeAnswer = escapeHtml(answerText);

            const qTitle = document.querySelectorAll('.mark_name')[idx];
            if (!qTitle) return {success: false, reason: 'no_title'};
            let container = qTitle.closest('.questionLi') || qTitle.parentElement?.parentElement;
            if (!container) return {success: false, reason: 'no_container'};

            const iframe = container.querySelector('iframe[id^="ueditor"]');

            // 方法1: UE.instants API
            if (typeof UE !== 'undefined' && UE.instants && iframe) {
                const keys = Object.keys(UE.instants);
                for (const key of keys) {
                    const ed = UE.instants[key];
                    try {
                        const edIframe = ed.container?.querySelector('iframe');
                        if (edIframe && edIframe.id === iframe.id) {
                            if (ed.focus) ed.focus();
                            ed.setContent('<p>' + safeAnswer + '</p>', true);
                            if (ed.fireEvent) ed.fireEvent('contentchange');
                            if (ed.blur) ed.blur();
                            return {success: true, method: 'ueditor_api'};
                        }
                    } catch (e) {}
                }
            }

            // 方法2: UEditor iframe body + 同步 textarea
            if (iframe) {
                try {
                    const doc = iframe.contentDocument;
                    if (doc && doc.body) {
                        doc.body.innerHTML = '<p>' + safeAnswer + '</p>';
                        const textarea = container.querySelector('textarea[id^="answer"]');
                        if (textarea) {
                            textarea.value = answerText;
                            textarea.dispatchEvent(new Event('change', {bubbles: true}));
                        }
                        return {success: true, method: 'ueditor_fallback'};
                    }
                } catch (e) {
                    // cross-origin，跳过
                }
            }

            // 方法3: 普通 textarea
            const textarea = container.querySelector('textarea[id^="answer"]');
            if (textarea) {
                textarea.value = answerText;
                textarea.dispatchEvent(new Event('change', {bubbles: true}));
                textarea.dispatchEvent(new Event('input', {bubbles: true}));
                return {success: true, method: 'textarea'};
            }

            // 方法4: contenteditable
            const editable = container.querySelector('[contenteditable="true"]');
            if (editable) {
                editable.innerHTML = '<p>' + safeAnswer + '</p>';
                return {success: true, method: 'contenteditable'};
            }

            return {success: false, reason: 'no_input_found'};
        }''', answer, index)

        if not result.get('success'):
            return False

        # 验证填充结果
        await asyncio.sleep(0.2)
        verified = await page.evaluate('''(idx) => {
            const qTitle = document.querySelectorAll('.mark_name')[idx];
            if (!qTitle) return false;
            let container = qTitle.closest('.questionLi') || qTitle.parentElement?.parentElement;
            if (!container) return false;

            const iframe = container.querySelector('iframe[id^="ueditor"]');
            if (iframe) {
                try {
                    const doc = iframe.contentDocument;
                    if (doc && doc.body && doc.body.innerText.trim()) return true;
                } catch (e) {}
            }
            const textarea = container.querySelector('textarea[id^="answer"]');
            if (textarea && textarea.value.trim()) return true;
            const editable = container.querySelector('[contenteditable="true"]');
            if (editable && editable.innerText.trim()) return true;
            return false;
        }''', index)

        return verified
    except Exception as e:
        logging.warning(f"_fill_blank failed for index={index}: {e}")
        return False


async def _fill_choice(page, index: int, q_num: int, answer: str) -> bool:
    """填充选择题（支持多选），每个选项点击后等待 UI 更新"""
    letters = [c for c in answer.upper() if c.isalpha()]
    if not letters:
        return False

    try:
        result = await page.evaluate(f'''(letters) => {{
            const qTitle = document.querySelectorAll('.mark_name')[{index}];
            if (!qTitle) return {{success: false, reason: 'no_title'}};
            let container = qTitle.closest('.questionLi') || qTitle.closest('.mark_item') || qTitle.parentElement;
            if (!container) container = qTitle.parentElement?.parentElement;
            if (!container) return {{success: false, reason: 'no_container'}};

            const options = container.querySelectorAll('.answerBg');
            const clicked = [];
            const skipped = [];
            const notFound = [];

            for (const letter of letters) {{
                let found = false;
                for (const opt of options) {{
                    const span = opt.querySelector('span[data]');
                    if (span && span.getAttribute('data') === letter) {{
                        const isSelected = opt.classList.contains('check_answerColor') ||
                                           opt.classList.contains('onBg') ||
                                           opt.classList.contains('selected') ||
                                           opt.getAttribute('aria-checked') === 'true';
                        if (isSelected) {{
                            skipped.push(letter);
                        }} else {{
                            opt.click();
                            clicked.push(letter);
                        }}
                        found = true;
                        break;
                    }}
                }}
                if (!found) notFound.push(letter);
            }}

            return {{
                success: clicked.length > 0 || skipped.length > 0,
                clicked: clicked,
                skipped: skipped,
                notFound: notFound,
            }};
        }}''', letters)

        # 多选时每个点击间隔等待，避免竞态
        if len(letters) > 1 and result.get('clicked'):
            await asyncio.sleep(0.3 * len(result['clicked']))

        return result.get('success', False)
    except Exception as e:
        logging.warning(f"_fill_choice failed for q_num={q_num}, index={index}: {e}")
        return False


# ============================================================
# submit — 暂存并提交作业
# ============================================================

async def cmd_submit(args):
    """暂存并提交作业"""
    result = await ensure_browser(args.url, getattr(args, 'headless', False))
    if not result["success"]:
        return result

    page = result["page"]

    await page.evaluate('window.scrollTo(0, document.body.scrollHeight)')
    await asyncio.sleep(1)

    save_clicked = False
    submit_clicked = False

    # 暂存
    try:
        save_result = await page.evaluate(r'''() => {
            const btns = document.querySelectorAll('a, button, input[type="button"], div[onclick], span[onclick]');
            for (const btn of btns) {
                const text = (btn.innerText || btn.value || '').trim();
                if (text.includes('暂存') || text === '保存' || text.includes('暂存作业')) {
                    btn.click();
                    return {clicked: true, text};
                }
            }
            return {clicked: false};
        }''')
        if save_result.get('clicked'):
            save_clicked = True
            await asyncio.sleep(3)
            await _dismiss_dialogs(page)
    except Exception as e:
        return _error(f"暂存失败: {e}")

    # 提交
    try:
        await page.evaluate('window.scrollTo(0, document.body.scrollHeight)')
        await asyncio.sleep(1)

        submit_result = await page.evaluate(r'''() => {
            const btns = document.querySelectorAll('a, button, input[type="button"], div[onclick], span[onclick]');
            for (const btn of btns) {
                const text = (btn.innerText || btn.value || '').trim();
                if (text === '提交' || text.includes('交卷') || text.includes('我要交卷') || text.includes('提交作业')) {
                    btn.click();
                    return {clicked: true, text};
                }
            }
            return {clicked: false};
        }''')

        if submit_result.get('clicked'):
            submit_clicked = True
            await asyncio.sleep(2)
            await _dismiss_dialogs(page)
    except Exception as e:
        return {"success": False, "save_clicked": save_clicked, "submit_clicked": False, "error": f"提交失败: {e}"}

    return _success(
        save_clicked=save_clicked,
        submit_clicked=submit_clicked,
        message="作业已提交" if submit_clicked else "已暂存，未找到提交按钮，请手动提交",
    )


async def _dismiss_dialogs(page, max_rounds=3):
    """点击确认对话框"""
    for _ in range(max_rounds):
        try:
            confirm = page.locator('text=确定')
            if await confirm.count() > 0:
                await confirm.first.click()
                await asyncio.sleep(2)
            else:
                break
        except Exception:
            break


# ============================================================
# screenshot — 对当前页面截图
# ============================================================

async def cmd_screenshot(args):
    """对当前页面截图并保存"""
    result = await ensure_browser(args.url, getattr(args, 'headless', False))
    if not result["success"]:
        return result

    page = result["page"]

    try:
        await page.wait_for_load_state('networkidle', timeout=15000)
    except Exception:
        pass
    await asyncio.sleep(1)

    os.makedirs(_SCREENSHOT_DIR, exist_ok=True)
    filename = getattr(args, 'output', '') or f"screenshot_{asyncio.get_running_loop().time():.0f}.png"
    path = os.path.join(_SCREENSHOT_DIR, filename)

    await page.screenshot(path=path, full_page=True)

    return _success(path=path, message=f"截图已保存: {path}")


# ============================================================
# check — 检查填充状态
# ============================================================

async def cmd_check(args):
    """检查页面上各题目的填充状态"""
    result = await ensure_browser(args.url, getattr(args, 'headless', False))
    if not result["success"]:
        return result

    page = result["page"]

    try:
        await page.wait_for_load_state('networkidle', timeout=15000)
    except Exception:
        pass
    await asyncio.sleep(1)

    status = await page.evaluate(r'''() => {
        const results = [];
        const qElements = document.querySelectorAll('.mark_name');
        qElements.forEach((el, idx) => {
            const text = el.innerText.trim();
            const match = text.match(/^(\d+)\./);
            if (!match) return;

            const qNum = parseInt(match[1]);
            const container = el.closest('.questionLi') || el.closest('.mark_item') || el.parentElement?.parentElement;
            if (!container) return;

            let answered = false;
            let answerText = '';

            // 检查选择题
            const selected = container.querySelector('.check_answerColor, .onBg, .selected, [aria-checked="true"]');
            if (selected) {
                answered = true;
                const span = selected.querySelector('span[data]');
                answerText = span ? span.getAttribute('data') : selected.innerText.trim().substring(0, 20);
            }

            // 检查填空题
            if (!answered) {
                const textarea = container.querySelector('textarea[id^="answer"]');
                if (textarea && textarea.value.trim()) {
                    answered = true;
                    answerText = textarea.value.trim().substring(0, 50);
                }
            }
            if (!answered) {
                const iframe = container.querySelector('iframe[id^="ueditor"]');
                if (iframe && iframe.contentDocument) {
                    const body = iframe.contentDocument.body;
                    if (body && body.innerText.trim()) {
                        answered = true;
                        answerText = body.innerText.trim().substring(0, 50);
                    }
                }
            }

            results.push({num: qNum, answered, answer: answerText});
        });
        return results;
    }''')

    answered_count = sum(1 for q in status if q['answered'])
    total = len(status)

    return _success(
        total=total,
        answered=answered_count,
        unanswered=total - answered_count,
        questions=status,
        message=f"共 {total} 题，已答 {answered_count} 题，未答 {total - answered_count} 题",
    )


# ============================================================
# 守护进程模式 — 保持浏览器常驻，通过 socket 通信
# ============================================================

async def _handle_client(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
    """处理来自 CLI 的请求"""
    try:
        data = await asyncio.wait_for(reader.readline(), timeout=300)
        request = json.loads(data.decode('utf-8'))
    except Exception as e:
        response = _error(f"请求解析失败: {e}")
        writer.write(json.dumps(response, ensure_ascii=False).encode('utf-8') + b'\n')
        await writer.drain()
        writer.close()
        return

    command = request.get('command', '')
    args_dict = request.get('args', {})

    # 构造 argparse.Namespace
    args = argparse.Namespace(**args_dict)

    try:
        if command == 'login':
            result = await cmd_login(args)
        elif command == 'crawl':
            result = await cmd_crawl(args)
        elif command == 'fill':
            result = await cmd_fill(args)
        elif command == 'submit':
            result = await cmd_submit(args)
        elif command == 'screenshot':
            result = await cmd_screenshot(args)
        elif command == 'check':
            result = await cmd_check(args)
        elif command == 'close':
            await close_browser()
            result = _success(message="浏览器已关闭")
        elif command == 'status':
            has_browser = _browser_state["page"] is not None
            result = _success(browser_active=has_browser, url=_browser_state["page"].url if has_browser else None)
        else:
            result = _error(f"未知命令: {command}")
    except Exception as e:
        result = _error(f"执行异常: {e}")

    response_data = json.dumps(result, ensure_ascii=False).encode('utf-8') + b'\n'
    writer.write(response_data)
    await writer.drain()
    writer.close()


async def cmd_daemon(args):
    """启动守护进程"""
    port = getattr(args, 'port', _DEFAULT_DAEMON_PORT)

    server = await asyncio.start_server(_handle_client, '127.0.0.1', port)
    print(json.dumps(_success(message=f"守护进程已启动，监听 127.0.0.1:{port}", port=port)), flush=True)

    async with server:
        await server.serve_forever()


def _send_to_daemon(request: dict, port: int = _DEFAULT_DAEMON_PORT) -> dict:
    """向守护进程发送请求"""
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
            sock.settimeout(120)
            sock.connect(('127.0.0.1', port))
            sock.sendall(json.dumps(request, ensure_ascii=False).encode('utf-8') + b'\n')

            data = b''
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                data += chunk
                if b'\n' in data:
                    break
        return json.loads(data.decode('utf-8'))
    except (ConnectionRefusedError, socket.timeout, OSError):
        return None


def _is_daemon_running(port: int = _DEFAULT_DAEMON_PORT) -> bool:
    """检查守护进程是否在运行"""
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.settimeout(2)
        sock.connect(('127.0.0.1', port))
        sock.close()
        return True
    except (ConnectionRefusedError, socket.timeout, OSError):
        return False


# ============================================================
# 主入口
# ============================================================

def parse_args():
    parser = argparse.ArgumentParser(description='超星学习通自动答题工具')
    subparsers = parser.add_subparsers(dest='command')

    # login
    p = subparsers.add_parser('login', help='打开浏览器等待登录')
    p.add_argument('--url', required=True)
    p.add_argument('--headless', action='store_true')

    # crawl
    p = subparsers.add_parser('crawl', help='爬取题目')
    p.add_argument('--url', required=True)
    p.add_argument('--headless', action='store_true')

    # fill
    p = subparsers.add_parser('fill', help='填充答案')
    p.add_argument('--url', required=True)
    p.add_argument('--answers', default='')
    p.add_argument('--answers-file', default='')
    p.add_argument('--headless', action='store_true')

    # submit
    p = subparsers.add_parser('submit', help='暂存并提交')
    p.add_argument('--url', required=True)
    p.add_argument('--headless', action='store_true')

    # screenshot
    p = subparsers.add_parser('screenshot', help='截图')
    p.add_argument('--url', required=True)
    p.add_argument('--output', default='')
    p.add_argument('--headless', action='store_true')

    # check
    p = subparsers.add_parser('check', help='检查填充状态')
    p.add_argument('--url', required=True)
    p.add_argument('--headless', action='store_true')

    # daemon
    p = subparsers.add_parser('daemon', help='启动守护进程')
    p.add_argument('--port', type=int, default=_DEFAULT_DAEMON_PORT)

    return parser.parse_args()


async def main():
    args = parse_args()

    if not args.command:
        print(json.dumps(_error("请指定子命令: login, crawl, fill, submit, screenshot, check, daemon")))
        sys.exit(1)

    # 守护进程模式
    if args.command == 'daemon':
        await cmd_daemon(args)
        return

    # CLI 模式：尝试通过守护进程执行（复用浏览器）
    daemon_port = getattr(args, 'port', _DEFAULT_DAEMON_PORT)
    if _is_daemon_running(daemon_port):
        request = {"command": args.command, "args": vars(args)}
        result = _send_to_daemon(request, daemon_port)
        if result is not None:
            print(json.dumps(result, ensure_ascii=False, indent=2))
            return
        # 守护进程无响应，降级到本地执行

    # 本地执行
    try:
        if args.command == 'login':
            result = await cmd_login(args)
        elif args.command == 'crawl':
            result = await cmd_crawl(args)
        elif args.command == 'fill':
            result = await cmd_fill(args)
        elif args.command == 'submit':
            result = await cmd_submit(args)
        elif args.command == 'screenshot':
            result = await cmd_screenshot(args)
        elif args.command == 'check':
            result = await cmd_check(args)
        else:
            result = _error(f"未知命令: {args.command}")
    except Exception as e:
        result = _error(f"执行异常: {e}")

    print(json.dumps(result, ensure_ascii=False, indent=2))

    # fill/submit 完成后等待 20 秒再关闭浏览器（让用户有时间查看页面）
    if args.command in ('fill', 'submit') and result.get('success'):
        await asyncio.sleep(20)

    # CLI 模式下在退出前优雅关闭浏览器
    await asyncio.sleep(1)
    await close_browser()


if __name__ == '__main__':
    asyncio.run(main())

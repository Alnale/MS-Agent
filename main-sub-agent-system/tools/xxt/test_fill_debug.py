"""
调试脚本：测试 fill 命令，添加详细日志，浏览器不会自动关闭。
用法: python test_fill_debug.py
"""

import asyncio
import json
import sys
import os

sys.path.insert(0, os.path.dirname(__file__))

from auto_answer import ensure_browser, close_browser, _browser_state

TEST_URL = "https://mooc1-api.chaoxing.com/mooc-ans/mooc2/work/dowork?courseId=261818141&classId=142757648&cpi=406056404&workId=53866650&answerId=0&standardEnc=e0133dd0d875103fb9d110233ed8c4d8&enc=91881b4072fdb17d7282f300689b8c85"
TEST_ANSWERS = {"1": "A", "2": "B", "3": "C"}


async def main():
    print("=" * 60)
    print("Step 1: 启动浏览器并导航到页面...")
    print("=" * 60)

    result = await ensure_browser(TEST_URL, headless=False)
    print(f"ensure_browser 结果: {json.dumps({k: v for k, v in result.items() if k != 'page'}, ensure_ascii=False)}")

    if not result["success"]:
        print(f"失败: {result}")
        return

    page = result["page"]
    print(f"当前 URL: {page.url}")
    print(f"浏览器状态: playwright={_browser_state['playwright'] is not None}, context={_browser_state['context'] is not None}, page={_browser_state['page'] is not None}")

    print("\n" + "=" * 60)
    print("Step 2: 等待页面加载...")
    print("=" * 60)

    try:
        await page.wait_for_load_state('networkidle', timeout=15000)
    except Exception as e:
        print(f"networkidle 超时 (可忽略): {e}")

    await asyncio.sleep(3)
    print(f"页面标题: {await page.title()}")
    print(f"页面 URL: {page.url}")

    print("\n" + "=" * 60)
    print("Step 3: 检查页面内容...")
    print("=" * 60)

    content = await page.content()
    has_login = '用户登录' in content or 'passport2' in page.url
    print(f"需要登录: {has_login}")
    print(f"页面内容长度: {len(content)} 字符")

    # 检查是否有题目
    question_count = await page.evaluate('''() => {
        return document.querySelectorAll('.mark_name').length;
    }''')
    print(f"找到 .mark_name 元素数量: {question_count}")

    if question_count == 0:
        print("警告: 未找到题目元素！")
        print("页面前 2000 字符:")
        print(content[:2000])

    print("\n" + "=" * 60)
    print("Step 4: 测试填充功能...")
    print("=" * 60)

    answers = TEST_ANSWERS
    filled = 0
    skipped = 0
    errors = []

    questions_info = await page.evaluate('''() => {
        const questions = [];
        const qElements = document.querySelectorAll('.mark_name');
        qElements.forEach((el, idx) => {
            const text = el.innerText.trim();
            const match = text.match(/^(\\d+)\\./);
            if (match) {
                questions.push({
                    num: parseInt(match[1]),
                    index: idx
                });
            }
        });
        return questions;
    }''')

    print(f"获取到 {len(questions_info)} 道题目")
    print(f"答案: {answers}")

    for q in questions_info:
        q_num = q['num']
        if q_num not in answers:
            skipped += 1
            continue

        answer = answers[q_num]
        print(f"\n填充题目 {q_num}，答案: {answer}")

        # 选择题处理
        for letter in answer:
            result = await page.evaluate(f'''(num) => {{
                const qTitle = document.querySelectorAll('.mark_name')[{q['index']}];
                if (!qTitle) return {{success: false, reason: 'no title'}};
                let container = qTitle.closest('.questionLi') || qTitle.closest('.mark_item') || qTitle.parentElement;
                if (!container) container = qTitle.parentElement.parentElement;
                if (!container) return {{success: false, reason: 'no container'}};

                const options = container.querySelectorAll('.answerBg, .clearfix.answerBg');
                for (const opt of options) {{
                    const span = opt.querySelector('span[data]');
                    if (span && span.getAttribute('data') === '{letter}') {{
                        opt.click();
                        return {{success: true, letter: '{letter}', data: '{letter}'}};
                    }}
                }}
                return {{success: false, reason: 'option not found'}};
            }}''', q_num)

            print(f"  结果: {result}")
            if result.get('success'):
                filled += 1
            else:
                errors.append(f"题目 {q_num} 选项 {letter}: {result.get('reason')}")

        await asyncio.sleep(0.1)

    print(f"\n填充完成: filled={filled}, skipped={skipped}, errors={errors}")

    # 截图保存
    screenshot_path = os.path.join(os.path.dirname(__file__), 'test_fill_result.png')
    await page.screenshot(path=screenshot_path, full_page=True)
    print(f"截图已保存: {screenshot_path}")

    print("\n" + "=" * 60)
    print("Step 5: 浏览器将保持打开 30 秒...")
    print("=" * 60)
    await asyncio.sleep(30)

    await close_browser()
    print("浏览器已关闭。")


if __name__ == '__main__':
    asyncio.run(main())

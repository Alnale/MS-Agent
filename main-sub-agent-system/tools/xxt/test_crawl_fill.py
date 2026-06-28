"""
测试脚本：crawl + fill + check 完整流程，浏览器保持可见。
用法: python test_crawl_fill.py
"""

import asyncio
import json
import sys
import os

sys.path.insert(0, os.path.dirname(__file__))

from auto_answer import ensure_browser, close_browser, _browser_state, cmd_crawl, cmd_fill, cmd_check

TEST_URL = "https://mooc1-api.chaoxing.com/mooc-ans/mooc2/work/dowork?courseId=260607599&classId=139474840&cpi=406056404&workId=53492715&answerId=55879093&standardEnc=8458328ae4c89b16811accc7204a93e9&enc=a75c85be4d1bca80d1c9446343d9d5b3"


async def main():
    print("=" * 60)
    print("Step 1: 启动浏览器并爬取题目 (crawl)...")
    print("=" * 60)

    import argparse
    crawl_args = argparse.Namespace(url=TEST_URL, headless=False, command='crawl')
    crawl_result = await cmd_crawl(crawl_args)
    print(f"crawl 结果:\n{json.dumps(crawl_result, ensure_ascii=False, indent=2)}")

    if not crawl_result.get("success"):
        print(f"爬取失败: {crawl_result}")
        await asyncio.sleep(10)
        await close_browser()
        return

    print("\n" + "=" * 60)
    print("Step 2: 填充测试答案 (fill)...")
    print("=" * 60)

    # 根据爬取结果生成测试答案
    test_answers = {}
    for q in crawl_result.get("questions", []):
        num = str(q["num"])
        if q["type"] == "single":
            # 选择题填第一个选项
            if q.get("options"):
                test_answers[num] = q["options"][0]["key"]
            else:
                test_answers[num] = "A"
        elif q["type"] == "fill_blank":
            test_answers[num] = f"测试答案_{num}"
        else:
            test_answers[num] = "A"

    print(f"测试答案: {json.dumps(test_answers, ensure_ascii=False)}")

    fill_args = argparse.Namespace(
        url=TEST_URL,
        headless=False,
        answers=json.dumps(test_answers, ensure_ascii=False),
        answers_file='',
        command='fill',
    )
    fill_result = await cmd_fill(fill_args)
    print(f"\nfill 结果:\n{json.dumps(fill_result, ensure_ascii=False, indent=2)}")

    print("\n" + "=" * 60)
    print("Step 3: 检查填充状态 (check)...")
    print("=" * 60)

    check_args = argparse.Namespace(url=TEST_URL, headless=False, command='check')
    check_result = await cmd_check(check_args)
    print(f"check 结果:\n{json.dumps(check_result, ensure_ascii=False, indent=2)}")

    # 截图
    page = _browser_state.get("page")
    if page:
        screenshot_path = os.path.join(os.path.dirname(__file__), 'test_crawl_fill_result.png')
        await page.screenshot(path=screenshot_path, full_page=True)
        print(f"\n截图已保存: {screenshot_path}")

    print("\n" + "=" * 60)
    print("浏览器将保持打开 60 秒，请查看页面状态...")
    print("=" * 60)
    await asyncio.sleep(60)

    await close_browser()
    print("浏览器已关闭。")


if __name__ == '__main__':
    asyncio.run(main())

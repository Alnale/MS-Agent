"""Quick test: verify that cmd_fill launches a browser (headless=False) and navigates."""
import asyncio
import json
import sys
import os

sys.path.insert(0, os.path.dirname(__file__))

from auto_answer import cmd_fill, close_browser
import argparse


async def main():
    # Use a dummy URL that will at least trigger the browser launch
    args = argparse.Namespace(
        url="https://mooc1.chaoxing.com/mycourse/studentcourse?courseId=239727699",
        answers=json.dumps({"1": "B", "2": "C", "3": "D"}),
        answers_file="",
        headless=False,
    )
    print("Calling cmd_fill ...", flush=True)
    try:
        result = await cmd_fill(args)
        print(json.dumps(result, ensure_ascii=False, indent=2))
    except Exception as e:
        print(f"Exception: {e}")
    finally:
        await close_browser()


if __name__ == "__main__":
    asyncio.run(main())

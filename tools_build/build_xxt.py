# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for xxt auto_answer tool"""

block_cipher = None

a = Analysis(
    ['../main-sub-agent-system/tools/xxt/auto_answer.py'],
    pathex=[],
    binaries=[],
    datas=[
        ('../main-sub-agent-system/tools/xxt/schema.json', '.'),
        ('../main-sub-agent-system/tools/xxt/description.txt', '.'),
    ],
    hiddenimports=[
        'playwright',
        'playwright.async_api',
        'playwright._impl',
        'playwright._impl._driver',
    ],
    hookspath=[],
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.zipfiles,
    a.datas,
    [],
    name='auto_answer',
    debug=False,
    strip=False,
    upx=True,
    runtime_tmpdir=None,
    console=True,
)

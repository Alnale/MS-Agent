# -*- mode: python ; coding: utf-8 -*-


a = Analysis(
    ['C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\xxt\\auto_answer.py'],
    pathex=[],
    binaries=[],
    datas=[('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\xxt\\schema.json', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\xxt\\description.txt', '.')],
    hiddenimports=['playwright', 'playwright.async_api', 'playwright._impl', 'playwright._impl._driver'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='auto_answer',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

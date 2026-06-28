# -*- mode: python ; coding: utf-8 -*-


a = Analysis(
    ['C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\server.py'],
    pathex=[],
    binaries=[],
    datas=[('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\ocr_engine.py', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\index.html', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\styles.css', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\app.js', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\icon.png', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\icon01.png', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\pdf.min.js', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\pdf.worker.min.js', '.'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\fonts', 'fonts'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\help_docs', 'help_docs'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\music', 'music'), ('C:\\Users\\asus\\Desktop\\MS Agent\\main-sub-agent-system\\tools\\DocFlow\\kmind-markdown-to-mindmap-0.1.0', 'kmind-markdown-to-mindmap-0.1.0')],
    hiddenimports=['flask', 'flask.json', 'fitz', 'pymupdf', 'pymupdf.layout', 'pdf2docx', 'docx', 'cv2', 'rapidocr_onnxruntime', 'scipy', 'scipy.ndimage', 'lxml', 'lxml.etree', 'lxml.html', 'pypdf', 'win32com.client', 'pythoncom', 'PIL', 'PIL.Image', 'ocr_engine'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=['PyQt5', 'PySide6', 'tkinter'],
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
    name='docflow_server',
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

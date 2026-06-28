# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for DocFlow server"""

block_cipher = None

a = Analysis(
    ['../main-sub-agent-system/tools/DocFlow/server.py'],
    pathex=['../main-sub-agent-system/tools/DocFlow'],
    binaries=[],
    datas=[
        ('../main-sub-agent-system/tools/DocFlow/ocr_engine.py', '.'),
        ('../main-sub-agent-system/tools/DocFlow/index.html', '.'),
        ('../main-sub-agent-system/tools/DocFlow/styles.css', '.'),
        ('../main-sub-agent-system/tools/DocFlow/app.js', '.'),
        ('../main-sub-agent-system/tools/DocFlow/icon.png', '.'),
        ('../main-sub-agent-system/tools/DocFlow/icon01.png', '.'),
        ('../main-sub-agent-system/tools/DocFlow/pdf.min.js', '.'),
        ('../main-sub-agent-system/tools/DocFlow/pdf.worker.min.js', '.'),
        ('../main-sub-agent-system/tools/DocFlow/fonts', 'fonts'),
        ('../main-sub-agent-system/tools/DocFlow/help_docs', 'help_docs'),
        ('../main-sub-agent-system/tools/DocFlow/music', 'music'),
        ('../main-sub-agent-system/tools/DocFlow/kmind-markdown-to-mindmap-0.1.0', 'kmind-markdown-to-mindmap-0.1.0'),
    ],
    hiddenimports=[
        'flask',
        'flask.json',
        'fitz',
        'pymupdf',
        'pymupdf.layout',
        'pdf2docx',
        'docx',
        'cv2',
        'rapidocr_onnxruntime',
        'scipy',
        'scipy.ndimage',
        'lxml',
        'lxml.etree',
        'lxml.html',
        'pypdf',
        'win32com',
        'win32com.client',
        'pythoncom',
        'PIL',
        'PIL.Image',
        'ocr_engine',
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
    name='docflow_server',
    debug=False,
    strip=False,
    upx=True,
    runtime_tmpdir=None,
    console=True,
)

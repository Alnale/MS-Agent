@echo off
chcp 65001 >nul 2>&1
setlocal

set TOOLS_DIR=C:\Users\asus\Desktop\MS Agent\main-sub-agent-system\tools
set PYINSTALLER=C:\Users\asus\AppData\Roaming\Python\Python313\Scripts\pyinstaller.exe

cd /d "C:\Users\asus\Desktop\MS Agent\tools_build"

echo Building DocFlow tool...
"%PYINSTALLER%" --onefile --clean ^
    --add-data "%TOOLS_DIR%\DocFlow\ocr_engine.py;." ^
    --add-data "%TOOLS_DIR%\DocFlow\index.html;." ^
    --add-data "%TOOLS_DIR%\DocFlow\styles.css;." ^
    --add-data "%TOOLS_DIR%\DocFlow\app.js;." ^
    --add-data "%TOOLS_DIR%\DocFlow\icon.png;." ^
    --add-data "%TOOLS_DIR%\DocFlow\icon01.png;." ^
    --add-data "%TOOLS_DIR%\DocFlow\pdf.min.js;." ^
    --add-data "%TOOLS_DIR%\DocFlow\pdf.worker.min.js;." ^
    --add-data "%TOOLS_DIR%\DocFlow\fonts;fonts" ^
    --add-data "%TOOLS_DIR%\DocFlow\help_docs;help_docs" ^
    --add-data "%TOOLS_DIR%\DocFlow\music;music" ^
    --add-data "%TOOLS_DIR%\DocFlow\kmind-markdown-to-mindmap-0.1.0;kmind-markdown-to-mindmap-0.1.0" ^
    --hidden-import flask ^
    --hidden-import flask.json ^
    --hidden-import fitz ^
    --hidden-import pymupdf ^
    --hidden-import pymupdf.layout ^
    --hidden-import pdf2docx ^
    --hidden-import docx ^
    --hidden-import cv2 ^
    --hidden-import rapidocr_onnxruntime ^
    --hidden-import scipy ^
    --hidden-import scipy.ndimage ^
    --hidden-import lxml ^
    --hidden-import lxml.etree ^
    --hidden-import lxml.html ^
    --hidden-import pypdf ^
    --hidden-import win32com.client ^
    --hidden-import pythoncom ^
    --hidden-import PIL ^
    --hidden-import PIL.Image ^
    --hidden-import ocr_engine ^
    --exclude-module PyQt5 ^
    --exclude-module PySide6 ^
    --exclude-module tkinter ^
    --name docflow_server ^
    "%TOOLS_DIR%\DocFlow\server.py"

if %errorlevel% neq 0 (
    echo ERROR: DocFlow build failed!
    exit /b 1
)
echo DocFlow build complete.
endlocal

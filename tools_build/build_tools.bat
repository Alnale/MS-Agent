@echo off
chcp 65001 >nul 2>&1
setlocal

echo ========================================
echo   Building Tools (PyInstaller)
echo ========================================

set TOOLS_DIR=%~dp0..\main-sub-agent-system\tools
set BUILD_DIR=%~dp0

REM Build xxt
echo.
echo [1/2] Building xxt tool...
cd /d "%BUILD_DIR%"
pyinstaller --onefile --clean ^
    --add-data "%TOOLS_DIR%\xxt\schema.json;." ^
    --add-data "%TOOLS_DIR%\xxt\description.txt;." ^
    --hidden-import playwright ^
    --hidden-import playwright.async_api ^
    --hidden-import playwright._impl ^
    --hidden-import playwright._impl._driver ^
    --name auto_answer ^
    "%TOOLS_DIR%\xxt\auto_answer.py"
if %errorlevel% neq 0 (
    echo ERROR: xxt build failed!
    exit /b 1
)
echo xxt build complete.

REM Build DocFlow
echo.
echo [2/2] Building DocFlow tool...
cd /d "%BUILD_DIR%"
pyinstaller --onefile --clean ^
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
    --hidden-import fitz ^
    --hidden-import pymupdf ^
    --hidden-import pymupdf.layout ^
    --hidden-import pdf2docx ^
    --hidden-import docx ^
    --hidden-import cv2 ^
    --hidden-import rapidocr_onnxruntime ^
    --hidden-import scipy ^
    --hidden-import lxml ^
    --hidden-import lxml.etree ^
    --hidden-import pypdf ^
    --hidden-import win32com.client ^
    --hidden-import pythoncom ^
    --hidden-import PIL ^
    --name docflow_server ^
    "%TOOLS_DIR%\DocFlow\server.py"
if %errorlevel% neq 0 (
    echo ERROR: DocFlow build failed!
    exit /b 1
)
echo DocFlow build complete.

echo.
echo ========================================
echo   Tools build complete!
echo   Output: dist\auto_answer.exe
echo           dist\docflow_server.exe
echo ========================================
endlocal

@echo off
chcp 65001 >nul 2>&1
setlocal

set TOOLS_DIR=C:\Users\asus\Desktop\MS Agent\main-sub-agent-system\tools
set PYTHON_EXE=C:\Users\asus\AppData\Local\Programs\Python\Python313\python.exe
set RELEASE_TOOLS=C:\Users\asus\Desktop\MS Agent\release\tools

echo ========================================
echo   Building DocFlow Portable Environment
echo ========================================

REM Create portable venv
echo.
echo [1/3] Creating portable Python environment...
if exist "%RELEASE_TOOLS%\DocFlow\venv" rmdir /s /q "%RELEASE_TOOLS%\DocFlow\venv"
"%PYTHON_EXE%" -m venv "%RELEASE_TOOLS%\DocFlow\venv" --clear
if %errorlevel% neq 0 (
    echo ERROR: Failed to create venv!
    exit /b 1
)

REM Install dependencies
echo.
echo [2/3] Installing dependencies...
"%RELEASE_TOOLS%\DocFlow\venv\Scripts\pip.exe" install --no-cache-dir -r "%TOOLS_DIR%\DocFlow\requirements.txt"
if %errorlevel% neq 0 (
    echo ERROR: Failed to install dependencies!
    exit /b 1
)

REM Copy DocFlow files
echo.
echo [3/3] Copying DocFlow files...
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.py" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.html" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.css" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.js" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.json" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.txt" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.db" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.png" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\*.mp3" "%RELEASE_TOOLS%\DocFlow\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\fonts" "%RELEASE_TOOLS%\DocFlow\fonts\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\help_docs" "%RELEASE_TOOLS%\DocFlow\help_docs\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\music" "%RELEASE_TOOLS%\DocFlow\music\" >nul
xcopy /E /Y /I "%TOOLS_DIR%\DocFlow\kmind-markdown-to-mindmap-0.1.0" "%RELEASE_TOOLS%\DocFlow\kmind-markdown-to-mindmap-0.1.0\" >nul

echo.
echo ========================================
echo   DocFlow portable build complete!
echo ========================================
endlocal

@echo off
chcp 65001 >nul 2>&1
setlocal

set TOOLS_DIR=C:\Users\asus\Desktop\MS Agent\main-sub-agent-system\tools
set RELEASE_TOOLS=C:\Users\asus\Desktop\MS Agent\release\tools
set EMBED_DIR=%RELEASE_TOOLS%\DocFlow\python

echo ========================================
echo   Building DocFlow with Embedded Python
echo ========================================

REM Step 1: Download embedded Python if not cached
echo.
echo [1/4] Setting up embedded Python...
if not exist "%EMBED_DIR%\python.exe" (
    if not exist "%TEMP%\python-3.11.9-embed-amd64.zip" (
        echo Downloading Python 3.11.9 embeddable...
        curl -L -o "%TEMP%\python-3.11.9-embed-amd64.zip" "https://www.python.org/ftp/python/3.11.9/python-3.11.9-embed-amd64.zip"
    )
    if not exist "%EMBED_DIR%" mkdir "%EMBED_DIR%"
    echo Extracting Python...
    powershell -command "Expand-Archive -Path '%TEMP%\python-3.11.9-embed-amd64.zip' -DestinationPath '%EMBED_DIR%' -Force"

    REM Enable site-packages by uncommenting import site
    powershell -command "(Get-Content '%EMBED_DIR%\python311._pth') -replace '#import site','import site' | Set-Content '%EMBED_DIR%\python311._pth'"

    REM Install pip
    echo Installing pip...
    curl -L -o "%EMBED_DIR%\get-pip.py" "https://bootstrap.pypa.io/get-pip.py"
    "%EMBED_DIR%\python.exe" "%EMBED_DIR%\get-pip.py" --no-warn-script-location
    del "%EMBED_DIR%\get-pip.py"
)

REM Step 2: Install dependencies
echo.
echo [2/4] Installing dependencies...
"%EMBED_DIR%\python.exe" -m pip install --no-cache-dir -r "%TOOLS_DIR%\DocFlow\requirements.txt" --no-warn-script-location
if %errorlevel% neq 0 (
    echo ERROR: Failed to install dependencies!
    exit /b 1
)

REM Step 3: Copy DocFlow files
echo.
echo [3/4] Copying DocFlow files...
if not exist "%RELEASE_TOOLS%\DocFlow" mkdir "%RELEASE_TOOLS%\DocFlow"
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

REM Step 4: Copy embedded Python for xxt too
echo.
echo [4/4] Setting up xxt Python...
if not exist "%RELEASE_TOOLS%\xxt\python" (
    xcopy /E /Y /I "%EMBED_DIR%" "%RELEASE_TOOLS%\xxt\python\" >nul
)

echo.
echo ========================================
echo   Embedded Python build complete!
echo ========================================
echo.
echo Release tools directory:
echo   %RELEASE_TOOLS%\DocFlow\python\python.exe
echo   %RELEASE_TOOLS%\xxt\python\python.exe
echo.
endlocal

@echo off
chcp 65001 >nul 2>&1
setlocal

echo ========================================
echo   Agent Teams - One-Click Build Script
echo ========================================

REM Step 1: Build frontend
echo.
echo [1/4] Building frontend...
cd /d "%~dp0frontend"
call npm run build
if %errorlevel% neq 0 (
    echo ERROR: Frontend build failed!
    exit /b 1
)
echo Frontend build complete.

REM Step 2: Build backend (release mode)
echo.
echo [2/4] Building backend (release mode)...
cd /d "%~dp0main-sub-agent-system"
cargo build --release
if %errorlevel% neq 0 (
    echo ERROR: Backend build failed!
    exit /b 1
)
echo Backend build complete.

REM Step 3: Copy to release folder
echo.
echo [3/4] Packaging release...
set RELEASE_DIR=%~dp0release
set PROJECT_DIR=%~dp0main-sub-agent-system

REM Kill running instance if exists
taskkill /IM agent-server.exe /F >nul 2>&1
timeout /t 2 /nobreak >nul 2>&1

if exist "%RELEASE_DIR%\tools" rmdir /s /q "%RELEASE_DIR%\tools"
if not exist "%RELEASE_DIR%" mkdir "%RELEASE_DIR%"

copy /y "%PROJECT_DIR%\target\release\agent-server.exe" "%RELEASE_DIR%\"
copy /y "%PROJECT_DIR%\config.json" "%RELEASE_DIR%\"
if exist "%PROJECT_DIR%\.env" copy /y "%PROJECT_DIR%\.env" "%RELEASE_DIR%\"
copy /y "%~dp0start.bat" "%RELEASE_DIR%\"

REM Copy tools directory (exclude cache, temp, output, python venv)
echo Copying tools...
robocopy "%PROJECT_DIR%\tools" "%RELEASE_DIR%\tools" /E /XD __pycache__ output temp uploads python /NFL /NDL /NJH /NJS /NC /NS /NP >nul 2>&1

REM Step 4: Build embedded Python for tools
echo.
echo [4/4] Building embedded Python for tools...
call "%~dp0tools_build\build_docflow_embedded.bat"
if %errorlevel% neq 0 (
    echo WARNING: Embedded Python build failed, tools may not work without Python installed.
)

echo.
echo ========================================
echo   Build complete!
echo   Output: %RELEASE_DIR%\agent-server.exe
echo ========================================
echo.
echo Release contents:
echo   agent-server.exe  - Frontend + Backend server
echo   config.json       - System configuration
echo   .env              - API keys
echo   tools/            - Tool scripts + embedded Python
echo.
echo To run: double-click start.bat in the release folder
echo.
endlocal

@echo off
chcp 65001 >nul 2>&1
setlocal

set TOOLS_DIR=C:\Users\asus\Desktop\MS Agent\main-sub-agent-system\tools
set PYINSTALLER=C:\Users\asus\AppData\Roaming\Python\Python313\Scripts\pyinstaller.exe

cd /d "C:\Users\asus\Desktop\MS Agent\tools_build"

echo Building xxt tool...
"%PYINSTALLER%" --onefile --clean ^
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
endlocal

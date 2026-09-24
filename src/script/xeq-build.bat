rem xeq builder — cmd.exe delegates to PowerShell, which reads this same file.
set "xeq_ps=%TEMP%\xeq_%RANDOM%%RANDOM%.ps1"
copy /y "%~f0" "%xeq_ps%" >nul
powershell -NoProfile -ExecutionPolicy Bypass -File "%xeq_ps%" %*
set "xeq_rc=%errorlevel%"
del "%xeq_ps%" >nul 2>&1
exit /b %xeq_rc%

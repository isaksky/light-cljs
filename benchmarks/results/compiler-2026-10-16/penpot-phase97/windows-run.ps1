param([string]$Run)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$env:PATH = "$env:USERPROFILE\scoop\apps\msys2\current\ucrt64\bin;$env:PATH"
Set-Location -LiteralPath "$Run\i"
$code = 1
function Native([string]$Exe, [string[]]$Arguments) {
  $options = @{
    FilePath = $Exe
    RedirectStandardOutput = "$Run\o\native-out.log"
    RedirectStandardError = "$Run\o\native-err.log"
    PassThru = $true
  }
  if ($Arguments.Length -gt 0) { $options.ArgumentList = $Arguments }
  $process = Start-Process @options
  $null = $process.Handle
  $process.WaitForExit()
  $nativeCode = $process.ExitCode
  Get-Content -LiteralPath "$Run\o\native-out.log","$Run\o\native-err.log" -Encoding UTF8 |
    Add-Content -LiteralPath "$Run\o\probe.log" -Encoding UTF8
  if ($nativeCode -ne 0) { throw "Native command failed ($nativeCode): $Exe" }
}
try {
  Native 'g++' @('--version')
  Native 'g++' @('-O2','-std=c++20','-Wall','-Wextra','-Werror','-DV8_COMPRESS_POINTERS','-Iinclude','-ffunction-sections','-fdata-sections','-c','v8_stack.cc','-o','shim.o')
  Native 'g++' @('-O2','-std=c++20','-Wall','-Wextra','-Werror','stack-probe.cc','v8_thread_stack.cc','-o','stack-probe.exe')
  Native '.\stack-probe.exe' @()
  $code = 0
} catch {
  $_ | Out-String | Add-Content -LiteralPath "$Run\o\probe.log" -Encoding UTF8
} finally {
  Set-Content -LiteralPath "$Run\o\exit-code.txt" -Value $code -Encoding ASCII
}
exit $code

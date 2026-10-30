$ErrorActionPreference = 'Stop'
$taskTools = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\tools'))
if (Test-Path -LiteralPath (Join-Path $taskTools 'rust\cargo\bin\cargo.exe')) {
    $env:CARGO_HOME = Join-Path $taskTools 'rust\cargo'
    $env:RUSTUP_HOME = Join-Path $taskTools 'rust\rustup'
    $env:PATH = "$env:CARGO_HOME\bin;$env:PATH"
}
if (Test-Path -LiteralPath (Join-Path $taskTools 'node24')) {
    $taskNode = Get-ChildItem -LiteralPath (Join-Path $taskTools 'node24') -Directory | Select-Object -First 1
    $env:PATH = "$($taskNode.FullName);$env:PATH"
}
$taskMsvc = Join-Path $taskTools 'rust\msvc'
if ((Test-Path -LiteralPath "$taskMsvc\VC\Tools\MSVC") -and (Test-Path -LiteralPath "$taskMsvc\Windows Kits\10\Lib")) {
    $taskVc = Get-ChildItem -LiteralPath "$taskMsvc\VC\Tools\MSVC" -Directory | Sort-Object Name -Descending | Select-Object -First 1
    $taskSdk = Get-ChildItem -LiteralPath "$taskMsvc\Windows Kits\10\Lib" -Directory | Sort-Object Name -Descending | Select-Object -First 1
    if ($taskVc -and $taskSdk) {
        $env:PATH = "$($taskVc.FullName)\bin\Hostx64\x64;$taskMsvc\Windows Kits\10\bin\$($taskSdk.Name)\x64;$env:PATH"
        $env:INCLUDE = "$($taskVc.FullName)\include;$taskMsvc\Windows Kits\10\Include\$($taskSdk.Name)\ucrt;$taskMsvc\Windows Kits\10\Include\$($taskSdk.Name)\shared;$taskMsvc\Windows Kits\10\Include\$($taskSdk.Name)\um"
        $env:LIB = "$($taskVc.FullName)\lib\x64;$($taskSdk.FullName)\ucrt\x64;$($taskSdk.FullName)\um\x64"
    }
}

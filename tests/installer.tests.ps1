$ErrorActionPreference='Stop'
$taskSource=Join-Path $PSScriptRoot '..\src-tauri\migrate-legacy.ps1'
$taskTokens=$null; $taskErrors=$null
$taskAst=[Management.Automation.Language.Parser]::ParseFile($taskSource,[ref]$taskTokens,[ref]$taskErrors)
if($taskErrors.Count){throw 'Syntax validation failed'}
foreach($taskFunction in $taskAst.FindAll({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst]},$false)) { Invoke-Expression $taskFunction.Extent.Text }
$taskEncoding=New-Object Text.UTF8Encoding($false)
$taskFixture=Join-Path ([IO.Path]::GetTempPath()) ('propad-installer-tests-'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($taskFixture)|Out-Null
$taskInstalledExe=Join-Path $taskFixture 'App Folder\ProPadDashboard.exe'
$taskTargetExecutables=@($taskInstalledExe)
$taskPassed=0
function Assert([bool]$Condition,[string]$Name){if(!$Condition){throw $Name};$script:taskPassed++;Write-Output ('PASS '+$Name)}
try {
    Assert ($taskErrors.Count -eq 0) 'Windows PowerShell syntax'
    Assert ((Get-CommandExecutable ('"'+$taskInstalledExe+'" --background')) -eq $taskInstalledExe) 'Quoted executable with spaces'
    Assert ($null -eq (Get-CommandExecutable '')) 'Empty command is ignored'
    Assert (Test-SamePath $taskInstalledExe $taskInstalledExe.ToUpperInvariant()) 'Case-insensitive path identity'
    Assert (!(Test-SamePath $taskInstalledExe (Join-Path $taskFixture 'Other.exe'))) 'Unrelated application differs'
    Assert (Test-OwnRunValue ('"'+$taskInstalledExe+'" --background')) 'Matching startup command belongs to app'
    Assert (!(Test-OwnRunValue '"C:\unrelated.exe" --background')) 'Unrelated startup command is protected'
    $taskFile=Join-Path $taskFixture 'settings.json'
    Write-AtomicJson $taskFile @{version=1; name='控制器'}
    $taskRead=[IO.File]::ReadAllText($taskFile,$taskEncoding)|ConvertFrom-Json
    Assert ($taskRead.version -eq 1 -and $taskRead.name -eq '控制器') 'Initial atomic JSON write and Unicode'
    Write-AtomicJson $taskFile @{version=3; enabled=$true}
    $taskRead=[IO.File]::ReadAllText($taskFile,$taskEncoding)|ConvertFrom-Json
    Assert ($taskRead.version -eq 3 -and $taskRead.enabled) 'Atomic replacement preserves valid JSON'
    $taskKnown=Join-Path $taskFixture 'sha.txt';[IO.File]::WriteAllText($taskKnown,'abc',$taskEncoding)
    Assert ((Get-Sha256 $taskKnown) -eq 'BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD') 'SHA256 works without Get-FileHash module'
    Write-Output ("Installer helper tests: $taskPassed passed")
} finally {
    # Only delete the known fixture files we created, without recursive operations.
    foreach($taskName in @('settings.json','sha.txt')){$taskPath=Join-Path $taskFixture $taskName;if([IO.File]::Exists($taskPath)){[IO.File]::Delete($taskPath)}}
    [IO.Directory]::Delete($taskFixture)
}

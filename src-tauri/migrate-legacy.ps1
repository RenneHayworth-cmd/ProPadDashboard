# Windows PowerShell 5.1 compatible. Used by the current-user NSIS installer.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet('PreInstall','PostInstall','PreUninstall','PostUninstall')]
    [string]$Mode,
    [Parameter(Mandatory=$true)][string]$InstallDirectory,
    [Parameter(Mandatory=$true)][string]$StateFile,
    [string]$UpdateMode = '0'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$taskEncoding = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = $taskEncoding
$taskRunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$taskRunName = 'ProPadDashboard'
$taskLegacyDirectory = [IO.Path]::GetFullPath((Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'ProPadDashboard'))
$taskInstallDirectory = [IO.Path]::GetFullPath($InstallDirectory).TrimEnd('\')
$taskInstalledExe = Join-Path $taskInstallDirectory 'ProPadDashboard.exe'
$taskLegacyExe = Join-Path $taskLegacyDirectory 'ProPadDashboard.exe'
$taskPendingMigration = Join-Path $taskLegacyDirectory 'migration-v3-pending.json'
$taskTargetExecutables = @($taskInstalledExe, $taskLegacyExe) | Select-Object -Unique

function Test-SamePath([string]$First, [string]$Second) {
    if ([string]::IsNullOrWhiteSpace($First) -or [string]::IsNullOrWhiteSpace($Second)) { return $false }
    try { return [string]::Equals([IO.Path]::GetFullPath($First), [IO.Path]::GetFullPath($Second), [StringComparison]::OrdinalIgnoreCase) }
    catch { return $false }
}

function Get-Sha256([string]$Path) {
    # Do not depend on Get-FileHash's script module: NSIS may inherit a PSModulePath
    # from PowerShell 7 or another launcher while executing Windows PowerShell 5.1.
    $taskAlgorithm = [Security.Cryptography.SHA256]::Create()
    $taskStream = [IO.File]::OpenRead($Path)
    try { return ([BitConverter]::ToString($taskAlgorithm.ComputeHash($taskStream))).Replace('-', '') }
    finally { $taskStream.Dispose(); $taskAlgorithm.Dispose() }
}

function Get-RunValue {
    if (!(Test-Path -LiteralPath $taskRunKey)) { return $null }
    $taskProperty = Get-ItemProperty -LiteralPath $taskRunKey -Name $taskRunName -ErrorAction SilentlyContinue
    if ($null -eq $taskProperty) { return $null }
    return [string]$taskProperty.$taskRunName
}

function Get-CommandExecutable([string]$Command) {
    if ([string]::IsNullOrWhiteSpace($Command)) { return $null }
    if ($Command -match '^\s*"([^"\r\n]+)"(?:\s|$)') { return [Environment]::ExpandEnvironmentVariables($Matches[1]) }
    if ($Command -match '^\s*(.+?\.exe)(?:\s|$)') { return [Environment]::ExpandEnvironmentVariables($Matches[1]) }
    return $null
}

function Test-OwnRunValue([string]$Value) {
    $taskExecutable = Get-CommandExecutable $Value
    foreach ($taskTarget in $taskTargetExecutables) { if (Test-SamePath $taskExecutable $taskTarget) { return $true } }
    return $false
}

function Write-AtomicJson([string]$Path, $Value) {
    $taskParent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Path))
    [IO.Directory]::CreateDirectory($taskParent) | Out-Null
    $taskTemporary = Join-Path $taskParent ('.propad-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllText($taskTemporary, ($Value | ConvertTo-Json -Depth 8), $taskEncoding)
        if (Test-Path -LiteralPath $Path) { [IO.File]::Replace($taskTemporary, $Path, [NullString]::Value) }
        else { [IO.File]::Move($taskTemporary, $Path) }
    }
    finally { if (Test-Path -LiteralPath $taskTemporary) { Remove-Item -LiteralPath $taskTemporary -Force } }
}

function Send-GracefulExit([string]$PipeName) {
    # Connect, write and reply share one two-second budget. A missing pipe is normal.
    $taskWatch = [Diagnostics.Stopwatch]::StartNew()
    $taskPipe = $null; $taskReader = $null; $taskWriter = $null
    try {
        $taskPipe = New-Object IO.Pipes.NamedPipeClientStream('.', $PipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
        $taskPipe.Connect(350)
        $taskWriter = New-Object IO.StreamWriter($taskPipe, $taskEncoding, 1024, $true)
        $taskWriter.NewLine = "`n"; $taskWriter.AutoFlush = $true
        $taskReader = New-Object IO.StreamReader($taskPipe, $taskEncoding, $false, 1024, $true)
        $taskWriting = $taskWriter.WriteLineAsync('exit')
        $taskRemaining = [Math]::Max(1, 2000 - [int]$taskWatch.ElapsedMilliseconds)
        if (!$taskWriting.Wait($taskRemaining)) { return }
        $taskReply = $taskReader.ReadLineAsync()
        $taskRemaining = [Math]::Max(1, 2000 - [int]$taskWatch.ElapsedMilliseconds)
        if ($taskReply.Wait($taskRemaining)) { $null = $taskReply.Result }
    }
    catch [TimeoutException] { }
    catch [IO.IOException] { }
    catch [AggregateException] { }
    finally {
        if ($null -ne $taskReader) { $taskReader.Dispose() }
        if ($null -ne $taskWriter) { $taskWriter.Dispose() }
        if ($null -ne $taskPipe) { $taskPipe.Dispose() }
    }
}

function Get-RunningTargetProcesses {
    foreach ($taskProcess in @(Get-Process -Name 'ProPadDashboard' -ErrorAction SilentlyContinue)) {
        try {
            $taskPath = $taskProcess.Path
            foreach ($taskTarget in $taskTargetExecutables) {
                if (Test-SamePath $taskPath $taskTarget) { $taskProcess; break }
            }
        }
        catch [System.ComponentModel.Win32Exception] { }
    }
}

function Stop-ApplicationGracefully {
    $taskSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    Send-GracefulExit ('ProPadDashboard.' + $taskSid)
    Send-GracefulExit ('ProPadDashboard.Tauri.' + $taskSid)
    $taskWatch = [Diagnostics.Stopwatch]::StartNew()
    do {
        $taskRunning = @(Get-RunningTargetProcesses)
        if ($taskRunning.Count -eq 0) { return }
        Start-Sleep -Milliseconds 80
    } while ($taskWatch.ElapsedMilliseconds -lt 2000)
    throw 'The installed dashboard did not exit within two seconds. Exit it from the tray and run setup again. No process was forcefully terminated.'
}

function Backup-LegacyBaseline([bool]$StartupEnabled, [string]$RunCommand) {
    $taskSource = $null
    foreach ($taskCandidate in @($taskLegacyExe, $taskInstalledExe)) {
        if (Test-Path -LiteralPath $taskCandidate -PathType Leaf) {
            $taskVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($taskCandidate)
            if ($taskVersion.FileMajorPart -lt 3) { $taskSource = $taskCandidate; break }
        }
    }
    if ($null -eq $taskSource) { return $null }
    $taskBackup = Join-Path $taskLegacyDirectory 'backup-v2'
    $taskComplete = Join-Path $taskBackup 'baseline-complete.json'
    if (Test-Path -LiteralPath $taskComplete -PathType Leaf) { return $taskBackup }
    # An incomplete previous attempt is kept intact; never overwrite a baseline.
    if (Test-Path -LiteralPath $taskBackup) { $taskBackup += '-' + [Guid]::NewGuid().ToString('N') }
    [IO.Directory]::CreateDirectory($taskBackup) | Out-Null
    $taskFiles = @{}
    foreach ($taskItem in @(
        @{ Source=$taskSource; Name='ProPadDashboard.exe' },
        @{ Source=(Join-Path $taskLegacyDirectory 'settings.json'); Name='settings.json' },
        @{ Source=(Join-Path $taskLegacyDirectory 'install-info.json'); Name='install-info.json' }
    )) {
        if (Test-Path -LiteralPath $taskItem.Source -PathType Leaf) {
            $taskDestination = Join-Path $taskBackup $taskItem.Name
            [IO.File]::Copy($taskItem.Source, $taskDestination, $false)
            $taskFiles[$taskItem.Name] = Get-Sha256 $taskDestination
        }
    }
    $taskManifest = @{
        BackupVersion=1; SourceVersion=[Diagnostics.FileVersionInfo]::GetVersionInfo($taskSource).FileVersion
        CreatedAtUtc=[DateTime]::UtcNow.ToString('o'); StartupEnabled=$StartupEnabled
        OriginalRunCommand=$RunCommand; SourceExecutable=$taskSource; Sha256=$taskFiles
    }
    # Commit the completion marker last, after all file copies and hashes succeed.
    $taskMarker = Join-Path $taskBackup 'baseline-complete.json'
    $taskStream = [IO.File]::Open($taskMarker, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $taskBytes=$taskEncoding.GetBytes(($taskManifest | ConvertTo-Json -Depth 8)); $taskStream.Write($taskBytes,0,$taskBytes.Length) }
    finally { $taskStream.Dispose() }
    return $taskBackup
}

try {
    switch ($Mode) {
        'PreInstall' {
            $taskOldRun = Get-RunValue
            if ($null -ne $taskOldRun -and !(Test-OwnRunValue $taskOldRun)) {
                throw 'The ProPadDashboard startup name belongs to another command. Setup left it unchanged; resolve the name collision before installing.'
            }
            $taskExisting = (Test-Path -LiteralPath $taskInstalledExe) -or (Test-Path -LiteralPath $taskLegacyExe) -or (Test-Path -LiteralPath (Join-Path $taskLegacyDirectory 'settings.json'))
            $taskStartupEnabled = if ($taskExisting) { $null -ne $taskOldRun } else { $true }
            # Persist the original preference before any backup or installer step can
            # fail. A retry may first run a previous NSIS uninstaller that removes Run.
            if (Test-Path -LiteralPath $taskPendingMigration -PathType Leaf) {
                try {
                    $taskPending = Get-Content -LiteralPath $taskPendingMigration -Raw | ConvertFrom-Json
                    if (Test-SamePath $taskPending.InstallDirectory $taskInstallDirectory) {
                        $taskStartupEnabled = [bool]$taskPending.StartupEnabled
                        $taskOldRun = [string]$taskPending.OriginalRunCommand
                    }
                } catch { }
            }
            Write-AtomicJson $taskPendingMigration @{
                InstallDirectory=$taskInstallDirectory; StartupEnabled=[bool]$taskStartupEnabled
                OriginalRunCommand=$taskOldRun; CreatedAtUtc=[DateTime]::UtcNow.ToString('o')
            }
            Stop-ApplicationGracefully
            $taskBackup = Backup-LegacyBaseline $taskStartupEnabled $taskOldRun
            Write-AtomicJson $StateFile @{
                SchemaVersion=1; StartupEnabled=[bool]$taskStartupEnabled; OriginalRunCommand=$taskOldRun
                InstallDirectory=$taskInstallDirectory; BackupDirectory=$taskBackup; CreatedAtUtc=[DateTime]::UtcNow.ToString('o')
            }
            Write-Output 'Migration preparation completed. Existing settings and startup preference were preserved.'
        }
        'PostInstall' {
            if (!(Test-Path -LiteralPath $StateFile -PathType Leaf)) { throw 'Migration state is missing; startup was not changed.' }
            $taskState = Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json
            if (!(Test-SamePath $taskState.InstallDirectory $taskInstallDirectory)) { throw 'Migration state does not match the installation directory.' }
            if (!(Test-Path -LiteralPath $taskInstalledExe -PathType Leaf)) { throw 'Installed application is missing; startup was not changed.' }
            $taskCurrentRun = Get-RunValue
            if ($null -ne $taskCurrentRun -and !(Test-OwnRunValue $taskCurrentRun)) { throw 'Startup name changed during installation; the unrelated value was left unchanged.' }
            if ($taskState.StartupEnabled) {
                if (!(Test-Path -LiteralPath $taskRunKey)) { New-Item -Path $taskRunKey -Force | Out-Null }
                New-ItemProperty -LiteralPath $taskRunKey -Name $taskRunName -Value ('"' + $taskInstalledExe + '" --background') -PropertyType String -Force | Out-Null
            }
            elseif ($null -ne $taskCurrentRun -and (Test-OwnRunValue $taskCurrentRun)) { Remove-ItemProperty -LiteralPath $taskRunKey -Name $taskRunName }
            Write-AtomicJson (Join-Path $taskLegacyDirectory 'install-info.json') @{
                Version='3.0.0'; InstalledAtUtc=[DateTime]::UtcNow.ToString('o')
                Executable=$taskInstalledExe; StartupEnabled=[bool]$taskState.StartupEnabled
                BackupDirectory=$taskState.BackupDirectory
            }
            if (Test-Path -LiteralPath $taskPendingMigration) { Remove-Item -LiteralPath $taskPendingMigration -Force }
            Write-Output 'Migration completed. Login startup preference was restored.'
        }
        'PreUninstall' {
            Stop-ApplicationGracefully
            $taskOldRun = Get-RunValue
            $taskUnrelated = if ($null -ne $taskOldRun -and !(Test-SamePath (Get-CommandExecutable $taskOldRun) $taskInstalledExe)) { $taskOldRun } else { $null }
            Write-AtomicJson $StateFile @{ SchemaVersion=1; UnrelatedRunCommand=$taskUnrelated; InstallDirectory=$taskInstallDirectory }
            if ($UpdateMode -ne '1' -and $null -ne $taskOldRun -and (Test-SamePath (Get-CommandExecutable $taskOldRun) $taskInstalledExe)) {
                Remove-ItemProperty -LiteralPath $taskRunKey -Name $taskRunName
            }
            Write-Output 'Application exited. Only its matching login entry was removed; user settings and backups are retained.'
        }
        'PostUninstall' {
            if (Test-Path -LiteralPath $StateFile -PathType Leaf) {
                $taskState = Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json
                if (!(Test-SamePath $taskState.InstallDirectory $taskInstallDirectory)) { throw 'Uninstall state does not match the installation directory.' }
                if ($null -ne $taskState.UnrelatedRunCommand -and $null -eq (Get-RunValue)) {
                    if (!(Test-Path -LiteralPath $taskRunKey)) { New-Item -Path $taskRunKey -Force | Out-Null }
                    New-ItemProperty -LiteralPath $taskRunKey -Name $taskRunName -Value ([string]$taskState.UnrelatedRunCommand) -PropertyType String -Force | Out-Null
                }
            }
            Write-Output 'Uninstall startup check completed. Settings and backup files were retained.'
        }
    }
    exit 0
}
catch { [Console]::Error.WriteLine($_.Exception.Message); exit 2 }

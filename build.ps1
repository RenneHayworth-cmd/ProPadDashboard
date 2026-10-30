param([switch]$SkipTests)
$ErrorActionPreference='Stop'
Push-Location $PSScriptRoot
try {
    . .\buildenv.ps1
    if (!(Get-Command cargo -ErrorAction SilentlyContinue)) { throw '请先安装 Rust MSVC 工具链、C++ 构建工具和 Windows SDK。' }
    if (!(Get-Command npm.cmd -ErrorAction SilentlyContinue)) { throw '请先安装 Node.js 24 LTS。' }
    & npm.cmd ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw '前端依赖安装失败' }
    if (!$SkipTests) {
        & npm.cmd test
        if ($LASTEXITCODE -ne 0) { throw '前端检查失败' }
        & cargo test --workspace --locked
        if ($LASTEXITCODE -ne 0) { throw 'Rust 检查失败' }
    }
    & npm.cmd run tauri build -- --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw '安装包构建失败' }
} finally { Pop-Location }

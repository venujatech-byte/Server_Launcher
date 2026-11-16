$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo check --locked --tests --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "Windows cargo check failed ($LASTEXITCODE)" }
} finally {
    Pop-Location
}

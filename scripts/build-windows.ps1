$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
corepack yarn install --immutable
if ($LASTEXITCODE -ne 0) { throw "yarn install failed" }
corepack yarn test
if ($LASTEXITCODE -ne 0) { throw "UI tests failed" }
cargo test --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw "Core tests failed" }
corepack yarn tauri build --features desktop
if ($LASTEXITCODE -ne 0) { throw "Native build failed" }
Write-Host "NSIS installer: src-tauri/target/release/bundle/nsis/"

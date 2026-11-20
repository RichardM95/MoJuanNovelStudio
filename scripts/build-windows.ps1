$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
npm ci
if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
npm test
if ($LASTEXITCODE -ne 0) { throw "UI tests failed" }
cargo test --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw "Core tests failed" }
npm run tauri -- build --features desktop
if ($LASTEXITCODE -ne 0) { throw "Native build failed" }
Write-Host "NSIS installer: src-tauri/target/release/bundle/nsis/"

param(
    [switch]$RequireGpu
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$testTarget = Join-Path ([System.IO.Path]::GetTempPath()) 'qu-gpu-target'
$engineTarget = Join-Path ([System.IO.Path]::GetTempPath()) 'qu-engine-target'
$wasmTarget = Join-Path ([System.IO.Path]::GetTempPath()) 'qu-wasm-target'

Push-Location -LiteralPath $repoRoot
try {
    if ($RequireGpu) {
        $env:QU_REQUIRE_GPU = '1'
    }

    cargo test --target-dir $testTarget --release -p qu-core -p qu-gpu -p qu-wasm -- --nocapture
    if ($LASTEXITCODE -ne 0) { throw 'Core/GPU/WASM tests failed' }

    cargo check --target-dir $wasmTarget --target wasm32-unknown-unknown -p qu-core -p qu-wasm
    if ($LASTEXITCODE -ne 0) { throw 'Browser WASM compilation failed' }

    cargo test --target-dir $engineTarget --manifest-path '.\engine\Cargo.toml' --workspace
    if ($LASTEXITCODE -ne 0) { throw 'Reference engine tests failed' }

    node --test tests/*.test.js
    if ($LASTEXITCODE -ne 0) { throw 'Browser/documentation tests failed' }
}
finally {
    Remove-Item Env:\QU_REQUIRE_GPU -ErrorAction SilentlyContinue
    Pop-Location
}

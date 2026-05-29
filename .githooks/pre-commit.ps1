$ErrorActionPreference = "Stop"

Write-Host "=== pre-commit: cargo build ==="
$env:RUSTFLAGS = "-D warnings"
cargo build --all-targets
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "=== pre-commit: cargo clippy ==="
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "=== pre-commit: cargo test ==="
cargo test
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "=== pre-commit: checking file lengths ==="
$maxLines = 500
$files = git diff --cached --name-only --diff-filter=ACM -- '*.rs'
foreach ($f in $files) {
  if ($f -and (Test-Path $f)) {
    $count = (Get-Content $f | Where-Object { $_.Trim() -ne "" }).Count
    if ($count -gt $maxLines) {
      Write-Host "ERROR: $f has $count non-empty lines (max $maxLines)"
      exit 1
    }
  }
}

Write-Host "=== pre-commit: all checks passed ==="

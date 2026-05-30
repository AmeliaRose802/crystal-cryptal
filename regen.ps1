$ErrorActionPreference = "Stop"
foreach ($f in Get-ChildItem examples\*.cry) {
  $name = $f.BaseName
  cargo run --quiet -- $f.FullName -o "examples\out_$name" 2>$null
}
cargo run --quiet -- examples/do_NOT_COMMIT_INTERNAL -o examples/out_internal_multi 2>$null
Copy-Item examples\out_SHA2\* examples\out_SHA2_new\ -Recurse -Force -ErrorAction SilentlyContinue
$sets = @("out_SDEP","out_chacha20","out_Blake2b","out_trivium","out_ZUC","out_SHA2","out_SHA2_new","out_HMAC","out_internal_multi","out_internal_single")
foreach ($d in $sets) {
  $p = "examples\$d"
  if (Test-Path $p) {
    $r = .\check_links.ps1 -Root $p | Select-String "Total broken"
    Write-Output "$d : $r"
  }
}

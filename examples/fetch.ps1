#!/usr/bin/env pwsh
# Downloads third-party Cryptol specs for testing pretty-specs.
# Source: https://github.com/GaloisInc/cryptol-specs (BSD-3-Clause)

$ErrorActionPreference = "Stop"
$dir = $PSScriptRoot

$base = "https://raw.githubusercontent.com/GaloisInc/cryptol-specs/master"
$specs = @{
    "chacha20.cry" = "$base/Primitive/Symmetric/Cipher/Stream/chacha20.cry"
    "SHA2.cry"     = "$base/Primitive/Keyless/Hash/SHA2/Specification.cry"
    "HMAC.cry"     = "$base/Primitive/Symmetric/MAC/HMAC/Specification.cry"
    "trivium.cry"  = "$base/Primitive/Symmetric/Cipher/Stream/trivium.cry"
    "Blake2b.cry"  = "$base/Primitive/Keyless/Hash/Blake2b.cry"
    "ZUC.cry"      = "$base/Primitive/Symmetric/Cipher/Stream/ZUC.cry"
}

$count = 0
foreach ($entry in $specs.GetEnumerator()) {
    $out = Join-Path $dir $entry.Key
    if (Test-Path $out) {
        Write-Host "  skip $($entry.Key) (already exists)"
        continue
    }
    Write-Host "  fetch $($entry.Key)"
    Invoke-WebRequest -Uri $entry.Value -OutFile $out -UseBasicParsing
    $count++
}

Write-Host "Done — $count file(s) downloaded, $($specs.Count - $count) skipped."

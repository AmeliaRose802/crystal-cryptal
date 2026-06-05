<#
.SYNOPSIS
    End-to-end pipeline: docs generation + SAW verification + proof badges.

.DESCRIPTION
    Runs the full pretty-specs / saw-spec-gen pipeline for a Cryptol spec file:

    1. Emit function inventory (--emit-function-list) from the .cry spec.
    2. Call saw-spec-gen gen-verify for each function in the inventory.
    3. Collect saw-spec-gen result.json files (--adapt-saw-results).
    4. Re-render docs with proof status badges (--proof-status).

    Any step can be skipped with the corresponding -Skip* switch.

.PARAMETER Spec
    Path to the .cry specification file (required).

.PARAMETER Output
    Directory for the final rendered documentation. Default: ./docs

.PARAMETER Impl
    Path to the C++ or Rust implementation file passed to saw-spec-gen.
    If omitted, Step 2 (SAW verification) is skipped.

.PARAMETER ImplLang
    Implementation language: "cpp" or "rust". Default: "cpp".

.PARAMETER SawSpecGenRoot
    Path to the saw-spec-gen repository root, used to locate verify.ps1
    (C++) and verify-rust.ps1 (Rust). Defaults to the parent directory of
    the saw-spec-gen binary when it can be resolved, or the value of the
    SAW_SPEC_GEN_ROOT environment variable.

.PARAMETER SawSpecGen
    Path (or name on PATH) of the saw-spec-gen binary. Default: "saw-spec-gen".
    Also used to locate SawSpecGenRoot when SawSpecGenRoot is not set explicitly.

.PARAMETER PrettySpecs
    Path (or name on PATH) of the pretty-specs binary. Default: "pretty-specs".
    Set to "cargo run --" to run from source.

.PARAMETER VerifyOutput
    Directory where saw-spec-gen writes out_*/result.json files.
    Default: ./verify_out

.PARAMETER ManifestOutput
    Path for the unified proof_manifest.json. Default: ./proof_manifest.json

.PARAMETER CxxIncludeDirs
    Extra include directories passed to clang via -I (C++ only). Maps to
    verify.ps1's -IncludeDirs. Pass multiple values as a comma-separated list:
    -CxxIncludeDirs cpp\include,cpp\third_party.

.PARAMETER CxxStandard
    C++ standard passed to clang as -std=<value> (e.g. "c++20"). Maps to
    verify.ps1's -CxxStandard.

.PARAMETER ExtraClangFlags
    Additional flags forwarded verbatim to every clang invocation (C++ only).
    Maps to verify.ps1's -ClangFlags. Example: -ExtraClangFlags '-fexceptions','-fno-inline'.

.PARAMETER Logo
    Path to a logo image (svg/png) to copy into <Output>/images/ during
    Step 0 and Step 4. When combined with -Docfx, pretty-specs prints the
    matching `_appLogoPath` globalMetadata snippet to stderr. Source path
    should live OUTSIDE the -Output directory so it survives doc regen.

.PARAMETER Favicon
    Path to a favicon (.ico/.png/.svg) to copy into <Output>/images/ during
    Step 0 and Step 4. When combined with -Docfx, pretty-specs prints the
    matching `_appFaviconPath` globalMetadata snippet to stderr.

.PARAMETER ExtraDocs
    One or more directories of additional Markdown (and supporting) files
    to include verbatim in the generated site. Each directory is copied to
    <Output>/<basename>/ preserving structure during Step 0 and Step 4.
    In -Docfx mode an entry is appended to the top-level toc.yml so the
    pages appear in the navbar (prefers a toc.yml at the dir root, falling
    back to index.md). Items may use the optional DIR:Display Name syntax
    to override the toc label. Example: -ExtraDocs docs_extra,guides:Guides.

.PARAMETER SkipVerify
    Skip saw-spec-gen verification (Steps 1–2). Still adapts existing results
    if VerifyOutput already contains result.json files.

.PARAMETER SkipAdapt
    Skip --adapt-saw-results (Step 3). Use an existing proof_manifest.json.

.PARAMETER SkipDocs
    Skip final doc rendering (Step 4). Useful when you only want the manifest.

.EXAMPLE
    # Full pipeline from source:
    .\pipeline.ps1 -Spec examples/SDEP.cry -Impl ../myproject/sdep.cpp `
        -PrettySpecs "cargo run --" -Output docs/

.EXAMPLE
    # Docs only (no verification):
    .\pipeline.ps1 -Spec examples/SDEP.cry -SkipVerify -SkipAdapt -Output docs/

.EXAMPLE
    # Adapt existing saw-spec-gen results and regenerate docs:
    .\pipeline.ps1 -Spec examples/SDEP.cry -SkipVerify -Output docs/
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$Spec,

    [string]$Output = "docs",
    [string]$Impl = "",
    [ValidateSet("cpp", "rust")]
    [string]$ImplLang = "cpp",
    [string]$SawSpecGenRoot = "",
    [string]$SawSpecGen = "saw-spec-gen",
    [string]$PrettySpecs = "pretty-specs",
    [string]$VerifyOutput = "verify_out",
    [string]$ManifestOutput = "proof_manifest.json",
    [string[]]$CxxIncludeDirs = @(),
    [string]$CxxStandard = "",
    [string[]]$ExtraClangFlags = @(),
    [string]$Logo = "",
    [string]$Favicon = "",
    [string[]]$ExtraDocs = @(),
    [switch]$SkipVerify,
    [switch]$SkipAdapt,
    [switch]$SkipDocs,
    [switch]$Docfx,
    # Soft-exit on Cryptol-only helpers (no matching impl symbol)
    # instead of erroring out. Default: on, because pretty-specs
    # naturally generates one job per top-level Cryptol def and most
    # spec modules contain private helpers (packPad, derivePin, etc.)
    # with no implementation analog. Use -SpecOnlyOnMissing:$false to
    # restore the old strict-error behaviour.
    [switch]$SpecOnlyOnMissing = $true
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# Auto-enable -Docfx when a docfx.json sits in the current working directory
# (typical for repos that host a DocFX site). Without --docfx, pretty-specs
# does not emit toc.yml, which breaks the DocFX navbar.
if (-not $Docfx -and (Test-Path (Join-Path (Get-Location) 'docfx.json'))) {
    Write-Host "[pipeline] docfx.json detected — enabling -Docfx automatically." -ForegroundColor DarkGray
    $Docfx = $true
}

# ── Helper ───────────────────────────────────────────────────────────────────

function Invoke-PrettySpecs {
    param([string[]]$ArgsList)
    if ($PrettySpecs -eq "cargo run --") {
        $cmd = "cargo"
        $allArgs = @("run", "--") + $ArgsList
    } else {
        $cmd = $PrettySpecs
        $allArgs = $ArgsList
    }
    Write-Host "  > $cmd $($allArgs -join ' ')" -ForegroundColor DarkGray
    & $cmd @allArgs
    if ($LASTEXITCODE -ne 0) {
        throw "pretty-specs exited with code $LASTEXITCODE"
    }
}

# ── Step 0: Initial doc render (without proof status) ────────────────────────

Write-Host "`n[Step 0] Initial doc render" -ForegroundColor Cyan
$step0Args = @($Spec, "-o", $Output)
if ($Docfx) { $step0Args += "--docfx" }
if ($Logo    -ne "") { $step0Args += @("--logo",    $Logo) }
if ($Favicon -ne "") { $step0Args += @("--favicon", $Favicon) }
foreach ($d in $ExtraDocs) { if ($d -ne "") { $step0Args += @("--extra-docs", $d) } }
Invoke-PrettySpecs $step0Args

# ── Step 1: Emit function list ────────────────────────────────────────────────

if (-not $SkipVerify -and $Impl -ne "") {
    $functionListPath = Join-Path $VerifyOutput "function_list.json"
    Write-Host "`n[Step 1] Emitting function list -> $functionListPath" -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path $VerifyOutput | Out-Null
    Invoke-PrettySpecs @($Spec, "--emit-function-list", "-o", $functionListPath)

    # ── Resolve saw-spec-gen repo root for verify.ps1 / verify-rust.ps1 ──────

    # Priority: explicit -SawSpecGenRoot > SAW_SPEC_GEN_ROOT env var >
    # directory two levels above the resolved saw-spec-gen binary.
    if ($SawSpecGenRoot -eq "") {
        if ($env:SAW_SPEC_GEN_ROOT -ne $null -and $env:SAW_SPEC_GEN_ROOT -ne "") {
            $SawSpecGenRoot = $env:SAW_SPEC_GEN_ROOT
        } else {
            # Try to resolve from binary: <repo>/target/release/saw-spec-gen.exe
            # → <repo>/target/release/ → <repo>/target/ → <repo>/
            $resolvedBin = Get-Command $SawSpecGen -ErrorAction SilentlyContinue
            if ($resolvedBin) {
                $binDir = Split-Path $resolvedBin.Source -Parent
                $candidate = Split-Path (Split-Path $binDir -Parent) -Parent
                if (Test-Path (Join-Path $candidate "verify.ps1")) {
                    $SawSpecGenRoot = $candidate
                }
            }
        }
    }

    if ($SawSpecGenRoot -eq "" -or -not (Test-Path (Join-Path $SawSpecGenRoot "verify.ps1"))) {
        Write-Host ""
        Write-Host "ERROR: Cannot locate saw-spec-gen's verify.ps1." -ForegroundColor Red
        Write-Host "  Set -SawSpecGenRoot to the saw-spec-gen repository root, or set" -ForegroundColor Red
        Write-Host "  the SAW_SPEC_GEN_ROOT environment variable." -ForegroundColor Red
        Write-Host "  Example: .\pipeline.ps1 ... -SawSpecGenRoot C:\path\to\saw-spec-gen" -ForegroundColor Red
        exit 1
    }

    $verifyScript = if ($ImplLang -eq "rust") {
        Join-Path $SawSpecGenRoot "verify-rust.ps1"
    } else {
        Join-Path $SawSpecGenRoot "verify.ps1"
    }

    # ── Step 2: Run saw-spec-gen per function ─────────────────────────────────

    Write-Host "`n[Step 2] Running saw-spec-gen for each function" -ForegroundColor Cyan

    $functions = Get-Content $functionListPath | ConvertFrom-Json
    $total = $functions.Count
    $passed = 0
    $failed = 0

    foreach ($fn in $functions) {
        $name    = $fn.name
        $cryptol = $fn.name   # saw-spec-gen uses the same name by default
        $outDir  = Join-Path $VerifyOutput "out_$name"
        # verify.ps1/verify-rust.ps1 recreate the output dir themselves; no pre-create needed

        Write-Host "  Verifying $name ..." -NoNewline
        try {
            # Build the hashtable-splatted arg set for the verify script.
            # All parameters — including the impl file — go into the hashtable
            # so PowerShell resolves them by name, not positionally.
            $verifyArgs = @{
                CryptolSpec = $Spec
                CryptolFn   = $cryptol
                Function    = $name
                OutputDir   = $outDir
            }
            if ($ImplLang -eq "rust") { $verifyArgs.RustFile = $Impl }
            else                      { $verifyArgs.CppFile  = $Impl }
            if ($ImplLang -eq "cpp") {
                if ($CxxIncludeDirs.Count -gt 0)  { $verifyArgs.IncludeDirs = $CxxIncludeDirs }
                if ($CxxStandard -ne "")           { $verifyArgs.CxxStandard = $CxxStandard }
                if ($ExtraClangFlags.Count -gt 0)  { $verifyArgs.ClangFlags  = $ExtraClangFlags }
            }
            if ($SpecOnlyOnMissing) { $verifyArgs.SpecOnlyOnMissing = $true }

            & $verifyScript @verifyArgs
            if ($LASTEXITCODE -eq 0) {
                Write-Host " ok" -ForegroundColor Green
                $passed++
            } else {
                Write-Host " FAILED (exit $LASTEXITCODE)" -ForegroundColor Yellow
                $failed++
                # Write a result.json so --adapt-saw-results picks up the failure
                @{
                    cryptol_fn = $name
                    status     = "error"
                    message    = "verify script exited with code $LASTEXITCODE"
                } | ConvertTo-Json | Set-Content (Join-Path $outDir "result.json")
            }
        } catch {
            Write-Host " ERROR: $_" -ForegroundColor Red
            $failed++
            New-Item -ItemType Directory -Force -Path $outDir | Out-Null
            @{
                cryptol_fn = $name
                status     = "error"
                message    = "$_"
            } | ConvertTo-Json | Set-Content (Join-Path $outDir "result.json")
        }
    }

    Write-Host "  $passed/$total passed, $failed failed" -ForegroundColor $(if ($failed -gt 0) { "Yellow" } else { "Green" })
} elseif (-not $SkipVerify) {
    Write-Host "`n[Step 1+2] Skipped (no -Impl provided — set -Impl to enable SAW verification)" -ForegroundColor DarkGray
}

# ── Step 3: Adapt saw-spec-gen results ───────────────────────────────────────

if (-not $SkipAdapt) {
    if (Test-Path $VerifyOutput) {
        Write-Host "`n[Step 3] Adapting saw-spec-gen results -> $ManifestOutput" -ForegroundColor Cyan
        Invoke-PrettySpecs @("--adapt-saw-results", $VerifyOutput, "--manifest-output", $ManifestOutput)
    } else {
        Write-Host "`n[Step 3] Skipped ($VerifyOutput not found)" -ForegroundColor DarkGray
    }
} else {
    Write-Host "`n[Step 3] Skipped (-SkipAdapt)" -ForegroundColor DarkGray
}

# ── Step 4: Final render with proof badges ────────────────────────────────────

if (-not $SkipDocs) {
    if (Test-Path $ManifestOutput) {
        Write-Host "`n[Step 4] Rendering docs with proof badges -> $Output" -ForegroundColor Cyan
        $step4Args = @($Spec, "--proof-status", $ManifestOutput, "-o", $Output)
        if ($Docfx) { $step4Args += "--docfx" }
        if ($Logo    -ne "") { $step4Args += @("--logo",    $Logo) }
        if ($Favicon -ne "") { $step4Args += @("--favicon", $Favicon) }
        foreach ($d in $ExtraDocs) { if ($d -ne "") { $step4Args += @("--extra-docs", $d) } }
        Invoke-PrettySpecs $step4Args
    } else {
        Write-Host "`n[Step 4] No proof manifest found at $ManifestOutput — docs already rendered in Step 0" -ForegroundColor DarkGray
    }
} else {
    Write-Host "`n[Step 4] Skipped (-SkipDocs)" -ForegroundColor DarkGray
}

Write-Host "`nDone. Docs -> $Output" -ForegroundColor Green

# Example Cryptol Specs

Third-party Cryptol specifications downloaded for testing `pretty-specs`
against diverse input formats. These files are **git-ignored** and not
part of this project — they are sourced from open-source repositories
under their respective licenses.

## Sources

All specs below are from [GaloisInc/cryptol-specs](https://github.com/GaloisInc/cryptol-specs)
(BSD-3-Clause license), downloaded 2026-05-29 from `master`.

| File | Path in cryptol-specs | Why it's interesting |
|------|----------------------|---------------------|
| `chacha20.cry` | `Primitive/Symmetric/Cipher/Stream/chacha20.cry` | Stream cipher; `private` block, helper fns (`map`, `iterate`), `property` tests, RFC test vectors, full `/**/` doc comments |
| `SHA2.cry` | `Primitive/Keyless/Hash/SHA2/Specification.cry` | Parameterized module (`parameter` block with type constraints), `/** */` docstrings, `` :prove `` repl directives, type-level arithmetic |
| `HMAC.cry` | `Primitive/Symmetric/MAC/HMAC/Specification.cry` | Parameterized module, higher-order `H` parameter, unicode in comments (K₀, ≤), `@see` references, `private` section |
| `trivium.cry` | `Primitive/Symmetric/Cipher/Stream/trivium.cry` | Compact stream cipher; infinite sequences (`[inf]`), register shift operations, multiple equivalent implementations, `property` search |
| `Blake2b.cry` | `Primitive/Keyless/Hash/Blake2b.cry` | Record types (`Context`), 2D lookup tables (`SIGMA_TABLE`), `updates` primitive, large hex test vectors |
| `ZUC.cry` | `Primitive/Symmetric/Cipher/Stream/ZUC.cry` | Telecom stream cipher (3GPP); S-boxes as large literal arrays, LFSR operations, `newtype`, multiple test cases |

## Re-downloading

```powershell
$base = "https://raw.githubusercontent.com/GaloisInc/cryptol-specs/master"
Invoke-WebRequest "$base/Primitive/Symmetric/Cipher/Stream/chacha20.cry" -OutFile chacha20.cry
Invoke-WebRequest "$base/Primitive/Keyless/Hash/SHA2/Specification.cry" -OutFile SHA2.cry
Invoke-WebRequest "$base/Primitive/Symmetric/MAC/HMAC/Specification.cry" -OutFile HMAC.cry
Invoke-WebRequest "$base/Primitive/Symmetric/Cipher/Stream/trivium.cry" -OutFile trivium.cry
Invoke-WebRequest "$base/Primitive/Keyless/Hash/Blake2b.cry" -OutFile Blake2b.cry
Invoke-WebRequest "$base/Primitive/Symmetric/Cipher/Stream/ZUC.cry" -OutFile ZUC.cry
```

param([string]$Root)
$broken = New-Object System.Collections.ArrayList
Get-ChildItem -Path $Root -Recurse -Filter *.md | ForEach-Object {
  $dir = $_.DirectoryName
  $text = Get-Content $_.FullName -Raw
  foreach ($m in [regex]::Matches($text, '\]\(([^)]+)\)')) {
    $link = $m.Groups[1].Value
    if ($link -match '^(https?:|#|mailto:)') { continue }
    $path = ($link -split '#')[0]
    if ($path -eq '') { continue }
    if (-not (Test-Path (Join-Path $dir $path))) {
      [void]$broken.Add([PSCustomObject]@{ From = $_.FullName.Replace("$PWD\",""); Link = $link })
    }
  }
}
Write-Output "Total broken: $($broken.Count)"
$broken | Group-Object Link | Sort-Object Count -Descending | ForEach-Object { "{0,4}  {1}" -f $_.Count, $_.Name }
Write-Output "---- sample sources ----"
$broken | Select-Object -First 15 | ForEach-Object { "$($_.From)  ->  $($_.Link)" }

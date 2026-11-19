<#
  SonicBoost - cut a new release.

      .\release.ps1 -Version 1.1.0          # bump, changelog, commit, tag, push
      .\release.ps1 -Version 1.1.0 -DryRun  # show what would change, touch nothing in git

  Pushing the tag triggers GitHub Actions, which builds the installers and
  publishes the GitHub Release. See docs/RELEASING.md.
#>
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [switch]$DryRun
)

$ErrorActionPreference = "Continue"
Set-Location $PSScriptRoot
function Fail($m) { Write-Host "`n$m" -ForegroundColor Red; exit 1 }
$utf8 = New-Object System.Text.UTF8Encoding($false)   # no BOM (JSON/TOML parsers dislike it)
function Save($path, $text) { [IO.File]::WriteAllText((Resolve-Path $path), $text, $utf8) }
# Windows PowerShell 5.1 reads files as ANSI by default, which garbles UTF-8 (e.g. "‹" -> "â€¹").
function Load($path) { [IO.File]::ReadAllText((Resolve-Path $path), $utf8) }
function ReplaceFirst($text, $pattern, $replacement) { ([regex]$pattern).Replace($text, $replacement, 1) }

if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$') { Fail "Version must look like 1.2.3 or 1.2.3-beta.1" }
$tag = "v$Version"

if (-not $DryRun) {
    $pending = git status --porcelain
    if ($pending) {
        Write-Host "These changes will be included in the $tag release commit:" -ForegroundColor White
        $pending | ForEach-Object { Write-Host "   $_" }
    }
    $branch = git rev-parse --abbrev-ref HEAD
    if ($branch -ne "main") { Fail "Switch to the main branch first (you are on '$branch')." }
    if (git tag -l $tag) { Fail "Tag $tag already exists." }
}

$current = ((Load "src-tauri\tauri.conf.json") | ConvertFrom-Json).version
Write-Host "Releasing $current -> $Version" -ForegroundColor White

# 1. versions
$conf = Load "src-tauri\tauri.conf.json"
$conf = ReplaceFirst $conf '"version"\s*:\s*"[^"]+"' ('"version": "' + $Version + '"')
$cargo = Load "src-tauri\Cargo.toml"
$cargo = ReplaceFirst $cargo '(?m)^version\s*=\s*"[^"]+"' ('version = "' + $Version + '"')
$pkg = Load "package.json"
$pkg = ReplaceFirst $pkg '"version"\s*:\s*"[^"]+"' ('"version": "' + $Version + '"')

# 2. changelog: [Unreleased] -> [Version] - date, update compare links
$date = Get-Date -Format "yyyy-MM-dd"
$log = Load "CHANGELOG.md"
if ($log -notmatch '(?m)^## \[Unreleased\]') { Fail "CHANGELOG.md has no '## [Unreleased]' section." }
$log = ReplaceFirst $log '(?m)^## \[Unreleased\][ \t]*\r?\n' "## [Unreleased]`n`n## [$Version] - $date`n"
$repo = "https://github.com/fr4gout/SonicBoost"
$log = ReplaceFirst $log '(?m)^\[Unreleased\]:.*$' "[Unreleased]: $repo/compare/$tag...HEAD`n[$Version]: $repo/compare/v$current...$tag"

if ($DryRun) {
    Write-Host "`n--- tauri.conf.json / Cargo.toml / package.json -> $Version"
    Write-Host "--- CHANGELOG.md would start with:"
    ($log -split "`n" | Select-Object -First 14) -join "`n" | Write-Host
    Write-Host "`nDry run: nothing written." -ForegroundColor Yellow
    exit 0
}

Save "src-tauri\tauri.conf.json" $conf
Save "src-tauri\Cargo.toml" $cargo
Save "package.json" $pkg
Save "CHANGELOG.md" $log

# 3. commit, tag, push
git add -A
git commit -q -m "chore(release): $tag"
if ($LASTEXITCODE -ne 0) { Fail "git commit failed." }
git tag -a $tag -m "SonicBoost $tag"
git push origin main
if ($LASTEXITCODE -ne 0) { Fail "Push of main failed." }
git push origin $tag
if ($LASTEXITCODE -ne 0) { Fail "Push of tag $tag failed." }

Write-Host "`nReleased $tag. GitHub Actions is building the installers:" -ForegroundColor Green
Write-Host "  $repo/actions"
Write-Host "  $repo/releases/tag/$tag  (ready in ~10-15 min)"

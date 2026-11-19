<#
  SonicBoost - first-time publish to GitHub (fr4gout/SonicBoost) + v1.0.0 release.

  Double-click PUBLISH.bat, or run:   .\publish.ps1
  Options:  -Remote <url>   push somewhere else
            -NoTag          push the code only, without creating the release tag

  What it does
    1. puts the CI workflow + issue templates into .github\
    2. creates the git repo (branch main) and commits everything
    3. pushes to GitHub (a GitHub sign-in window may open the first time)
    4. tags v<version> and pushes the tag -> GitHub Actions builds the
       installers and publishes the GitHub Release automatically
#>
param(
    [string]$Remote = "https://github.com/fr4gout/SonicBoost.git",
    [switch]$NoTag
)

$ErrorActionPreference = "Continue"
Set-Location $PSScriptRoot
function Step($m) { Write-Host "`n==> $m" -ForegroundColor White }
function Fail($m) { Write-Host "`n$m" -ForegroundColor Red; exit 1 }

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Fail "Git is not installed. Run:  winget install --id Git.Git -e   then reopen this window."
}

Step "Placing GitHub workflow and templates in .github\"
New-Item -ItemType Directory -Force -Path ".github\workflows", ".github\ISSUE_TEMPLATE" | Out-Null
Copy-Item "ci\build-windows.yml" ".github\workflows\build-windows.yml" -Force
Copy-Item "ci\ISSUE_TEMPLATE\*" ".github\ISSUE_TEMPLATE\" -Force
Copy-Item "ci\pull_request_template.md" ".github\pull_request_template.md" -Force

Step "Preparing the git repository"
if (-not (Test-Path ".git")) { git init | Out-Null }
if (-not (git config user.name))  { git config user.name  (Read-Host "Your name for commits") }
if (-not (git config user.email)) { git config user.email (Read-Host "Your GitHub e-mail") }
git checkout -B main 2>&1 | Out-Null
if ((git remote) -contains "origin") { git remote set-url origin $Remote } else { git remote add origin $Remote }

$version = (Get-Content "src-tauri\tauri.conf.json" -Raw | ConvertFrom-Json).version
$tag = "v$version"

Step "Committing"
git add -A
git diff --cached --quiet
if ($LASTEXITCODE -ne 0) {
    git commit -q -m "feat: SonicBoost $tag" -m "Windows sound booster with 10-band EQ, pitch shifter, Now Playing, global hotkeys and live visualizers. Rust audio engine, Tauri UI."
    if ($LASTEXITCODE -ne 0) { Fail "git commit failed." }
} else { Write-Host "Nothing new to commit." }

Step "Pushing to $Remote  (sign in to GitHub if a window opens)"
git push -u origin main
if ($LASTEXITCODE -ne 0) {
    Write-Host "GitHub already has commits (e.g. a README created on the website) - merging them in..." -ForegroundColor Yellow
    git pull origin main --allow-unrelated-histories --no-edit -X ours
    if ($LASTEXITCODE -ne 0) { Fail "Could not merge with GitHub. Resolve the conflicts, then run this script again." }
    git push -u origin main
    if ($LASTEXITCODE -ne 0) { Fail "Push failed. Check that the repo exists and you are signed in as fr4gout." }
}

if (-not $NoTag) {
    Step "Creating release tag $tag"
    if (-not (git tag -l $tag)) { git tag -a $tag -m "SonicBoost $tag" }
    git push origin $tag
    if ($LASTEXITCODE -ne 0) { Fail "Could not push tag $tag." }
}

$web = $Remote -replace '\.git$', ''
Write-Host "`nDone!" -ForegroundColor Green
Write-Host "  Build progress : $web/actions"
Write-Host "  Release (10-15 min): $web/releases/tag/$tag"
Start-Process "$web/actions"

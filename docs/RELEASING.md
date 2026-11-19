# Releasing SonicBoost

Releases are fully automated. You push a version tag, and GitHub Actions tests the code, builds both installers and publishes a **GitHub Release** with them attached.

## Versioning

SonicBoost uses [Semantic Versioning](https://semver.org/), `MAJOR.MINOR.PATCH`:

| Change | Bump | Example |
|:--|:--|:--|
| Bug fixes only | PATCH | `1.0.0 → 1.0.1` |
| New features, backwards compatible | MINOR | `1.0.1 → 1.1.0` |
| Breaking changes (settings format, behaviour) | MAJOR | `1.1.0 → 2.0.0` |
| Test builds | pre-release | `1.1.0-beta.1` (published as a **pre-release**) |

The version is stored in three files, and they must all match the tag. CI checks this and refuses to release on a mismatch:

- `src-tauri/tauri.conf.json` → `"version"` (shown in the installer and in Windows "Apps")
- `src-tauri/Cargo.toml` → `version`
- `package.json` → `"version"`

## Making a release

1. Under `## [Unreleased]` in `CHANGELOG.md`, list what changed.
2. Run the release script from the project folder:

   ```powershell
   .\release.ps1 -Version 1.1.0
   ```

   The script:
   - Checks that the working tree is clean and that you're on `main`
   - Sets the version in all three files
   - Moves the `[Unreleased]` notes into a new `## [1.1.0] - <today>` section and updates the comparison links
   - Commits `chore(release): v1.1.0`, creates the annotated tag `v1.1.0`, and pushes both

3. Watch **Actions → Build Windows installer**. After roughly 10–15 minutes, **Releases** shows:
   - `SonicBoost_1.1.0_x64-setup.exe`
   - `SonicBoost_1.1.0_x64_en-US.msi`
   - Release notes taken from that version's CHANGELOG section

Add `-DryRun` to preview the changes without committing or pushing.

## If something goes wrong

| Problem | Fix |
|:--|:--|
| CI says "Version mismatch" | Run `.\release.ps1 -Version X.Y.Z` instead of tagging by hand |
| The build failed after tagging | Fix the problem, then run `git tag -d vX.Y.Z; git push origin :refs/tags/vX.Y.Z`, then release again |
| Wrong release notes | Edit the release on GitHub, or fix `CHANGELOG.md` and re-run the workflow on the tag |

## First release

The first time, `PUBLISH.bat` pushes the project and tags **v1.0.0**, which creates the first release.

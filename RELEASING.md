# Releasing Papan

Papan is not notarized (no Apple Developer account) and the Windows installers are not signed. The main macOS channel is
a **prebuilt app**: a Homebrew cask (`brew install --cask alifu/tap/papan`) and `Papan.zip` on the GitHub release.
Windows and Linux users download the installers from the same release.

## One release

1. Pick the version, for example `0.0.1-alpha-1`. Set it in all three places and check them:
   - `version` in `src-tauri/tauri.conf.json` (the app),
   - `version` in `src-tauri/Cargo.toml` (the crate; also update `src-tauri/Cargo.lock`),
   - `version` in `package.json` (also `package-lock.json`).

   ```sh
   sh scripts/release/check-version.sh
   ```

   A version with a `-` (alpha, beta, rc) is published as a GitHub **pre-release**.
2. Windows MSI only allows numbers, so `bundle.windows.wix.version` in `tauri.conf.json` holds a numeric twin of the
   version (`0.0.1-alpha-1` is `0.0.1.1`). Change it with the version. The NSIS `.exe` shows the full version.
3. Run the checks: `make check`, then `sh scripts/release/test-release.sh`.
4. Commit, then tag and push: `git tag vX.Y.Z && git push origin main vX.Y.Z` (use the same version as in the files,
   with a `v`).
5. The **Release** workflow checks the versions, builds on macOS (universal, ad-hoc signed), Windows and Linux, and
   publishes one GitHub Release with:

   | File | What |
   |---|---|
   | `Papan.zip` | `Papan.app` (universal, ad-hoc signed); the cask installs from it |
   | `Papan_*.dmg` | macOS disk image |
   | `Papan_*-setup.exe`, `Papan_*.msi` | Windows installers (unsigned) |
   | `Papan_*.AppImage`, `.deb`, `.rpm` | Linux (no `.rpm` for a pre-release: RPM versions cannot contain `-`) |
   | `SHA256SUMS` | checksums of all of the above |
   | `papan.rb` | the finished cask |

6. **The cask** goes to the tap as `Casks/papan.rb`:
   - automatic: add a repository secret `TAP_TOKEN` (a token that can push to `alifu/homebrew-tap`) and the workflow
     commits the cask itself;
   - manual: copy the release's `papan.rb` to `Casks/papan.rb` in the tap and push.
7. Check on a Mac if you can: `brew update && brew install --cask alifu/tap/papan`, then open the app (allow it once).

## Dry run

A pull request that changes `.github/workflows/release.yml`, `scripts/release/**` or `src-tauri/tauri.conf.json` builds
all three platforms and uploads the files as workflow artifacts without publishing. You can also start the workflow by
hand from the Actions tab once it is on the default branch. Publishing only happens for a pushed tag.

## What the cask installs

`scripts/release/make-cask.sh VERSION SHA256` renders the cask. It installs `Papan.app` into `/Applications`, quits the
app on uninstall and removes its data folders with `brew uninstall --zap`.

## Notes

- Ad-hoc signing comes from `APPLE_SIGNING_IDENTITY: "-"` in the workflow. If you later get a Developer ID, add the
  certificate and notarization secrets; the "Open Anyway" step disappears.
- Windows shows a SmartScreen warning for unsigned installers: *More info* then *Run anyway*.
- The Linux build needs the packages listed in the workflow (WebKitGTK 4.1 and friends); `rpm` is installed for the
  `.rpm` bundle.

# Release Process

This document describes how to cut and publish a release of `txt`.

## Platform support

| Platform | Target triple | Binary archive |
|---|---|---|
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |
| Linux x86_64 (static) | `x86_64-unknown-linux-musl` | `.tar.gz` |
| Linux aarch64 (static) | `aarch64-unknown-linux-musl` | `.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `.zip` |

Linux binaries are statically linked against musl libc — they run on any distro without glibc version constraints.

## Versioning

`txt` follows [Semantic Versioning](https://semver.org) (`MAJOR.MINOR.PATCH`):

- **PATCH** — bug fixes, performance improvements, no behaviour changes.
- **MINOR** — new features, backward-compatible.
- **MAJOR** — breaking changes (config format, key bindings, command-line interface).

Releases are tagged `vMAJOR.MINOR.PATCH` (e.g. `v1.2.0`). Pre-releases use `-alpha.N`, `-beta.N`, or `-rc.N` suffixes (e.g. `v1.0.0-rc.1`).

## Prerequisites

- Push access to the `main` branch and permission to create tags on GitHub.
- `cargo` installed locally (used to verify the build before tagging).

## Step-by-step release procedure

### 1. Prepare the release commit

1. Confirm all intended changes are merged to `main` and CI is green.
2. Update the version in `Cargo.toml`:
   ```toml
   version = "X.Y.Z"
   ```
3. Update `Cargo.lock` (runs automatically):
   ```sh
   cargo build
   ```
4. Commit:
   ```sh
   git commit -am "Release vX.Y.Z"
   ```

### 2. Tag and push

```sh
git tag vX.Y.Z
git push origin main
git push origin vX.Y.Z
```

Pushing the tag triggers `.github/workflows/release.yml`, which:
- Builds binaries for all five targets in parallel
- Creates a GitHub Release named `vX.Y.Z`
- Attaches all binary archives and `checksums.txt`
- Auto-generates release notes from commit messages

Monitor progress at: `https://github.com/<owner>/txt/actions`

### 3. Verify the release

Once the workflow finishes:
- Open the release page on GitHub and confirm all five archives are attached.
- Download the binary for your platform, run `txt --version`, and spot-check it.

### 4. Post-release: update distribution channels

#### Homebrew tap

The Homebrew formula lives in a separate repository (`<owner>/homebrew-tap`).

1. Compute the SHA256 for each archive:
   ```sh
   sha256sum txt-vX.Y.Z-*.tar.gz
   ```
   (Or download `checksums.txt` from the GitHub Release.)

2. In `homebrew-tap`, edit `Formula/txt.rb`:
   - Update `version`, `url`, and each `sha256` field for the bottles.

3. Commit and push to the tap repository.

Users who added the tap (`brew tap <owner>/tap`) will receive the update on their next `brew upgrade`.

#### AUR (Arch Linux)

The AUR package (`txt-bin`) needs its `PKGBUILD` updated.

1. Clone your AUR repository:
   ```sh
   git clone ssh://aur@aur.archlinux.org/txt-bin.git
   ```
2. Update `pkgver`, `source` URL, and `sha256sums` in `PKGBUILD`.
3. Regenerate `.SRCINFO`:
   ```sh
   makepkg --printsrcinfo > .SRCINFO
   ```
4. Commit and push to the AUR:
   ```sh
   git commit -am "Update to vX.Y.Z"
   git push
   ```

## Distribution and installation

### macOS and Linux — install script

```sh
curl -fsSL https://raw.githubusercontent.com/<owner>/txt/main/install.sh | sh
```

Installs the latest release binary to `~/.local/bin/txt`.
To update, run the same command again.
To uninstall: `rm ~/.local/bin/txt`

### Windows — install script (PowerShell)

```powershell
irm https://raw.githubusercontent.com/<owner>/txt/main/install.ps1 | iex
```

Installs the latest release to `%LOCALAPPDATA%\txt\txt.exe` and adds it to your user `PATH`.
To update, run the same command again.
To uninstall: delete `%LOCALAPPDATA%\txt` and remove it from your `PATH`.

### Homebrew (macOS and Linux)

```sh
brew tap <owner>/tap
brew install txt
```

To update: `brew upgrade txt`

### AUR (Arch Linux)

Using an AUR helper such as `paru` or `yay`:

```sh
paru -S txt-bin
```

### Manual download

Download the archive for your platform from the [Releases page](https://github.com/<owner>/txt/releases), extract the binary, and place it anywhere on your `PATH`.

Verify integrity using the provided `checksums.txt`:
```sh
sha256sum --check checksums.txt
```

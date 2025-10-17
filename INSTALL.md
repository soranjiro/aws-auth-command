# Installation Guide

## Homebrew (macOS/Linux)

### Option 1: Using Homebrew Tap (Recommended)

```sh
# Add the tap
brew tap soranjiro/awx https://github.com/soranjiro/aws-auth-command

# Install
brew install awx
```

### Option 2: Direct Formula Installation

```sh
brew install https://raw.githubusercontent.com/soranjiro/aws-auth-command/main/Formula/awx.rb
```

## npm/npx

Note: npm installation downloads a prebuilt binary (no Rust required). If you explicitly want to build from source, set `AWX_BUILD_FROM_SOURCE=1`.

### Global Installation

```sh
npm install -g @soranjiro/awx
```

### Using npx (No Installation)

```sh
npx @soranjiro/awx -- s3 ls
```

## Cargo (Rust)

```sh
cargo install --git https://github.com/soranjiro/aws-auth-command
```

Or from source (requires Rust):

```sh
git clone https://github.com/soranjiro/aws-auth-command.git
cd aws-auth-command
cargo install --path .
```

## Manual Installation

### Download Pre-built Binary

1. Go to [Releases](https://github.com/soranjiro/aws-auth-command/releases)
2. Download the binary for your platform:
   - macOS (Intel): `awx-x86_64-apple-darwin`
   - macOS (Apple Silicon): `awx-aarch64-apple-darwin`
   - Linux (x64): `awx-x86_64-unknown-linux-gnu`
   - Linux (ARM64): `awx-aarch64-unknown-linux-gnu`

3. Make it executable and move to PATH:

```sh
chmod +x awx-*
sudo mv awx-* /usr/local/bin/awx
```

### Build from Source

Requirements:
- Rust 1.70+ ([Install Rust](https://rustup.rs/))

```sh
git clone https://github.com/soranjiro/aws-auth-command.git
cd aws-auth-command
cargo build --release
sudo cp target/release/awx /usr/local/bin/
```

## Verify Installation

```sh
awx --version

## Maintainers: Release & Formula update

1. Create and push a tag `vX.Y.Z`.
2. GitHub Actions workflow `.github/workflows/release.yml` will:
   - Build archives for targets:
     - `x86_64-apple-darwin`, `aarch64-apple-darwin`
     - `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
   - Upload assets to the GitHub Release
   - Publish the npm package `@soranjiro/awx`
   - Attempt to update `Formula/awx.rb` with URLs and SHA256 and push the change
3. If the formula was not updated by CI, manually update `Formula/awx.rb`:
   - Replace the versioned URLs to point to the new `vX.Y.Z`
   - Replace SHA256 placeholders (`SHA256_*`) with checksums of uploaded archives
4. Users can then `brew update && brew upgrade awx` or `npm i -g @soranjiro/awx`.
```

## Updating

### Homebrew
```sh
brew update
brew upgrade awx
```

### npm
```sh
npm update -g @soranjiro/awx
```

### Cargo
```sh
cargo install --git https://github.com/soranjiro/aws-auth-command --force
```

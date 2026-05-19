# Release workflow rewrite plan — ctxmenu

## Goal

Switch the release workflow from manual `workflow_dispatch` (rolling `latest` tag) to **tag-push triggered**, with versioned releases and an auto-changelog. Only one release entry exists at any time; tags accumulate forever for history.

## Current state (`.github/workflows/release.yml`)

- **Trigger:** `workflow_dispatch` (manual button)
- **Release model:** rolling — force-replaces a `latest` tag and release every run
- **Binary:** `target/release/ctxmenu.exe`

## Proposed changes

1. **Trigger:** change to `push: tags: ['[0-9]+.[0-9]+.[0-9]+']` — workflow fires when you push a semver tag (e.g. `0.2.0`, no `v` prefix).
2. **Read version from the tag** (`GITHUB_REF`), validate it matches `X.Y.Z`, fail loudly otherwise.
3. **Sed the version into `Cargo.toml`** so the built binary's metadata matches the released version. Not committed back — local to the runner only.
4. **Build the binary** (unchanged — `cargo build --release`).
5. **Only on successful build:** find the most recent existing release, run `gh release delete <prev-tag> --yes` (without `--cleanup-tag` — tag remains as a historical marker, only the release entry + assets are freed).
6. **Create the new release** with:
   - Tag = the pushed version (e.g. `0.2.0`)
   - Name = `ctxmenu 0.2.0`
   - `generate_release_notes: true` (auto-changelog from commits/PRs since previous tag)
   - File = `target/release/ctxmenu.exe`

## Proposed full workflow

```yaml
name: Release

on:
  push:
    tags:
      - '[0-9]+.[0-9]+.[0-9]+'

permissions:
  contents: write

jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v6

      - name: Get version from tag
        id: version
        shell: bash
        run: |
          VERSION="${GITHUB_REF#refs/tags/}"
          if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            echo "::error::Tag '$VERSION' is not valid semver (expected X.Y.Z)"
            exit 1
          fi
          echo "version=$VERSION" >> "$GITHUB_OUTPUT"

      - name: Set Cargo.toml version
        shell: bash
        run: |
          sed -i "s/^version = \".*\"/version = \"${{ steps.version.outputs.version }}\"/" Cargo.toml
          grep '^version' Cargo.toml

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: actions/cache@v5
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('Cargo.lock') }}

      - name: Build release
        run: cargo build --release

      - name: Delete previous release entry (keep tag)
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        shell: bash
        run: |
          PREV_TAG=$(gh release list --limit 1 --json tagName --jq '.[0].tagName' || true)
          if [ -n "$PREV_TAG" ] && [ "$PREV_TAG" != "${{ steps.version.outputs.version }}" ]; then
            echo "Deleting previous release: $PREV_TAG (tag remains)"
            gh release delete "$PREV_TAG" --yes
          fi

      - name: Create release
        uses: softprops/action-gh-release@v3
        with:
          tag_name: ${{ steps.version.outputs.version }}
          name: ctxmenu ${{ steps.version.outputs.version }}
          generate_release_notes: true
          files: target/release/ctxmenu.exe
```

## Caveats

- The current `latest` release (`ctxmenu.exe` from 2026-05-19) will get **deleted** the first time a new tag is pushed. The `latest` tag itself remains in git history but you'll only have the new versioned release going forward.
- `Cargo.toml`'s committed version stays at whatever's currently in source — sed only affects the runner. Drift accepted per project rule.
- Tag pattern `[0-9]+.[0-9]+.[0-9]+` only matches strict semver `X.Y.Z`. Pre-release suffixes like `0.2.0-rc1` won't trigger.

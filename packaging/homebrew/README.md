# Homebrew distribution

`Formula/astria.rb` installs the published npm package (`@nodesify/astria`,
prebuilt native binaries, no Rust toolchain) and symlinks the `astria` binary.

## Users install via the tap

```bash
brew install nodesify/tap/astria
```

## Maintaining the tap

The tap lives at <https://github.com/Nodesify/homebrew-tap> and holds a copy of
this formula. After each release:

1. Copy `Formula/astria.rb` into the tap's `Formula/`.
2. Bump `url` and `version` to the new version.
3. Recompute the checksum:

   ```bash
   npm view @nodesify/astria dist.tarball
   curl -sL <tarball-url> | sha256sum
   ```

4. Verify locally: `brew install --build-from-source nodesify/tap/astria`.

If `Nodesify/homebrew-tap` does not exist yet, create the public repo with
`Formula/` at its root (Homebrew taps are `user/tap` → `github.com/user/homebrew-tap`).
Once astria has enough adoption, the formula can be submitted to homebrew-core
(see Homebrew's notable/usage acceptance criteria); until then the tap covers
`brew search` discoverability.

# Releasing Salt

Salt uses `vMAJOR.MINOR.PATCH` Git tags. The first public release is `v0.1.0`, matching the version in `Cargo.toml`. Keep `Cargo.lock` committed so source builds use the tested dependency set. Do not commit local `salt.toml`, build output, or machine-specific shell configuration.

## Release checklist

1. Review `git status` and `git diff --check`. Confirm that every source, example, documentation, and workflow change is committed.
2. On macOS and Linux, run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`, and `cargo build --release --locked`. Check `target/release/salt --version` and `target/release/salt --help`. The ZIP tests need `zip` on `PATH`.
3. Review the README, example configuration, license, and planned release notes. Push the release commit and verify that both GitHub CI jobs pass.
4. Run the `Release` workflow manually on `main` to build and inspect its four downloadable artifacts. A manual run does not create a GitHub release.
5. Tag the tested commit and push the tag: `git tag -a v0.1.0 -m 'Salt v0.1.0'`, then `git push origin v0.1.0`. The tag workflow checks the package version, runs formatting, lint and tests, builds Linux and macOS packages for x86-64 and ARM64, and creates a draft GitHub release with `SHA256SUMS`.
6. Review the draft release notes, download an asset to check it, and publish the draft in GitHub. Mention the local-only scope and supported platforms. The Homebrew tap can be updated after publication.

The Linux binaries are built on Ubuntu 24.04 and use GNU libc. Users on older or different Linux systems can build from source. Each archive contains `salt`, the README, the license, and the example configuration.

Avoid moving a published tag. For a fix, increase the patch version in `Cargo.toml` and publish a new tag.

## Homebrew tap

Use a separate public repository named `rsenses/homebrew-tap`. This lets users install Salt before it meets Homebrew/core's contribution criteria. The formula can be named `salt-file-manager` to distinguish it from the existing Salt infrastructure software while still installing the `salt` executable. Create that GitHub repository and a local tap with `brew tap-new rsenses/tap`, then connect the tap's Git remote to the new repository.

After the GitHub tag exists, download its source archive and calculate its SHA-256:

```sh
curl -L -o salt-v0.1.0.tar.gz https://github.com/rsenses/salt/archive/refs/tags/v0.1.0.tar.gz
shasum -a 256 salt-v0.1.0.tar.gz
```

Create `Formula/salt-file-manager.rb` in the tap with the reported checksum in place of `REPLACE_WITH_SHA256`:

```ruby
class SaltFileManager < Formula
  desc "Local terminal file manager with an editable directory buffer"
  homepage "https://github.com/rsenses/salt"
  url "https://github.com/rsenses/salt/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256"
  license "MIT"

  depends_on "rust" => :build

  def fetch
    system "cargo", "fetch", *std_cargo_fetch_args
  end

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "salt #{version}", shell_output("#{bin}/salt --version")
  end
end
```

Test the formula on macOS and Linux with `brew install --build-from-source rsenses/tap/salt-file-manager`, `brew test rsenses/tap/salt-file-manager`, and `brew audit --new --formula rsenses/tap/salt-file-manager`. Commit and push the tap only after these checks pass. Users can then run `brew install rsenses/tap/salt-file-manager`. The optional `fzf`, `zoxide`, `zip`, and Nerd Font features still need their respective tools or font installed separately.

For each later Salt release, update the formula's URL and checksum, test it, and push the tap change. Add the live Homebrew command to the main README once the tap exists.

Homebrew references: [taps](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap), [formula cookbook](https://docs.brew.sh/Formula-Cookbook.html), and [Rust build helpers](https://docs.brew.sh/rubydoc/Formula.html#std_cargo_args-instance_method).

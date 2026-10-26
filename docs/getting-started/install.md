# Install bussard

bussard is a single binary. Use an installer, download it by hand, or build it from source.

## Use an installer

On macOS or Linux, `brew install tmbo/tap/bussard` installs from the Homebrew tap. Without Homebrew, the install script downloads the binary for your machine and checks its SHA-256 checksum. The output below is illustrative:

```console
$ curl -fsSL https://raw.githubusercontent.com/tmbo/bussard/main/install.sh | sh
Downloading bussard <version> (bussard-macos-arm64)
Checksum verified
Installed /usr/local/bin/bussard

Next: bussard --help
```

The script installs to `/usr/local/bin` when it can write there, otherwise to `~/.local/bin`, and tells you how to add that to your `PATH`. Set `BUSSARD_VERSION` to pin a version and `BUSSARD_INSTALL_DIR` to choose the directory.

On Windows, the PowerShell installer does the same:

```console
$ irm https://raw.githubusercontent.com/tmbo/bussard/main/install.ps1 | iex
Downloading bussard <version> (bussard-windows-x64.exe)
Checksum verified
```

It puts `bussard.exe` in `%LOCALAPPDATA%\Programs\bussard`. Windows shows a SmartScreen warning, because the binary is not signed.

## Download the binary by hand

Every [release](https://github.com/tmbo/bussard/releases/latest) has five binaries: `bussard-linux-x64`, `bussard-linux-arm64`, `bussard-macos-arm64`, `bussard-macos-x64` and `bussard-windows-x64.exe`. Each comes with a `.sha256` file next to it. You can also download them from the [landing page](../../).

Check the download against its checksum file (`sha256sum -c` on Linux), make it executable and put it on your `PATH`:

```console
$ shasum -a 256 -c bussard-macos-arm64.sha256
bussard-macos-arm64: OK
$ chmod +x bussard-macos-arm64
$ mv bussard-macos-arm64 /usr/local/bin/bussard
$ xattr -d com.apple.quarantine /usr/local/bin/bussard
```

The `xattr` line is for macOS only: the binary is not notarized, so macOS blocks it until you remove the quarantine flag once.

## Build from source

With Rust 1.88 or newer:

```console
$ git clone https://github.com/tmbo/bussard
$ cd bussard
$ cargo install --path crates/bussard-cli
```

## Next steps

- [Your first model](first-model.md): import an ETS export or start empty, then watch the bus.
- [Connect an assistant](assistant.md): let an LLM read and edit the model over MCP.

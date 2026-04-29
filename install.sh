#!/bin/sh
set -eu

repo="${CORTEX_REPO:-tky0065/cortex}"
install_dir="${CORTEX_INSTALL_DIR:-$HOME/.local/bin}"
version="${CORTEX_VERSION:-latest}"

command_exists() {
    command -v "$1" >/dev/null 2>&1
}

fail() {
    echo "cortex install error: $*" >&2
    exit 1
}

command_exists curl || fail "curl is required"
command_exists tar || fail "tar is required"

os="$(uname -s)"
arch="$(uname -m)"

case "$os" in
    Linux)
        case "$arch" in
            x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
            *) fail "unsupported Linux architecture: $arch" ;;
        esac
        ;;
    Darwin)
        case "$arch" in
            x86_64|amd64) target="x86_64-apple-darwin" ;;
            arm64|aarch64) target="aarch64-apple-darwin" ;;
            *) fail "unsupported macOS architecture: $arch" ;;
        esac
        ;;
    *)
        fail "unsupported operating system: $os"
        ;;
esac

if [ "$version" = "latest" ]; then
    latest_url="$(curl -fsIL -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")"
    version="${latest_url##*/}"
    [ -n "$version" ] && [ "$version" != "latest" ] || fail "could not resolve latest release"
fi

archive="cortex-$version-$target.tar.gz"
base_url="https://github.com/$repo/releases/download/$version"
tmp_dir="$(mktemp -d)"

cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT HUP INT TERM

echo "Installing cortex $version for $target..."
curl -fL --progress-bar "$base_url/$archive" -o "$tmp_dir/$archive"
curl -fsSL "$base_url/SHA256SUMS" -o "$tmp_dir/SHA256SUMS"

if command_exists sha256sum; then
    (cd "$tmp_dir" && grep "  $archive\$" SHA256SUMS | sha256sum -c -) >/dev/null
elif command_exists shasum; then
    (cd "$tmp_dir" && grep "  $archive\$" SHA256SUMS | shasum -a 256 -c -) >/dev/null
else
    fail "sha256sum or shasum is required to verify the download"
fi

tar -xzf "$tmp_dir/$archive" -C "$tmp_dir"
mkdir -p "$install_dir"
install -m 755 "$tmp_dir/cortex" "$install_dir/cortex"

echo "cortex installed to $install_dir/cortex"

case ":$PATH:" in
    *":$install_dir:"*) ;;
    *)
        echo ""
        echo "Add this directory to your PATH:"
        echo "  export PATH=\"$install_dir:\$PATH\""
        ;;
esac

echo ""
echo "Run: cortex --version"

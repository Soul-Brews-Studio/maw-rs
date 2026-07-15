#!/bin/sh
set -eu

usage() {
  echo "usage: $0 <vCalVer-tag> <macos-arm64.sha256>" >&2
  exit 2
}

[ "$#" -eq 2 ] || usage
tag=$1
checksum_file=$2

case "$tag" in
  v[0-9]*.*) ;;
  *) echo "invalid release tag: $tag" >&2; exit 2 ;;
esac
case "$tag" in
  *-*) echo "Homebrew formulae are generated for stable tags only: $tag" >&2; exit 2 ;;
esac

sha256=$(awk 'NR == 1 { print $1 }' "$checksum_file")
case "$sha256" in
  *[!0-9a-fA-F]*|'') echo "invalid sha256 in $checksum_file" >&2; exit 2 ;;
esac
[ "${#sha256}" -eq 64 ] || { echo "invalid sha256 length in $checksum_file" >&2; exit 2; }

version=${tag#v}
cat <<FORMULA
class Maw < Formula
  desc "Fleet-native orchestration CLI"
  homepage "https://github.com/Soul-Brews-Studio/maw-rs"
  license "BUSL-1.1"

  stable do
    url "https://github.com/Soul-Brews-Studio/maw-rs/releases/download/${tag}/maw-rs-macos-arm64"
    version "${version}"
    sha256 "${sha256}"
    depends_on arch: :arm64
  end

  head do
    url "https://github.com/Soul-Brews-Studio/maw-rs.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "crates/maw-cli")
      mv bin/"maw-rs", bin/"maw"
    else
      chmod 0755, "maw-rs-macos-arm64"
      bin.install "maw-rs-macos-arm64" => "maw"
    end
    generate_completions_from_executable bin/"maw", "completions", shells: [:zsh]
  end

  test do
    assert_match "maw-rs v#{version}", shell_output("#{bin}/maw --version")
    system bin/"maw", "ls"
  end
end
FORMULA

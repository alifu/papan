#!/bin/sh
# Prints the Homebrew cask of a release, which installs the prebuilt Papan.app.
#
#   scripts/release/make-cask.sh VERSION SHA256 > papan.rb
#
# The Release workflow runs this and attaches the result to the GitHub release; copy it to Casks/papan.rb in
# alifu/homebrew-tap (the workflow does that itself when the TAP_TOKEN secret is set).
set -eu

[ $# -eq 2 ] || { echo "usage: make-cask.sh VERSION SHA256" >&2; exit 2; }
version="$1"
sha="$2"
# MAJOR.MINOR.PATCH with an optional -prerelease such as -alpha-1.
case "$version" in
  "" | *[!0-9A-Za-z.-]* | -* | .* | *..*) echo "bad version: $version" >&2; exit 2 ;;
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) echo "bad version: $version" >&2; exit 2 ;;
esac
case "$sha" in *[!0-9a-f]* | "") echo "bad sha256: $sha" >&2; exit 2 ;; esac
[ "${#sha}" -eq 64 ] || { echo "sha256 must be 64 hex digits" >&2; exit 2; }

cat <<RUBY
cask "papan" do
  version "$version"
  sha256 "$sha"

  url "https://github.com/alifu/papan/releases/download/v#{version}/Papan.zip"
  name "Papan"
  desc "Desktop API client, mock server and journey runner"
  homepage "https://github.com/alifu/papan"

  depends_on macos: :ventura

  app "Papan.app"

  uninstall quit: "com.papan-lifux.app"

  zap trash: [
    "~/Library/Application Support/com.papan-lifux.app",
    "~/Library/Caches/com.papan-lifux.app",
    "~/Library/Logs/com.papan-lifux.app",
    "~/Library/Saved Application State/com.papan-lifux.app.savedState",
    "~/Library/WebKit/com.papan-lifux.app",
  ]
end
RUBY

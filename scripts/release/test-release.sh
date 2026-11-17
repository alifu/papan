#!/bin/sh
# Checks the two release scripts. Run from the repository root: sh scripts/release/test-release.sh
set -u
fail=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fail=1; }

sha="$(printf 'x' | shasum -a 256 | cut -d ' ' -f 1)"

# make-cask.sh
out="$(sh scripts/release/make-cask.sh 0.0.1-alpha-1 "$sha" 2>&1)" && ok "cask renders a prerelease version" || bad "cask renders a prerelease version"
echo "$out" | grep -q '^  version "0.0.1-alpha-1"$' && ok "cask names the version" || bad "cask names the version"
echo "$out" | grep -q "^  sha256 \"$sha\"\$" && ok "cask names the checksum" || bad "cask names the checksum"
echo "$out" | grep -q 'releases/download/v#{version}/Papan.zip' && ok "cask points at Papan.zip" || bad "cask points at Papan.zip"
for v in "" "1.2" "v1.2.3" "1.2.3 4" '1.2.3";x' "-1.2.3" "1..3"; do
  sh scripts/release/make-cask.sh "$v" "$sha" >/dev/null 2>&1 && bad "cask refuses version '$v'" || ok "cask refuses version '$v'"
done
for s in "" abc "$(printf '%s' "$sha" | tr a-f A-F)" "${sha}0"; do
  sh scripts/release/make-cask.sh 1.2.3 "$s" >/dev/null 2>&1 && bad "cask refuses checksum '$s'" || ok "cask refuses checksum '$s'"
done

# check-version.sh
sh scripts/release/check-version.sh >/dev/null 2>&1 && ok "versions agree" || bad "versions agree"
sh scripts/release/check-version.sh 9.9.9 >/dev/null 2>&1 && bad "a wrong tag is refused" || ok "a wrong tag is refused"

exit "$fail"

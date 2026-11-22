#!/usr/bin/env bash
# Build a signed APT repository for Yonder under <out>/apt.
# Usage: build-apt-repo.sh <output-dir>
# Env:   APT_GPG_PRIVATE_KEY  armored private key; if empty the apt repo is skipped.
#        GH_TOKEN             token used by `gh` to download the release asset.
set -euo pipefail

OUT="${1:?usage: build-apt-repo.sh <output-dir>}"
REPO="RMoura98/yonder"

if [ -z "${APT_GPG_PRIVATE_KEY:-}" ]; then
  echo "::warning::APT_GPG_PRIVATE_KEY is empty; skipping APT repository (not publishing an unsigned repo)."
  exit 0
fi

WORK="$(mktemp -d)"
export GNUPGHOME="$WORK/gnupg"
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
trap 'gpgconf --kill gpg-agent >/dev/null 2>&1 || true; rm -rf "$WORK"' EXIT

echo "Downloading latest amd64 .deb from $REPO"
mkdir -p "$WORK/dl"
gh release download --repo "$REPO" --pattern "*_amd64.deb" --dir "$WORK/dl" --clobber
DEB="$(find "$WORK/dl" -maxdepth 1 -name '*_amd64.deb' | sort | tail -n1)"
[ -n "$DEB" ] || { echo "No amd64 .deb found in latest release" >&2; exit 1; }

NAME="$(dpkg-deb -f "$DEB" Package)"
echo "Package name: $NAME"

APT="$OUT/apt"
POOL="pool/main/${NAME:0:1}/$NAME"
DIST="dists/stable"
BIN="$DIST/main/binary-amd64"
rm -rf "$APT"
mkdir -p "$APT/$POOL" "$APT/$BIN"
cp "$DEB" "$APT/$POOL/"

(
  cd "$APT"
  apt-ftparchive packages pool > "$BIN/Packages"
  gzip -9 -n -k -f "$BIN/Packages"
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin="Yonder" \
    -o APT::FTPArchive::Release::Label="Yonder" \
    -o APT::FTPArchive::Release::Suite="stable" \
    -o APT::FTPArchive::Release::Codename="stable" \
    -o APT::FTPArchive::Release::Architectures="amd64" \
    -o APT::FTPArchive::Release::Components="main" \
    release "$DIST" > "$WORK/Release"
  mv "$WORK/Release" "$DIST/Release"
)

echo "Importing signing key"
printf '%s\n' "$APT_GPG_PRIVATE_KEY" | gpg --batch --import
FPR="$(gpg --batch --list-secret-keys --with-colons | awk -F: '/^fpr:/ {print $10; exit}')"
[ -n "$FPR" ] || { echo "Could not find imported secret key" >&2; exit 1; }

GPG_SIGN=(gpg --batch --yes --pinentry-mode loopback --passphrase "" --default-key "$FPR")
"${GPG_SIGN[@]}" --clearsign -o "$APT/$DIST/InRelease" "$APT/$DIST/Release"
"${GPG_SIGN[@]}" --armor --detach-sign -o "$APT/$DIST/Release.gpg" "$APT/$DIST/Release"

gpg --batch --yes --export "$FPR" > "$APT/yonder.gpg"
gpg --batch --yes --armor --export "$FPR" > "$APT/yonder.asc"

echo "APT repository built at $APT (key $FPR)"

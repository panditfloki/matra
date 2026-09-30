#!/bin/bash
#
# Sign a local Debug build with a STABLE self-signed identity, so the macOS
# keychain "Always Allow" grant persists across launches.
#
# Why this exists: `make run`/`make build` produce an ad-hoc-signed app. An
# ad-hoc build has no stable code identity, so the keychain access-control
# list cannot match it to a saved "Always Allow" grant — and the prompt to
# read Claude Code's (or another tool's) token returns on *every* launch. A
# stable self-signed identity fixes that: the grant binds to it and sticks.
#
# This is for local development only. It needs no Apple Developer account and
# does not notarize — official releases are Developer ID signed by
# `make release`. The certificate lives in your login keychain and is reused
# on subsequent runs.
#
# Usage:
#   Scripts/sign-local.sh [path/to/Matra.app]
# Default target is the app in /Applications.

set -euo pipefail

APP="${1:-/Applications/Matra.app}"
CN="Matra Local Signing"

if [ ! -d "$APP" ]; then
  echo "error: $APP not found — build and install it first (make run)." >&2
  exit 1
fi

if ! security find-identity -p codesigning | grep -q "$CN"; then
  echo "Creating self-signed code-signing certificate '$CN'…"
  DIR=$(mktemp -d)
  trap 'rm -rf "$DIR"' EXIT
  cat > "$DIR/ext.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions    = v3
prompt             = no
[dn]
CN = $CN
[v3]
basicConstraints     = critical,CA:false
keyUsage             = critical,digitalSignature
extendedKeyUsage     = critical,codeSigning
EOF
  openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
    -keyout "$DIR/key.pem" -out "$DIR/cert.pem" -config "$DIR/ext.cnf"
  # -legacy: OpenSSL 3 otherwise writes a PKCS#12 that `security` cannot
  # import ("MAC verification failed"). Harmless on OpenSSL that ignores it.
  openssl pkcs12 -export -legacy -inkey "$DIR/key.pem" -in "$DIR/cert.pem" \
    -out "$DIR/id.p12" -passout pass:matra -name "$CN" 2>/dev/null || \
  openssl pkcs12 -export -inkey "$DIR/key.pem" -in "$DIR/cert.pem" \
    -out "$DIR/id.p12" -passout pass:matra -name "$CN"
  # -T grants /usr/bin/codesign access to the key; -A avoids a prompt per use.
  security import "$DIR/id.p12" -k "$HOME/Library/Keychains/login.keychain-db" \
    -P matra -T /usr/bin/codesign -A
fi

# Quit a running instance so the bundle can be replaced in place.
osascript -e 'quit app "Matra"' 2>/dev/null || true
sleep 1

codesign --force --deep --sign "$CN" "$APP"
codesign -dv "$APP" 2>&1 | grep -E 'Authority|Identifier' || true

echo
echo "Signed with '$CN'. Relaunch Matra and grant the keychain prompt one"
echo "more time (new identity) — it will not ask again on later launches."

#!/bin/bash
# One-time: a self-signed code-signing identity "mmfast-dev" in your login
# keychain, so every rebuild of mmfast.app keeps the same signature and the
# keychain stops asking to approve the app after each build.
# Asks for your Mac password (importing the key and trusting it for code signing).
set -euo pipefail
if security find-identity -v -p codesigning | grep -q '"mmfast-dev"'; then
  echo "mmfast-dev already exists"; exit 0
fi
TMP=$(mktemp -d); trap 'rm -rf "$TMP"' EXIT
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=mmfast-dev" \
  -keyout "$TMP/key.pem" -out "$TMP/cert.pem" \
  -addext "keyUsage=critical,digitalSignature" -addext "extendedKeyUsage=critical,codeSigning" 2>/dev/null
openssl pkcs12 -export -legacy -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -out "$TMP/id.p12" -passout pass:mmfast 2>/dev/null \
  || openssl pkcs12 -export -inkey "$TMP/key.pem" -in "$TMP/cert.pem" -out "$TMP/id.p12" -passout pass:mmfast
security import "$TMP/id.p12" -k ~/Library/Keychains/login.keychain-db -P mmfast -T /usr/bin/codesign
security add-trusted-cert -r trustRoot -p codeSign -k ~/Library/Keychains/login.keychain-db "$TMP/cert.pem"
security find-identity -v -p codesigning | grep mmfast-dev
echo "done: rebuild with dev/bundle-macos.sh"

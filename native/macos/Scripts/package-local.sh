#!/bin/bash
# Package the existing developer configuration. No release or security-policy changes.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="${1:?Usage: package-local.sh /absolute/output/directory [SwiftPM-cache]}"
PACKAGE_CACHE="${2:-$PROJECT_DIR/build/SourcePackages}"
mkdir -p "$OUTPUT_DIR"

xcodebuild -project "$PROJECT_DIR/Matra.xcodeproj" -scheme Matra \
  -destination 'platform=macOS,arch=arm64' -configuration Debug \
  -derivedDataPath "$PROJECT_DIR/build" -clonedSourcePackagesDirPath "$PACKAGE_CACHE" \
  -onlyUsePackageVersionsFromResolvedFile CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM= \
  CODE_SIGN_STYLE=Automatic build

APP="$PROJECT_DIR/build/Build/Products/Debug/Matra.app"
VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP/Contents/Info.plist")
DESTINATION="$OUTPUT_DIR/Matra-$VERSION-macOS-arm64-local.dmg"
if [[ -e "$DESTINATION" ]]; then
  echo "Refusing to overwrite $DESTINATION" >&2
  exit 1
fi
codesign --verify --deep --strict "$APP"
STAGING=$(mktemp -d "${TMPDIR:-/tmp}/matra-dmg.XXXXXX")
ditto "$APP" "$STAGING/Matra.app"
ln -s /Applications "$STAGING/Applications"
cp "$PROJECT_DIR/INSTALL-MAC.md" "$STAGING/INSTALL-MAC.md"
hdiutil create -volname "Matra $VERSION" -srcfolder "$STAGING" -format UDZO -fs HFS+ "$DESTINATION"
hdiutil verify "$DESTINATION"
shasum -a 256 "$DESTINATION" > "$DESTINATION.sha256"
echo "Local installer: $DESTINATION"
echo 'Ad-hoc signed. Not Apple notarized. Staging retained for inspection:'
echo "$STAGING"

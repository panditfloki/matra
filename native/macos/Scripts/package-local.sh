#!/bin/bash
# Package an ad-hoc test build. Does not install, publish or change OS security.
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT_DIR="${1:?Usage: package-local.sh /absolute/output/directory [SwiftPM-cache]}"
PACKAGE_CACHE="${2:-$PROJECT_DIR/build/SourcePackages}"
mkdir -p "$OUTPUT_DIR"

xcodebuild -project "$PROJECT_DIR/Matra.xcodeproj" -scheme Matra \
  -destination 'platform=macOS,arch=arm64' -configuration Release \
  -derivedDataPath "$PROJECT_DIR/build" -clonedSourcePackagesDirPath "$PACKAGE_CACHE" \
  -onlyUsePackageVersionsFromResolvedFile CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM= \
  CODE_SIGN_STYLE=Automatic ENABLE_HARDENED_RUNTIME=NO \
  CODE_SIGN_INJECT_BASE_ENTITLEMENTS=NO build

APP="$PROJECT_DIR/build/Build/Products/Release/Matra.app"
VERSION=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP/Contents/Info.plist")
DESTINATION="$OUTPUT_DIR/Matra-$VERSION-macOS-arm64-local.dmg"
if [[ -e "$DESTINATION" ]]; then
  echo "Refusing to overwrite $DESTINATION" >&2
  exit 1
fi
codesign --verify --deep --strict "$APP"
STAGING=$(mktemp -d "$OUTPUT_DIR/.matra-dmg.XXXXXX")
ditto "$APP" "$STAGING/Matra.app"
# Strip build-machine debug paths, then re-sign this test bundle without carrying
# forward runtime flags or debugger entitlements from an earlier build.
strip -S "$STAGING/Matra.app/Contents/MacOS/Matra"
printf '%s\n' '<?xml version="1.0"?><plist version="1.0"><dict/></plist>' > "$OUTPUT_DIR/.package-entitlements.plist"
codesign --force --sign - --options 0 --timestamp=none \
  --entitlements "$OUTPUT_DIR/.package-entitlements.plist" "$STAGING/Matra.app"
python3 -B "$PROJECT_DIR/scripts/verify-package-launch.py" \
  "$STAGING/Matra.app" "$OUTPUT_DIR/launch-verification"
ln -s /Applications "$STAGING/Applications"
cp "$PROJECT_DIR/INSTALL-MAC.md" "$STAGING/INSTALL-MAC.md"
hdiutil create -volname "Matra $VERSION" -srcfolder "$STAGING" -format UDZO -fs HFS+ "$DESTINATION"
hdiutil verify "$DESTINATION"
shasum -a 256 "$DESTINATION" > "$DESTINATION.sha256"
echo "Local installer: $DESTINATION"
echo 'Ad-hoc signed. Not Apple notarized. Staging retained for inspection:'
echo "$STAGING"

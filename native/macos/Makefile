# Local-only Mātrā Mac build. No installation, signing-account changes or release.
XCODEGEN ?= xcodegen
PROJECT := Matra.xcodeproj
SCHEME := Matra
ARCH ?= $(shell uname -m)
DEST := platform=macOS,arch=$(ARCH)
PACKAGE_CACHE ?= $(CURDIR)/build/SourcePackages
APP := $(CURDIR)/build/Build/Products/Debug/Matra.app
BUILD_ARGS := -project $(PROJECT) -scheme $(SCHEME) -destination '$(DEST)' \
	-configuration Debug -derivedDataPath build \
	-clonedSourcePackagesDirPath '$(PACKAGE_CACHE)' -onlyUsePackageVersionsFromResolvedFile \
	CODE_SIGN_IDENTITY=- DEVELOPMENT_TEAM= CODE_SIGN_STYLE=Automatic

.PHONY: gen build preview-build test preview preview-local preview-live run install

gen:
	$(XCODEGEN) generate
	mkdir -p $(PROJECT)/project.xcworkspace/xcshareddata/swiftpm
	cp Package.resolved $(PROJECT)/project.xcworkspace/xcshareddata/swiftpm/Package.resolved

build: gen
	xcodebuild $(BUILD_ARGS) build

# A distinct local identity lets native controls target the preview reliably
# while the installed app keeps running. The production target is unchanged.
preview-build: gen
	xcodebuild $(BUILD_ARGS) PRODUCT_BUNDLE_IDENTIFIER=com.dydxfx.matra.mac.preview build

test: gen
	xcodebuild $(BUILD_ARGS) -parallel-testing-enabled NO test

# Sample data only. Never constructs provider readers or changes login startup.
preview: preview-build
	MATRA_DESIGN_PREVIEW=1 "$(APP)/Contents/MacOS/Matra"

# Real local Codex/Claude logs through the existing offline ccusage reader.
# Quota polling, account discovery and update startup stay off, as above.
preview-local: preview-build
	MATRA_DESIGN_PREVIEW=1 MATRA_PREVIEW_DATA=local "$(APP)/Contents/MacOS/Matra"

# Full live data (accounts, quotas, local logs) as the separate preview build,
# with its own settings and storage. Quit the installed Mātrā first; the app
# refuses to start otherwise. Updates, Phone Link and token renewal stay off.
preview-live: preview-build
	MATRA_DESIGN_PREVIEW=1 MATRA_PREVIEW_DATA=live "$(APP)/Contents/MacOS/Matra"

run: build
	"$(APP)/Contents/MacOS/Matra"

install:
	@echo "Installation is separate from design preview. Review README.md first."
	@exit 2

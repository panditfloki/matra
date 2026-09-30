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

.PHONY: gen build test preview run install

gen:
	$(XCODEGEN) generate
	mkdir -p $(PROJECT)/project.xcworkspace/xcshareddata/swiftpm
	cp Package.resolved $(PROJECT)/project.xcworkspace/xcshareddata/swiftpm/Package.resolved

build: gen
	xcodebuild $(BUILD_ARGS) build

test: gen
	xcodebuild $(BUILD_ARGS) -parallel-testing-enabled NO test

# Sample data only. Never constructs provider readers or changes login startup.
preview: build
	MATRA_DESIGN_PREVIEW=1 "$(APP)/Contents/MacOS/Matra"

run: build
	"$(APP)/Contents/MacOS/Matra"

install:
	@echo "Installation is separate from design preview. Review README.md first."
	@exit 2

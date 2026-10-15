# Build and run Ourcraft on the iOS simulator (default: the first iPad).
#   make run              build, install and launch on the simulator
#   make device           build for a real iPhone/iPad (sign in Xcode first)
.PHONY: run install boot-sim xcodebuild-simulator device clean

DEVICE = ${DEVICE_ID}
ifndef DEVICE_ID
	DEVICE=$(shell xcrun simctl list devices available | grep -i 'iPad' | head -n 1 | grep -E -o -i "([0-9a-f]{8}-([0-9a-f]{4}-){3}[0-9a-f]{12})")
endif

run: install
	xcrun simctl launch --console $(DEVICE) org.ourcraft.game

boot-sim:
	xcrun simctl boot $(DEVICE) || true
	open -a Simulator

install: xcodebuild-simulator boot-sim
	xcrun simctl install $(DEVICE) build/Build/Products/Debug-iphonesimulator/ourcraft_mobile.app

xcodebuild-simulator:
	xcodebuild -scheme ourcraft_mobile -configuration Debug -derivedDataPath build -destination "id=$(DEVICE)"

device:
	xcodebuild -scheme ourcraft_mobile -configuration Release -derivedDataPath build -destination 'generic/platform=iOS' -allowProvisioningUpdates

clean:
	rm -rf build

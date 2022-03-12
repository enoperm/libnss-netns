.PHONY: all
all: release

.PHONY: release
release: BUILD_FLAGS = --release
release: build

.PHONY: debug
debug: build

.PHONY: build
build:
	cargo build $(BUILD_FLAGS) -Z unstable-options --out-dir=./build
	-mv build/libnss_netns.so{,.2}
	strip build/libnss_netns.so.2

.PHONY: clean
clean:
	rm -r ./build

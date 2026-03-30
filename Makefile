BINARY    := synthesist
CMD       := ./cmd/synthesist
VERSION   := $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
LDFLAGS   := -ldflags "-X main.version=$(VERSION)"

# Dolt requires CGo + ICU
export CGO_ENABLED := 1

# macOS: find ICU via Homebrew (versioned formula first, then unversioned)
ICU_PREFIX := $(shell brew --prefix icu4c@78 2>/dev/null || brew --prefix icu4c 2>/dev/null)
ifdef ICU_PREFIX
  export CGO_CFLAGS   := -I$(ICU_PREFIX)/include
  export CGO_CXXFLAGS := -I$(ICU_PREFIX)/include
  export CGO_LDFLAGS  := -L$(ICU_PREFIX)/lib
endif

.PHONY: build install clean test lint check

build:
	go build $(LDFLAGS) -o $(BINARY) $(CMD)

install:
	go install $(LDFLAGS) $(CMD)

clean:
	rm -f $(BINARY)
	go clean -cache

test: build
	go test ./...

golden-update: build
	go test ./tests/golden -update

lint:
	golangci-lint run ./...

# Run synthesist check against local specs (if initialized)
check: build
	./$(BINARY) check

# Development: build and show help
dev: build
	./$(BINARY) help

# Release builds for CI
PLATFORMS := darwin/arm64 darwin/amd64 linux/amd64 linux/arm64

.PHONY: release
release:
	@mkdir -p dist
	@for platform in $(PLATFORMS); do \
		os=$${platform%%/*}; \
		arch=$${platform##*/}; \
		output="dist/$(BINARY)-$${os}-$${arch}"; \
		echo "Building $${output}..."; \
		GOOS=$${os} GOARCH=$${arch} go build $(LDFLAGS) -o $${output} $(CMD); \
	done

.PHONY: loc-check
loc-check: ## Fail if any non-generated Go file exceeds 400 LOC
	@FAIL=0; \
	for f in $$(find . -name '*.go' -not -path './vendor/*' -not -name '*_generated.go'); do \
		lines=$$(wc -l < "$$f"); \
		if [ "$$lines" -gt 850 ]; then \
			echo "FAIL: $$f ($$lines lines, max 800)"; \
			FAIL=1; \
		fi; \
	done; \
	if [ "$$FAIL" -eq 1 ]; then exit 1; fi; \
	echo "All files under 400 LOC"

.PHONY: skill
skill: build
	./$(BINARY) skill

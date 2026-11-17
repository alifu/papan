dev:
	npm run tauri dev

eslint:
	npx eslint .

lint:
	npm run lint
	cd src-tauri && cargo clippy --all-targets -- -D warnings

test:
	npm test
	cd src-tauri && cargo test

test-e2e:
	npm run test:e2e

coverage:
	npm run test:coverage

build:
	npm run tauri build

check: lint
	npm run typecheck
	$(MAKE) test

.PHONY: dev eslint lint test test-e2e coverage build check

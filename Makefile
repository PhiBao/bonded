.PHONY: contract-test contract-build api web build smoke setup

contract-test:
	cargo test --manifest-path contracts/bonded/Cargo.toml --features unit-testing

contract-build:
	cargo build --manifest-path contracts/bonded/Cargo.toml --target wasm32-unknown-unknown --release

api:
	cargo run --manifest-path services/api/Cargo.toml

web:
	pnpm --dir apps/web dev

build:
	pnpm --dir apps/web build

smoke:
	./scripts/smoke.sh

setup:
	./scripts/setup-testnet.sh

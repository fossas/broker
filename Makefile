
build:
	@cargo build --release

build-static:
	@cross build --features jemalloc --target=x86_64-unknown-linux-musl --release

dev:
	@cargo build

# make test TEST_FILTER=init:: will run only tests with "init::" in their description
test:
	@cargo nextest run $(TEST_FILTER)
	@cargo test --doc $(TEST_FILTER)

test-static:
	@cross nextest run --features jemalloc --target=x86_64-unknown-linux-musl $(TEST_FILTER)
	@cross test --features jemalloc --target=x86_64-unknown-linux-musl --doc $(TEST_FILTER)

review-snapshots:
	@cargo insta test --test-runner nextest --review

delete-unused-snapshots:
	@cargo insta test --test-runner nextest --unreferenced=delete

generate-dist:
	@cargo dist generate-ci github

run:
	@cargo run -- run

migration-status:
	@cargo sqlx migrate info --source db/migrations

migrate-up:
	@cargo sqlx migrate run --source db/migrations

migrate-down:
	@cargo sqlx migrate revert --source db/migrations

clippy:
	@cargo clippy --all-targets --all-features -- -D warnings

doc:
	@cargo doc --open --no-deps

.PHONY: test test-static run build build-static dev delete-unused-snapshots review-snapshots generate-dist migration-status migrate-up migrate-down doc clippy

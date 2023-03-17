
build:
	@cargo build --release
	
dev:
	@cargo build

test:
	@cargo nextest run
	@cargo test --doc

review-snapshots:
	@cargo insta test --review

generate-dist:
	@cargo dist generate-ci github --installer github-powershell --installer github-shell

run:
	@cargo run -- run

migration-status:
	@cargo sqlx migrate info --source db/migrations

migrate-up:
	@cargo sqlx migrate run --source db/migrations

migrate-down:
	@cargo sqlx migrate revert --source db/migrations

.PHONY: test run build dev review-snapshots generate-dist migration-status migrate-up migrate-down

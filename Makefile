
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

.PHONY: test run build dev review-snapshots generate-dist

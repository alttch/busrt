VERSION=$(shell grep ^version Cargo.toml|cut -d\" -f2)

all:
	@echo select target

test:
	clippy --features server
	clippy --features broker
	clippy --features ipc
	clippy --features rpc
	clippy --features cli
	clippy --features server,rpc

tag:
	git tag -a v${VERSION} -m v${VERSION}
	git push origin --tags

all:
	@echo select target

test:
	clippy --features server
	clippy --features broker
	clippy --features ipc
	clippy --features rpc
	clippy --features cli
	clippy --features server,rpc

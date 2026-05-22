KERNEL   := target/x86_64-unknown-none/debug/amaterasu_kernel
INITRD   := target/initrd.tar
BIOS_IMG := target/amaterasu-bios.img
UEFI_IMG := target/amaterasu-uefi.img

.PHONY: all kernel initrd image run clean test test-unit test-integration

all: image

kernel:
	cargo build --package amaterasu_kernel --target x86_64-unknown-none

initrd: $(INITRD)

$(INITRD): $(shell find initrd -type f)
	tar --format=ustar -cf $@ -C initrd .

image: kernel $(INITRD)
	KERNEL_PATH=$(KERNEL) RAMDISK_PATH=$(INITRD) OUT_DIR=target \
		cargo run --package boot

run: image
	qemu-system-x86_64 \
		-drive format=raw,file=$(BIOS_IMG) \
		-serial stdio \
		-m 128M \
		-no-reboot

test: test-unit test-integration

test-unit:
	cargo test --manifest-path tests/unit/Cargo.toml

test-integration: image
	@echo "=== Integration: boot test (allow ~3 min for SeaBIOS + bootloader) ===" ; \
	if timeout 180 qemu-system-x86_64 \
		-drive format=raw,file=$(BIOS_IMG) \
		-nographic \
		-no-reboot \
		-m 128M \
		2>/dev/null \
	| grep -q '\[BOOT\] kernel_ready'; then \
		echo "PASS: kernel reached ready state"; \
	else \
		echo "FAIL: kernel did not reach ready state"; exit 1; \
	fi

clean:
	cargo clean
	rm -f $(INITRD)

KERNEL    := target/x86_64-unknown-none/debug/amaterasu_kernel
INITRD    := target/initrd.tar
BIOS_IMG  := target/amaterasu-bios.img
UEFI_IMG  := target/amaterasu-uefi.img
OVMF_CODE := /usr/share/OVMF/OVMF_CODE_4M.fd
OVMF_VARS := /usr/share/OVMF/OVMF_VARS_4M.fd
OVMF      := /usr/share/ovmf/OVMF.fd

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
	@if [ ! -f "$(OVMF_CODE)" ] && [ ! -f "$(OVMF)" ]; then \
		echo "ERROR: OVMF firmware not found."; \
		echo "  Ubuntu/Debian: sudo apt install ovmf"; \
		echo "  Fedora/RHEL:   sudo dnf install edk2-ovmf"; \
		echo "  Override:      OVMF_CODE=... OVMF_VARS=...   or   OVMF=..."; \
		exit 1; \
	fi; \
	if [ -f "$(OVMF_CODE)" ] && [ -f "$(OVMF_VARS)" ]; then \
		cp -n $(OVMF_VARS) target/OVMF_VARS.fd 2>/dev/null; true; \
		PFLASH="-drive if=pflash,format=raw,readonly=on,file=$(OVMF_CODE) -drive if=pflash,format=raw,file=target/OVMF_VARS.fd"; \
	else \
		cp -n $(OVMF) target/OVMF_VARS.fd 2>/dev/null; true; \
		PFLASH="-drive if=pflash,format=raw,file=target/OVMF_VARS.fd"; \
	fi; \
	echo "=== Integration: boot test (UEFI/OVMF) ==="; \
	rm -f /tmp/amaterasu-boot.log; \
	timeout 60 qemu-system-x86_64 \
		$$PFLASH \
		-drive format=raw,file=$(UEFI_IMG) \
		-nographic -no-reboot -m 128M \
		2>&1 | tee /tmp/amaterasu-boot.log | grep -m1 '\[BOOT\] kernel_ready'; \
	true; \
	if grep -q 'KERNEL PANIC' /tmp/amaterasu-boot.log 2>/dev/null; then \
		echo "FAIL: kernel panicked"; \
		grep -A3 'KERNEL PANIC' /tmp/amaterasu-boot.log 2>/dev/null; \
		exit 1; \
	elif grep -q '\[BOOT\] kernel_ready' /tmp/amaterasu-boot.log 2>/dev/null; then \
		BOOT_NS=$$(grep 'kernel_ready' /tmp/amaterasu-boot.log | sed 's/.*+\([0-9]*\).*/\1/'); \
		echo "PASS: kernel reached ready state ($${BOOT_NS} ns)"; \
	else \
		echo "FAIL: kernel did not reach ready state"; \
		echo "log: $$(wc -c < /tmp/amaterasu-boot.log 2>/dev/null) bytes"; \
		echo "--- last 20 lines ---"; \
		tail -20 /tmp/amaterasu-boot.log 2>/dev/null; \
		exit 1; \
	fi

clean:
	cargo clean
	rm -f $(INITRD)

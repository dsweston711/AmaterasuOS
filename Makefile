KERNEL   := target/x86_64-unknown-none/debug/amaterasu_kernel
INITRD   := target/initrd.tar
BIOS_IMG := target/amaterasu-bios.img
UEFI_IMG := target/amaterasu-uefi.img

.PHONY: all kernel initrd image run clean

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

clean:
	cargo clean
	rm -f $(INITRD)

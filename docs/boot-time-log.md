# Boot Time Log

Tracking kernel boot time across milestones. All measurements are "kernel entry to last event," not including firmware POST.

**Measurement methodology:** RDTSC read at the top of `kernel_main` as T0, then at each milestone. Deltas converted to nanoseconds assuming a 1 GHz TSC (accurate for QEMU default CPU). See [ADR-002](decisions/ADR-002-boot-time-measurement.md) for full rationale.

| Date | Commit | Milestone | Boot Time | Notes |
|------|--------|-----------|-----------|-------|
| - | - | Initial commit | N/A | Kernel does not boot yet |
| 2026-04-21 | v0.0.1 | First boot: framebuffer paint in QEMU | not measured | No timing instrumentation yet, visual confirmation only. Next: add RDTSC timestamps. |
| 2026-05-20 | 1470137 | First measured boot (QEMU, BIOS, debug build) | **364,501,836 ns (~364 ms)** | Baseline. serial_init: +1,325,180 ns; framebuffer_init: +358,833,570 ns; kernel_ready: +364,501,836 ns. Framebuffer init dominates (~98%). |
| 2026-05-23 | dd07afc | First UEFI/OVMF boot (QEMU, GitHub Actions CI, debug build) | **222,276,503 ns (~222 ms)** | serial_init: +450,717 ns; memory_init: +25,630,908 ns; allocator_init: +27,671,160 ns; acpi_init: +42,283,530 ns; framebuffer_init: +204,170,075 ns; apic_init: +210,550,935 ns (WARN: over 200 ms budget); kernel_ready: +222,276,503 ns. Framebuffer still dominates (~92%). ~39% faster than BIOS baseline — UEFI skips BIOS POST overhead. |
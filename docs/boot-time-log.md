# Boot Time Log

Tracking kernel boot time across milestones. All measurements are "kernel entry to last event," not including firmware POST.

**Measurement methodology:** RDTSC read at the top of `kernel_main` as T0, then at each milestone. Deltas converted to nanoseconds assuming a 1 GHz TSC (accurate for QEMU default CPU). See [ADR-002](decisions/ADR-002-boot-time-measurement.md) for full rationale.

| Date | Commit | Milestone | Boot Time | Notes |
|------|--------|-----------|-----------|-------|
| - | - | Initial commit | N/A | Kernel does not boot yet |
| 2026-04-21 | v0.0.1 | First boot: framebuffer paint in QEMU | not measured | No timing instrumentation yet, visual confirmation only. Next: add RDTSC timestamps. |
| 2026-05-20 | 1470137 | First measured boot (QEMU, BIOS, debug build) | **364,501,836 ns (~364 ms)** | Baseline. serial_init: +1,325,180 ns; framebuffer_init: +358,833,570 ns; kernel_ready: +364,501,836 ns. Framebuffer init dominates (~98%). |
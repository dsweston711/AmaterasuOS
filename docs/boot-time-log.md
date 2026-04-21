# Boot Time Log

Tracking kernel boot time across milestones. All measurements are "kernel entry to last event," not including firmware POST.

| Date | Commit | Milestone | Boot Time | Notes |
|------|--------|-----------|-----------|-------|
| - | - | Initial commit | N/A | Kernel does not boot yet |
| 2026-04-21 | v0.0.1 | First boot: framebuffer paint in QEMU | not measured | No timing instrumentation yet, visual confirmation only. Next: add RDTSC timestamps. |
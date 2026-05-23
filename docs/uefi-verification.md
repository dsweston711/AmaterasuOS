# UEFI Boot Verification — QEMU

Records the first verified end-to-end UEFI boot of AmaterasuOS in QEMU.
Run with `make run-uefi`. All observations are via the serial terminal.

## Environment

| Field | Value |
|-------|-------|
| Date | 2026-05-22 |
| Commit | e5f73ae |
| Host OS | WSL2 (Linux 6.6.114.1-microsoft-standard-WSL2) |
| QEMU version | QEMU emulator version 8.x (Ubuntu) |
| OVMF | /usr/share/OVMF/OVMF_CODE_4M.fd + OVMF_VARS_4M.fd |

## Boot sequence

- [x] OVMF POST completes without error
- [x] Kernel serial output begins (`AmaterasuOS booting...`)
- [x] All `[BOOT]` markers appear in order: `serial_init` → `memory_init` → `allocator_init` → `acpi_init` → `framebuffer_init` → `apic_init` → `kernel_ready`
- [x] Welcome splash displays
- [x] Shell prompt (`amaterasu:/>`) appears

## Shell command verification

Each command tested manually. Pass = expected output produced, no panic.

| Command | Input | Pass |
|---------|-------|------|
| `help` | `help` | ✓ |
| `help <cmd>` | `help ls` | ✓ |
| `echo` | `echo hello world` | ✓ |
| `uname` | `uname -a` | ✓ — `AmaterasuOS 0.1.0 x86_64` |
| `hostname` | `hostname` | ✓ — `AmaterasuOS` |
| `pwd` | `pwd` | ✓ — `/` |
| `ls` | `ls /` | ✓ — `etc/`, `home`, `README` |
| `ls` (path) | `ls /sys` | ✓ — `welcome` |
| `cd` | `cd /sys && pwd` | ✗ — `&&` not parsed; shell passes `/sys && pwd` as literal path |
| `cd` (home) | `cd && pwd` | ✗ — `&&` not parsed; shell passes `&& pwd` as literal path |
| `cat` | `cat /sys/welcome` | ✓ |
| `stat` | `stat /etc/version` | ✓ — path, type, size displayed |
| `head` | `head -n 3 /sys/welcome` | ✓ |
| `tail` | `tail -n 3 /sys/welcome` | ✓ |
| `wc` | `wc -l /sys/welcome` | ✓ |
| `grep` | `grep -i amaterasu /sys/welcome` | ✓ — `AmaterasuOS` |
| `cpu` | `cpu` | ✓ — vendor/brand reported |
| `uptime` | `uptime` | ✓ |
| `heap` | `heap` | ✓ — start address, size, slab stats |
| `export` | `export FOO=bar; echo $FOO` | ✓ — `bar` (using `;` separator) |
| `history` | `history` | ✓ — numbered command list |
| `clear` | `clear` | ✓ |

## Observations

- `&&` is not supported as a command separator. `split_commands` only splits on `;`, so anything after `&&` is passed as a literal argument to the preceding command. Filed as #135.
- `parse_args` (flag parsing layer used by `wc`, `head`, `tail`, `grep`) has no unit tests. All flag-based commands need regression coverage. Filed as #136.
- All other commands behaved correctly on first UEFI/OVMF boot in QEMU.

## Result

PASS (with known issues filed as #135 and #136)

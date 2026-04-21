## AI Usage

AmaterasuOS is developed with the assistance of AI tools (primarily Claude). AI is used for:

- Explaining unfamiliar concepts (UEFI, x86 internals, Rust bare-metal patterns)
- Generating first-draft code for well-understood patterns (boilerplate, standard drivers)
- Reviewing and suggesting improvements on code I've written
- Surveying design tradeoffs before architectural decisions
- Debugging assistance when I'm stuck

All design decisions, architectural choices, performance targets, and final code
that lands in the repo are my responsibility. AI-generated code is read,
understood, and modified before being committed. If I can't explain why it works,
it doesn't ship.

Commits do not individually flag AI involvement, but significant architectural
decisions are documented in `docs/decisions/` with the reasoning behind them,
including where AI input shaped the decision.
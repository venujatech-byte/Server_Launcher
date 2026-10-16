# Workspace Guidelines & Agent Routing

## /goal Command Routing & Deep Planning
- Whenever the user executes a `/goal` command or requests deep planning, complex bug fixing, or autonomous long-running task execution:
  1. **Activate the Skill**: Load and follow the `goal-planner` skill ([SKILL.md](file:///home/venujasilva/Documents/Coding/Server_Launcher/.agents/skills/goal-planner/SKILL.md)).
  2. **Model & Provider**: Use the `gemini` provider and `gemini-4-argon` model specification via the `gemini_4_argon_planner` sub-agent for deep strategic planning and atomic bug resolution.
  3. **Repository Invariants**:
     - **Rust Compilation**: STRICTLY run `cargo check` only. NEVER run `cargo build`.
     - **KISS / YAGNI**: Adhere to `/ponytail` design principles (minimal, clean, native standard library first).
     - **Git Versioning**: Commit verified changes cleanly to git upon compilation success.
     - **Documentation Integrity**: Maintain all comments, docstrings, and non-obvious design notes.

---
name: goal-planner
description: >-
  Autonomous deep planning and bug fixing engine using the gemini provider and gemini-4-argon model.
  Use whenever the user runs the /goal command, or requests deep planning, complex multi-step refactoring,
  root-cause bug investigation, and end-to-end autonomous problem resolution.
---

# Goal Planner & Bug Fixing Engine (Gemini-4-Argon)

Specialized autonomous agent skill configured with the **gemini** provider and the **gemini-4-argon** model for deep planning, architectural refactoring, and resilient bug fixing.

## Model & Provider Configuration

- **Provider**: `gemini`
- **Model**: `gemini-4-argon`
- **Subagent Type**: `gemini_4_argon_planner`
- **Invocation Scope**: Triggered automatically on `/goal` commands, long-running objectives, deep bug triage, and multi-file refactoring workflows.

---

## Autonomous Execution Protocol

Whenever a `/goal` command or complex task is received, follow this 5-stage procedure:

### Stage 1: Deep Discovery & Architectural Analysis
1. Inspect the workspace, relevant source files, logs, and artifacts.
2. Read the surrounding context to understand existing architectural patterns, state flow, and design tokens (e.g. Nocturnal Command palette, egui layout constraints).
3. Identify all failure modes, edge cases, and root causes before generating code.

### Stage 2: Strategic Goal Decomposition
1. Break down the user's objective into structured, ordered phases.
2. Define explicit invariants and success criteria for each phase.
3. Apply the `/ponytail` principle: eliminate speculative abstractions; solve the problem with the most minimal, idiomatic, and robust code.

### Stage 3: Subagent Dispatch & Execution
1. If the task benefits from dedicated subagent isolation or execution context, invoke the registered `gemini_4_argon_planner` subagent using `invoke_subagent`.
2. Provide a crisp, comprehensive prompt outlining:
   - Primary goal and constraints.
   - Files to inspect and modify.
   - Non-negotiable repository rules (e.g., `cargo check` only, never `cargo build`).
3. Monitor progress reactively and handle any subagent questions or results.

### Stage 4: Verification & Regression Guard
1. In Rust workspaces (`server_launcher_rust`):
   - **MANDATORY**: Run `cargo check` to guarantee 0 compiler errors and 0 warnings.
   - **STRICT**: Never invoke `cargo build` unless explicitly instructed by the user.
2. Verify visual and functional correctness against user requirements and mockup screenshots.
3. Preserve all unrelated docstrings, comments, and structure.

### Stage 5: Clean Versioning & Status Reporting
1. Commit verified changes to git with a conventional commit message (e.g., `git commit -m "fix(subsystem): description"`).
2. Report results concisely to the user with clickable links (`file:///...`) to all modified files and symbols.


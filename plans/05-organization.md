# Plan 05: Project Organization and AI-Assisted Development

## Context

Solaya is a single-maintainer RISC-V 64-bit OS kernel aiming for 100% Linux binary compatibility. The kernel currently has ~22,500 lines of Rust, 74 implemented syscalls (out of ~320 on RISC-V), 22 system test files, and 935 total commits. Development is accelerating: 493 commits since January 2025, almost all via AI-assisted PRs through Claude Code.

The maintainer reviews all code via GitHub PRs. Claude Code is the primary "worker" -- it reads issues, writes code, runs tests via the MCP server (QEMU interaction), and creates PRs. The project already has:
- A `CLAUDE.md` with development guidelines
- A `claude-code-review.yml` GitHub Actions workflow for PR code review
- Hook files (doc-check, commit-reminder, question-reminder)
- A `claude.yml` GitHub Action that triggers on `@claude` mentions in issues and PRs
- CI pipeline (build, fmt, clippy, unit-test, miri, system-test)
- 12 documentation files in `doc/ai/` covering every subsystem

The goal: maximize autonomous AI output while the maintainer retains architectural control via PR review. This plan addresses how to organize work, specify tasks, handle parallel agents, and learn from reviews.

---

## 1. Work Organization Options

### Option A: GitHub Issues Only

Issues are the current approach. The 9 open issues range from one-liners ("Implement dynamic linking") to discussion prompts.

**Pros:**
- Standard tooling. `gh` CLI works. Claude Code Action already triggers on `@claude` mentions in issues.
- Labels, milestones, and project boards available.
- External visibility -- contributors can see the backlog.

**Cons:**
- AI agents need rich context to work autonomously. A one-line issue like "Implement CoW for fork" requires the agent to do significant research before coding. This wastes context window.
- No version control on issue text. Edits are invisible.
- Markdown in issues is limited (no front matter, no structured fields for machine parsing).
- Agents cannot easily update issue descriptions mid-flight.

### Option B: Files in Repo Only

Task specs as Markdown files in a `tasks/` directory, version-controlled alongside code.

**Pros:**
- Always available to agents in their working directory. No API calls needed.
- Full version control -- changes tracked in git history.
- Structured front matter possible (YAML headers with status, priority, dependencies).
- Agents can update task files as they work (mark sections complete, add notes).
- Works offline. No GitHub dependency.

**Cons:**
- No built-in notification/assignment system.
- Merge conflicts on task files when multiple agents work in parallel.
- No dashboard view without custom tooling.
- Not the standard way to track work -- unfamiliar to potential contributors.

### Option C: Hybrid (Recommended)

GitHub Issues for high-level tracking and human interaction. Files in repo for detailed task specifications that agents consume.

**Workflow:**
1. Maintainer creates a GitHub issue with a short description and desired outcome.
2. Before assigning to an agent, the maintainer (or a planning agent) creates a task spec file at `tasks/<issue-number>-<slug>.md` with structured details.
3. The issue body links to the task spec file. The task spec file links back to the issue.
4. Agent picks up the task by reading the spec file (always in its working directory).
5. Agent updates the spec file with implementation notes as it works.
6. PR references the issue (`Closes #N`), which auto-closes it on merge.

This combines GitHub's notification/tracking with file-based specs that agents can read without API calls.

### Option D: GitHub Projects

Kanban boards with custom fields and automations.

**Verdict:** Adds overhead without clear benefit for a single-maintainer project. The maintainer already knows the full backlog. Projects shine for team coordination, which is not the bottleneck here. Skip this unless the project grows to multiple human contributors.

### Recommendation

**Use Option C (Hybrid)** with lightweight task spec files. Keep GitHub Issues as the source of truth for "what needs to be done." Use task spec files only when a task needs more context than fits comfortably in an issue body (anything that requires architectural decisions, multiple files, or non-obvious constraints). Simple tasks like "remove clippy lint" or "implement stub syscall" can stay as issues only.

---

## 2. Task Specification Format

### What an AI Agent Needs

An agent starting with a fresh context window needs to understand:

1. **Goal** -- What is the user-visible outcome? What should work after this is done?
2. **Acceptance criteria** -- How do we know it is done? What tests should pass?
3. **Relevant files** -- Where in the codebase to look. Which subsystem docs to read.
4. **Constraints** -- Architectural decisions already made. Things to avoid.
5. **Dependencies** -- What must exist before this task can start.
6. **Scope boundary** -- What is explicitly NOT part of this task.

### Task Spec Template

```markdown
# Task: <Short Title>

Issue: #<number>
Status: open | in-progress | review | done
Priority: critical | high | medium | low
Depends-on: #<number>, #<number>
Subsystem: syscalls | memory | fs | net | processes | drivers | io

## Goal

<1-3 sentences describing the desired outcome from a user's perspective.>

## Acceptance Criteria

- [ ] <Specific, testable criterion>
- [ ] <Specific, testable criterion>
- [ ] System tests pass: `just system-test`
- [ ] Clippy clean: `just clippy`

## Context

<What the agent needs to know. Architecture decisions, relevant Linux behavior,
links to man pages or kernel docs. Keep this concise but sufficient for an agent
to start coding without additional research.>

## Relevant Files

- `kernel/src/syscalls/linux.rs` -- syscall dispatch
- `doc/ai/SYSCALLS.md` -- syscall subsystem docs
- <other files>

## Constraints

- <Architectural constraints, e.g. "must not add third-party crates">
- <Performance requirements>
- <Things to avoid>

## Out of Scope

- <Explicitly excluded work>

## Notes

<Optional: implementation hints, edge cases to watch for, related PRs.>
```

### When to Create a Task Spec File

**Create a file** when:
- The task requires touching 3+ files across different subsystems
- There are non-obvious architectural constraints
- The task has dependencies on other tasks
- The expected implementation is more than ~200 lines

**Skip the file** (issue only) when:
- The task is self-contained (single syscall stub, lint fix, doc update)
- The issue body already has enough context
- The agent can figure it out from CLAUDE.md and the existing docs

### Handling Dependencies

Use the `Depends-on` field in task specs. An agent should check whether dependencies are merged before starting work. For chains of dependent tasks, number them explicitly:

```
tasks/100-vfs-pipe-rework.md      (no dependencies)
tasks/101-epoll-basic.md          (depends on #100)
tasks/102-epoll-edge-trigger.md   (depends on #101)
```

Avoid deep dependency chains. Prefer independent tasks that can run in parallel. If a feature requires 5 sequential steps, consider whether the first 3 can be merged before specifying the last 2.

---

## 3. AI Agent Workflow

### End-to-End Flow

```
1. Maintainer creates issue (+ optional task spec file)
2. Maintainer assigns issue to @claude (or mentions @claude in issue body)
      |
      v
3. Claude Code Action triggers via GitHub Actions (claude.yml)
   - Reads issue body and linked task spec file
   - Reads CLAUDE.md, relevant doc/ai/* files
   - Creates branch from main
      |
      v
4. Agent implements the feature
   - Writes code, runs `just clippy`, runs `just system-test`
   - Uses MCP server to boot QEMU and test interactively
   - Commits incrementally
      |
      v
5. Agent creates PR
   - References issue with "Closes #N"
   - Writes summary and test plan
   - Pushes branch, opens PR via `gh pr create`
      |
      v
6. CI runs automatically (ci.yml)
   - build, fmt, clippy, unit-test, miri, system-test
      |
      v
7. Maintainer reviews PR on GitHub
   - Approves and merges, OR
   - Leaves review comments
      |
      v
8. If review feedback exists:
   - Maintainer comments "@claude please address the review feedback"
   - Claude Code Action triggers again on the PR
   - Agent reads review comments, makes fixes, pushes new commits
   - Repeat from step 6
      |
      v
9. PR merged. Issue auto-closed. Task spec file updated to "done" (if it exists).
```

### Branch Naming

Use descriptive branch names that match the issue:
- `feat/<slug>` for features
- `fix/<slug>` for bug fixes
- `refactor/<slug>` for refactoring

The existing PR history follows this pattern already (e.g., `feat/doom`, `fix-flakiness`, `implement-vfs`).

### Local Agent Workflow (Claude Code CLI)

For the maintainer working interactively with Claude Code on the local machine:

1. Start Claude Code in a worktree: `claude --worktree feat-cow-fork`
2. Tell it which issue to work on: "Implement #177 (CoW for fork)"
3. Agent reads the issue, CLAUDE.md, relevant docs
4. Agent works iteratively, commits incrementally
5. Maintainer can observe progress, redirect, or step away
6. Agent creates PR when done

### Incorporating Review Feedback

The existing `claude.yml` already supports `@claude` mentions in PR review comments and review submissions. The flow is:

1. Maintainer writes review comment: "@claude this should use `assert!` not `debug_assert!` per our policy. Also the error path doesn't handle EINTR."
2. Claude Code Action triggers, reads all review comments on the PR.
3. Agent makes changes, pushes new commits to the same branch.
4. Maintainer re-reviews.

To make this more effective, the review comment should be specific and actionable. The agent works best with:
- Exact file and line references (GitHub's review UI provides this automatically)
- Clear description of what is wrong and what the fix should be
- References to project conventions ("per CLAUDE.md, we prefer...")

### Handling Conflicts

When multiple agents work in parallel, merge conflicts are inevitable. Strategies:

1. **Assign non-overlapping subsystems.** If one agent works on `kernel/src/net/` and another on `kernel/src/memory/`, conflicts are unlikely. This is the primary strategy.

2. **Rebase before merge.** The maintainer can ask "@claude please rebase on main" when a PR has conflicts. The agent will rebase and resolve conflicts.

3. **Sequential merging.** Merge PRs one at a time. After each merge, other open PRs may need rebasing. This is the maintainer's job to coordinate.

4. **Avoid shared files.** `kernel/src/syscalls/linux.rs` is a frequent conflict point because every new syscall modifies it. Group related syscalls into a single PR to minimize conflicts on this file.

---

## 4. Learning from Reviews

### The Problem

When the maintainer leaves review feedback like "don't use `info!()` for debug output" or "this abstraction is unnecessary, inline it," that feedback should permanently change the agent's behavior. Currently, each agent session starts fresh with only CLAUDE.md and doc/ai/* as memory.

### Approach 1: CLAUDE.md as Living Style Guide (Primary)

CLAUDE.md is already the authoritative source of project conventions. It already contains rules like "Prefer less code," "Fail fast with assertions," and "No bloated comments."

**Process:**
1. After reviewing a PR, the maintainer identifies feedback that represents a general pattern (not a one-off fix).
2. The maintainer adds the rule to CLAUDE.md in the "Development Guidelines" section.
3. Every future agent session reads CLAUDE.md first and follows the updated rules.

**Example evolution:**
- Review: "Don't wrap single-use operations in helper functions"
- Already covered by: "Prefer less code. Avoid unnecessary abstractions, helpers for one-time operations."
- If not covered: Add a new guideline to CLAUDE.md.

This is the simplest and most effective approach. CLAUDE.md is version-controlled, always read by agents, and already established in the project. The key discipline is: after every review that reveals a pattern, update CLAUDE.md.

### Approach 2: Review Feedback Log (Supplementary)

Maintain a file `doc/ai/REVIEW-PATTERNS.md` that captures recurring review feedback with examples:

```markdown
## Review Patterns

### Unnecessary Abstractions
**Pattern:** Agent creates a trait or helper function used only once.
**Rule:** Inline single-use logic. Only extract when there are 3+ call sites.
**Example PR:** #175 -- removed DeviceManager trait that wrapped a single HashMap.

### Debug Output Level
**Pattern:** Agent uses `info!()` for debugging messages.
**Rule:** Use `debug!()` for all development-time output. `info!()` is for user-visible messages.
**Example PR:** #180 -- changed info! to debug! in syscall fallback handler.
```

This is supplementary to CLAUDE.md. It provides concrete examples that help agents understand the spirit of a rule, not just the letter. Include this file in the doc-check hook so agents read it.

### Approach 3: Post-Merge Review Agent

Create a subagent (`.claude/agents/review-learning.md`) that runs after the maintainer merges a PR with review comments:

1. Read all review comments on the merged PR.
2. Identify which comments represent general patterns vs. one-off fixes.
3. For general patterns, propose additions to CLAUDE.md or `doc/ai/REVIEW-PATTERNS.md`.
4. Create a PR with the proposed additions for the maintainer to review.

This automates the feedback loop but adds complexity. Start with manual CLAUDE.md updates (Approach 1) and add this later if the volume of reviews warrants it.

### Approach 4: Automated Review Comment Analysis

Periodically (e.g., monthly), run an analysis agent that:
1. Fetches all PR review comments from the last month via `gh api`.
2. Categorizes them: style, architecture, correctness, performance, testing.
3. Identifies the most common categories.
4. Suggests CLAUDE.md updates for the top patterns.

This is valuable for catching patterns the maintainer missed, but it is overkill at the current scale (fewer than 50 PRs merged). Revisit when the project has 200+ merged PRs.

### Recommendation

Start with **Approach 1** (update CLAUDE.md after every review) and **Approach 2** (maintain a review patterns file with concrete examples). These are low-overhead and immediately effective. Add Approach 3 when the volume of reviews makes manual updates burdensome.

---

## 5. Progress Tracking

### Syscall Compliance Dashboard

With ~74 of ~320 RISC-V Linux syscalls implemented, tracking progress is important for motivation and prioritization.

**Recommended approach: A machine-readable status file.**

Create `doc/syscall-status.md` (or `.json`) that lists every RISC-V Linux syscall and its implementation status:

```markdown
| # | Syscall | Status | Notes |
|---|---------|--------|-------|
| 17 | getcwd | done | Full implementation |
| 29 | ioctl | partial | TIOCGWINSZ, TIOCSPGRP, custom extensions |
| 56 | openat | done | Supports O_CREAT, O_DIRECTORY, dirfd |
| 57 | close | done | |
| 61 | getdents64 | done | |
| 62 | lseek | done | |
| 63 | read | done | |
| 64 | write | done | |
| 78 | readlinkat | stub | Returns EINVAL |
| 79 | fstatat | done | |
| 93 | exit | done | |
| 94 | exit_group | done | |
| 96 | set_tid_address | done | |
| ... | ... | ... | ... |
```

Statuses: `done`, `partial`, `stub` (returns hardcoded value), `not-started`.

**Generating the initial list:** Use the RISC-V syscall table from the Linux kernel headers (`include/uapi/asm-generic/unistd.h`) and cross-reference with the `linux_syscalls!` macro in `kernel/src/syscalls/linux.rs`. This can be automated.

**Keeping it updated:** A CI step can verify that the status file matches the actual implementation (grep the macro for implemented syscalls, compare to the status file).

### Linux Test Suite Integration

Issue #190 already tracks integrating Linux test suites. The most relevant are:

- **LTP (Linux Test Project)** -- The standard Linux kernel test suite. Thousands of tests covering syscalls, filesystem, networking, memory management. The gold standard for compliance.
- **strace test suite** -- Tests syscall behavior in detail.
- **musl libc-test** -- Tests libc functions, which exercise underlying syscalls.

Running a subset of LTP and tracking pass/fail counts gives the most meaningful compliance metric. A CI job could run LTP nightly and produce a report:
```
LTP: 142/3000 tests passing (4.7%)
Syscalls: 74/320 implemented (23.1%)
```

### Prioritization

Not all syscalls are equally important. Prioritize by:

1. **What real programs need.** Run `strace` on target programs (dash, coreutils, Doom, gcc) and count which unimplemented syscalls they hit. This is already partly done -- the kernel logs unimplemented syscalls with a backtrace.
2. **What test suites need.** When LTP is integrated, the failing tests reveal the highest-value syscalls to implement next.
3. **Dependency clusters.** Some syscalls are prerequisites for others. `epoll_create1` + `epoll_ctl` + `epoll_wait` form a cluster. Implementing one without the others is useless.
4. **Difficulty vs. impact.** Stub syscalls (return 0) unblock programs quickly. Full implementations take longer but provide correctness.

### Milestones

See 00-overview.md for the canonical milestone list (M1-M10). The key principle: define milestones around concrete capabilities, not syscall counts. Already achieved capabilities include running dash interactively, running Rust coreutils, and running Doom. This is more motivating and measurable than "implement 50 more syscalls."

---

## 6. Parallel AI Work

### Isolation with Git Worktrees

Claude Code has built-in worktree support. Each agent session gets its own working directory and branch:

```bash
# Start an agent in its own worktree
claude --worktree feat-epoll

# Or configure subagents with worktree isolation
# In .claude/agents/implementer.md frontmatter:
# isolation: worktree
```

Worktrees share the git object database but have independent working trees and index. This means:
- No file conflicts between agents during development.
- Each agent can run `just build` and `just system-test` independently.
- Branches are automatically created per worktree.

**QEMU port conflict warning:** System tests use dynamic port allocation, so multiple agents running tests simultaneously should work. However, the MCP server's `boot_qemu` runs a single QEMU instance. Parallel agents using the MCP server would conflict. For parallel work, agents should use `just system-test` (which manages its own QEMU instances) rather than the MCP server.

### Subsystem Partitioning

The most practical approach to parallel work is assigning agents to non-overlapping subsystems:

| Agent | Subsystem | Key Files |
|-------|-----------|-----------|
| A | Network (TCP, epoll) | `kernel/src/net/`, `kernel/src/syscalls/net_ops.rs` |
| B | Filesystem (ext2 write, symlinks) | `kernel/src/fs/`, `kernel/src/syscalls/fs_ops.rs` |
| C | Memory (CoW fork, mmap improvements) | `kernel/src/memory/`, `kernel/src/syscalls/mm_ops.rs` |
| D | Process (thread groups, ptrace) | `kernel/src/processes/`, `kernel/src/syscalls/process_ops.rs` |

The main conflict point is `kernel/src/syscalls/linux.rs`, which every new syscall must modify. Mitigations:
- Group related syscalls into a single PR (e.g., all epoll syscalls together).
- Merge PRs sequentially for tasks that touch this file.
- The maintainer resolves conflicts during merge (usually trivial -- adding lines to the macro invocation).

### Practical Limits on Parallelism

**2-3 parallel agents is the sweet spot** for a single-maintainer project. Reasons:

1. **Review bandwidth is the bottleneck.** The maintainer must review every PR. With 3 agents producing PRs, that is a steady stream of reviews. More agents means PRs queue up unreviewed.
2. **Merge coordination.** Each merge potentially creates conflicts in other open PRs. With 2-3 PRs open, this is manageable. With 10, it becomes a full-time job.
3. **CI throughput.** The self-hosted CI runner has `concurrency: group: ci, cancel-in-progress: false`, meaning CI jobs queue. 3 parallel PRs means 3 CI runs queued.
4. **Context cost.** Each agent costs money per token. Idle agents waiting for review feedback still consume tokens when they resume. Keeping the pipeline full without overflow maximizes value.

**When to increase parallelism:**
- When the maintainer can dedicate a full day to reviews (batch review mode).
- When tasks are in completely independent subsystems with no shared files.
- When CI is fast enough to not be a bottleneck (currently ~5 minutes per run on self-hosted).

### Coordination Between Agents

Agents do not directly communicate. Coordination happens through:

1. **Git history.** An agent starting work on a branch based on `main` sees all previously merged work.
2. **Task spec files.** An agent can read task specs to understand dependencies and avoid duplicating work.
3. **The maintainer.** The maintainer is the coordinator. They decide which tasks to assign, in what order, and when to hold a task until a dependency merges.

No sophisticated multi-agent orchestration framework is needed. The maintainer is the orchestrator.

---

## 7. Recommended Approach

### Concrete Setup (Immediate)

1. **Keep GitHub Issues for tracking.** Continue creating issues for work items. Add labels for subsystem (`net`, `fs`, `memory`, `process`, `driver`, `infra`) and type (`feature`, `bug`, `refactor`, `research`).

2. **Add task spec files for complex work.** For any task that needs more than a paragraph of context, create `tasks/<issue-number>-<slug>.md` using the template from Section 2. Simple tasks stay as issues only.

3. **Refine the Claude Code Action workflow.** The current `claude.yml` triggers on `@claude` mentions. Extend it to also trigger on issue assignment to a `claude-bot` user or a specific label like `agent-ready`:

   ```yaml
   on:
     issues:
       types: [opened, assigned, labeled]
   jobs:
     claude:
       if: |
         (github.event_name == 'issues' && github.event.label.name == 'agent-ready') ||
         ...existing conditions...
   ```

   This lets the maintainer prepare an issue with full context, then apply the `agent-ready` label to trigger the agent.

4. **Update CLAUDE.md after every review.** Make this a habit. If you leave review feedback that could apply to future PRs, update CLAUDE.md before merging. This is the single most impactful action for improving agent output quality over time.

5. **Create `doc/ai/REVIEW-PATTERNS.md`.** Start capturing review patterns with concrete examples from real PRs. Include it in the doc-check hook.

### Concrete Setup (Next Month)

6. **Build a syscall status tracker.** Generate `doc/syscall-status.md` from the RISC-V syscall table and current implementation. Add a CI step that verifies it stays in sync.

7. **Integrate LTP subset.** Pick 50-100 LTP tests targeting implemented syscalls. Run them in CI and track pass rate. This gives an objective compliance metric.

8. **Try parallel agents.** Run 2 agents on independent subsystems using `claude --worktree`. Evaluate whether review bandwidth is the bottleneck. Adjust based on experience.

### Concrete Setup (Quarter)

9. **Add a review-learning agent.** When the volume of PRs makes manual CLAUDE.md updates burdensome, create a `.claude/agents/review-learning.md` subagent that analyzes merged PR comments and proposes guideline updates.

10. **Define milestone M4-M8.** Break each milestone into specific issues and task specs. This creates a clear roadmap that agents can execute against.

### What NOT to Do

- **Do not build custom orchestration tooling.** The maintainer is the orchestrator. `gh` CLI + Claude Code Action + git worktrees is sufficient infrastructure.
- **Do not create a complex project management system.** GitHub Issues + optional task spec files is enough. The overhead of maintaining a Kanban board or custom dashboard is not worth it at this scale.
- **Do not run more than 3 parallel agents** until review bandwidth is proven to not be a bottleneck.
- **Do not optimize for agent efficiency at the cost of review quality.** The maintainer's review is the quality gate. Every shortcut that reduces review thoroughness degrades the codebase.
- **Do not track progress by syscall count alone.** Track by capability milestones (what programs can run) and LTP pass rate. Syscall count is a vanity metric -- 20 well-implemented syscalls that let Doom run are more valuable than 100 stubs.

---

## Sources

- [Best Practices for Claude Code](https://code.claude.com/docs/en/best-practices) -- Official Claude Code documentation
- [Effective Harnesses for Long-Running Agents](https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents) -- Anthropic engineering blog on multi-session agents and progress files
- [Claude Code GitHub Actions](https://code.claude.com/docs/en/github-actions) -- Official docs on GitHub integration
- [Claude Code Worktrees: Run Parallel Sessions Without Conflicts](https://claudefa.st/blog/guide/development/worktree-guide) -- Worktree isolation guide
- [WRAP Up Your Backlog with GitHub Copilot Coding Agent](https://github.blog/ai-and-ml/github-copilot/wrap-up-your-backlog-with-github-copilot-coding-agent/) -- GitHub's issue-to-PR agent workflow pattern
- [Scaling Open Source Code Review with AI](https://pydantic.dev/articles/scaling-open-source-with-ai) -- Pydantic's approach to AI-assisted PR review at scale
- [The Complete Guide to AI Agent Memory Files](https://hackernoon.com/the-complete-guide-to-ai-agent-memory-files-claudemd-agentsmd-and-beyond) -- CLAUDE.md and memory patterns
- [Claude Code Spec Workflow](https://github.com/Pimzino/claude-code-spec-workflow) -- Spec-driven development workflow for Claude Code
- [Common Workflows - Claude Code Docs](https://code.claude.com/docs/en/common-workflows) -- Official workflow patterns
- [Systrack - Linux Kernel Syscall Implementation Tracker](https://github.com/mebeim/systrack) -- Tool for tracking syscall implementation across architectures

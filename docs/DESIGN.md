# Driftr

## Project Plan: Automatic Versioning and Change Management for AI-Assisted Development

**Project name:** Driftr
**CLI binary:** `driftr`
**Package:** `driftr` (crates.io availability confirmed by project owner)
**Language:** Rust
**Status:** Exploration and initial technical planning

**Vision:** Make AI-assisted development safer and more productive by automatically capturing the evolution of work, organizing changes around tasks and agent sessions, and making experimentation and recovery effortless.

**Initial strategy:** Start with automatic Git workflow optimization for Markdown and other text-heavy files. Evolve toward an agent-aware change-management layer that supports experimentation, recovery, and coordination across AI coding sessions.

---

# 1. Executive Summary

AI coding agents have changed how software is developed. Rather than manually editing individual files and making deliberate commits after completing a unit of work, developers increasingly delegate large, iterative tasks to agents.

An agent might modify dozens of files, run tests, make corrections, try alternative approaches, and repeat this cycle several times before producing a satisfactory result.

Traditional Git remains essential, but its interaction model was designed around deliberate human-controlled checkpoints:

* Stage changes.
* Write commit messages.
* Commit.
* Create branches.
* Merge completed work.
* Revert or reset when necessary.

These mechanics introduce friction into a workflow where changes happen continuously and sometimes autonomously.

Driftr explores a different model.

Instead of requiring users and agents to explicitly manage every checkpoint, Driftr continuously observes work, automatically preserves recoverable states, and organizes the evolution of changes into meaningful units.

It starts by making Git nearly invisible for ordinary document editing. Over time, it can become an infrastructure layer that gives AI agents the freedom to experiment while maintaining reliable recovery and clean, intentional Git history.

## Core product principles

1. **Automatic:** Capture changes without requiring manual staging or committing.
2. **Recoverable:** Make it cheap and reliable to return to previous states.
3. **Session-aware:** Understand which agent or person is responsible for changes.
4. **Task-oriented:** Organize history around actual work rather than arbitrary time intervals.
5. **Git-compatible:** Preserve standard Git as the durable, interoperable representation of completed work.
6. **Lightweight:** Remain a small, fast, local-first Rust tool.
7. **Transparent:** Work with ordinary files, editors, and agents without imposing a proprietary environment.
8. **Agent-friendly:** Provide safe, predictable interfaces that agents can use directly.
9. **Progressive:** Introduce complexity only when demonstrated use cases justify it.

The initial product should solve a concrete problem extremely well: automatic, reliable versioning of files without manual Git operations.

The long-term product should address a more ambitious problem: how to make autonomous and semi-autonomous development safe enough that agents can work more freely.

---

# 2. Product Vision

## 2.1 The immediate problem

AI tools produce a growing amount of valuable work:

* Markdown documents
* Claude Code skills and instructions
* Project plans and specifications
* Source code
* Tests
* Configuration
* Scripts
* Generated artifacts
* Refactoring experiments

These files evolve through continuous iterations.

Developers often want the ability to:

* Recover a previous version.
* Understand how a document or implementation evolved.
* Compare alternative approaches.
* Undo an unwanted rewrite.
* Preserve a useful intermediate state.
* Avoid committing every small change.
* Maintain clean Git history without losing the details of development.

Git already has much of the underlying machinery, but it requires users to impose a deliberate structure on work that increasingly happens continuously.

Driftr removes that burden.

## 2.2 The long-term opportunity

The bigger opportunity is to create a reliable change-management layer for AI development.

Consider an AI agent implementing a complex feature.

It might:

1. Inspect a codebase.
2. Establish a baseline.
3. Implement an initial approach.
4. Modify multiple files.
5. Run tests.
6. Diagnose failures.
7. Experiment with a different design.
8. Revert some changes.
9. Refine the successful approach.
10. Deliver a completed implementation.

Today, the agent must operate within the constraints of the available filesystem and Git workflow. Recovery can be cumbersome, and concurrent agents introduce additional risk.

Driftr could provide a continuous, recoverable workspace where the agent can:

* Create checkpoints automatically.
* Explore alternative implementations.
* Compare experiments.
* Restore known-good states.
* Track changes by task and session.
* Consolidate successful work into conventional Git commits.

The goal isn't to replace Git. It's to make the development process leading up to a Git commit substantially safer and more flexible.

## 2.3 Product positioning

**Driftr: Let your code evolve. Keep every step.**

An automatic, AI-aware change-history and recovery layer for modern development.

The initial product is an automated Git workflow. The eventual product may become part of the infrastructure that enables safer, more autonomous coding agents.

---

# 3. Product Architecture: Three Layers of History

One of the most important architectural insights is that working history, agent sessions, and Git history represent different concepts.

They should be modeled separately, even if the initial implementation uses Git for all three.

## 3.1 Working history

Working history is the continuous record of changes as work progresses.

Its characteristics:

* Automatic capture
* Fine-grained checkpoints
* Low-friction recovery
* Temporary experiments
* Potentially disposable history
* No requirement for polished commit messages

Working history prioritizes recoverability and development flexibility over presentation.

It should capture useful intermediate states without forcing every intermediate state into permanent Git history.

## 3.2 Agent sessions

Sessions represent units of activity by an agent or human.

A session may include:

* Agent identity
* Session identifier
* Start and end times
* Task or prompt context, when available
* Files accessed or modified
* Tool invocation history, when available
* Associated snapshots
* Validation results
* Final outcome

Sessions provide attribution and context that Git commits alone cannot express.

A session is not necessarily equivalent to a commit, branch, or task. A single session may produce several meaningful milestones, and a milestone may combine work from multiple sessions.

## 3.3 Git history

Git history is the durable, interoperable record of intentional work.

It should contain:

* Meaningful milestones
* Human-readable commit messages
* Completed or deliberately preserved changes
* Standard Git objects and metadata
* Conventional branch and merge relationships

Git remains the source of truth for committed work.

Driftr manages the transition from messy working history to deliberate, durable Git history.

## 3.4 Conceptual architecture

```text
               Human / AI agents
                       |
                       v
               Filesystem activity
                       |
             +---------+---------+
             |                   |
             v                   v
       Filesystem watcher   Agent hooks
             |                   |
             v                   v
       Change detection     Session events
             |                   |
             +---------+---------+
                       |
                       v
              Attribution layer
                       |
                       v
             Working-history engine
                       |
           +-----------+-----------+
           |                       |
           v                       v
       Snapshots              Session model
           |                       |
           +-----------+-----------+
                       |
                       v
              Change management
                       |
           +-----------+-----------+
           |                       |
           v                       v
      Recovery / diff       Consolidation
                                   |
                                   v
                             Git history
```

This diagram describes the eventual system. The MVP should implement only the components necessary for reliable automatic versioning.

---

# 4. Storage Strategy: Git First, Separate History If Necessary

A critical technical question is whether Driftr needs its own snapshot storage or whether Git's existing object model can support the entire workflow.

Avoid introducing a separate database or version-control implementation without demonstrating a concrete need.

## 4.1 Approach A: Temporary Git commits

Automatically create Git commits as changes settle.

At the end of a task:

* Keep the successful result.
* Squash intermediate commits.
* Drop unsuccessful experiments.
* Produce one or more deliberate milestone commits.

Advantages:

* Minimal new storage machinery.
* Existing Git diff, restore, and history operations.
* Fast path to a working prototype.
* Excellent compatibility with standard tooling.

Disadvantages:

* Shared repository history can become noisy.
* Rewriting history can be disruptive.
* Temporary commits still occupy Git object storage until garbage collection.
* Concurrent agents may interfere with the shared working tree and Git index.
* Git commits do not inherently represent session ownership or task state.

This approach should be the first implementation experiment.

## 4.2 Approach B: Independent working-history storage

Capture temporary snapshots in a separate local store.

Possible implementation:

* Content-addressed blobs
* Snapshot manifests
* SQLite metadata index
* Append-only session and checkpoint records
* Optional Git export or consolidation

Advantages:

* Independent lifecycle for temporary history
* No need to rewrite existing Git commits during experimentation
* Better control over checkpoint granularity
* Potentially easier isolation of concurrent sessions

Disadvantages:

* More storage and recovery logic
* More complex reconciliation with the working tree
* New durability and corruption concerns
* Risk of rebuilding functionality already provided by Git

This approach should be considered only after the Git-based prototype exposes concrete limitations.

## 4.3 A possible hybrid

A future architecture could use Git as the content-addressed storage foundation while keeping temporary snapshot references outside permanent Git history.

For example:

* Working snapshots reference Git tree or blob objects.
* Session metadata records ownership and lifecycle.
* Successful snapshots become conventional commits.
* Unneeded temporary references are released for eventual garbage collection.

This may offer much of the flexibility of separate storage without implementing a completely independent object store.

It is an architectural hypothesis to validate, not an assumed design decision.

## 4.4 Storage decision criteria

Evaluate the alternatives against:

| Criterion          | What to measure                                                              |
| ------------------ | ---------------------------------------------------------------------------- |
| Reliability        | Can every acknowledged checkpoint be recovered?                              |
| Isolation          | Can concurrent agents maintain independent histories?                        |
| Performance        | What is the overhead of frequent snapshots?                                  |
| Storage efficiency | How much additional disk space does fine-grained history consume?            |
| Git compatibility  | Can successful work be preserved as normal commits?                          |
| Complexity         | How much code and operational behavior must Driftr own?                      |
| Recovery           | Can the system restore individual files or complete working states reliably? |
| Crash recovery     | What happens if the tool or machine crashes during a checkpoint?             |

Do not choose a separate snapshot store simply because it appears more architecturally elegant.

The simplest architecture that satisfies the product requirements should win.

---

# 5. Change Detection and Automatic Checkpointing

Automatic change detection is the foundation of the product.

## 5.1 Filesystem watcher

Use a cross-platform Rust filesystem notification library, such as `notify`, to observe file changes.

Target platforms:

* macOS: FSEvents
* Linux: inotify
* Windows: ReadDirectoryChangesW

The watcher should support:

* Recursive directory monitoring
* File creation and deletion
* Modification and renaming
* Newly created directories
* Event coalescing
* Ignore patterns
* Periodic reconciliation
* Recovery from missed filesystem notifications

Filesystem events are not authoritative records of content changes. The watcher should verify file state before creating a snapshot.

## 5.2 Automatic tracking

The initial version should focus on Markdown and ordinary text files.

Example configuration:

```toml
[tracking]
include = ["**/*.md"]
exclude = ["**/.git/**", "**/node_modules/**"]

[checkpoint]
idle_seconds = 30
max_interval_seconds = 300
```

The user should be able to point Driftr at a directory and have newly created files tracked automatically.

The system should not require manual `git add` or explicit registration of every file.

Expand supported file types based on real usage rather than restricting the architecture to Markdown.

## 5.3 Checkpoint strategy

A basic checkpoint lifecycle:

1. Detect a meaningful file change.
2. Start or refresh an editing session.
3. Collect subsequent changes.
4. Wait for a configurable idle period.
5. Verify that the working tree is stable.
6. Create a recoverable snapshot.
7. Associate it with the relevant session when possible.
8. Resume monitoring.

A maximum checkpoint interval protects against continuous activity.

For the initial implementation, a 30-second idle period and five-minute maximum interval are reasonable experimental defaults.

## 5.4 Checkpoint granularity

There is an important distinction between:

* Capturing every individual file write
* Capturing every settled editing session
* Capturing every logical task milestone

These represent different tradeoffs.

The MVP should begin with idle-based grouping. More advanced task-aware grouping can be introduced after session attribution is available.

Avoid assuming that every pause in activity represents the end of a logical task.

---

# 6. Agent Integration and Session Awareness

Agent integration is a major differentiator and an important direction for future development.

The filesystem watcher can identify what changed. Agent hooks can provide context about who is working and what activity caused the changes.

Together, they can improve attribution, grouping, and safety.

## 6.1 Claude Code integration

Start with optional Claude Code hooks.

Potential events to integrate:

* Session start
* Session end
* User prompt submission
* Tool invocation
* Tool completion
* Stop or task completion
* Subagent lifecycle events, where supported

The exact integration should follow the available Claude Code hook contracts and be validated against real workflows.

Driftr should remain independent of Claude Code. Hooks are an optional source of additional information, not a prerequisite for automatic tracking.

## 6.2 Session model

A conceptual session record might contain:

```json
{
  "session_id": "session-123",
  "agent": "claude-code",
  "started_at": "2026-10-04T10:00:00Z",
  "ended_at": null,
  "task": "Refactor query planner",
  "repository": "/workspace/project",
  "checkpoints": [],
  "files": [],
  "status": "active"
}
```

This is an illustrative schema. The implementation should use stable identifiers and the actual event data available from integrations.

A session should not require complete metadata to be useful.

## 6.3 File attribution

Combine signals from:

* Filesystem notifications
* Agent tool invocations
* Session identifiers
* File paths and repository identity
* Time intervals
* Explicit agent declarations, when available

Attribution confidence should be represented honestly.

For example:

* A file modified directly by an agent tool invocation has strong attribution evidence.
* A file modified during a session without a corresponding tool event has weaker evidence.
* A file changed simultaneously by multiple actors may have ambiguous ownership.

Do not silently assign every file change to the most recently active agent.

## 6.4 Multiple agents

Concurrent agents introduce important new requirements.

Consider two agents working in one repository:

* Agent A is implementing a database migration.
* Agent B is updating documentation.
* Both edit files in overlapping directories.
* Agent A completes while Agent B is still working.

Driftr should eventually be able to distinguish:

* Independent changes
* Shared files
* Conflicting modifications
* Session-specific snapshots
* Work that can safely be consolidated independently

The initial implementation should detect and report ambiguity rather than pretend it can resolve every conflict automatically.

## 6.5 Agent-facing interface

Eventually, Driftr should expose a small, predictable command interface for agents.

Potential operations:

```bash
driftr checkpoint "before refactor"
driftr history --session <id>
driftr diff --checkpoint <id>
driftr restore --checkpoint <id>
driftr milestone create "refactor complete"
```

These are conceptual examples.

A future MCP server could provide structured operations to agents, but the CLI should remain the initial integration surface.

The interface should be designed for deterministic, scriptable behavior:

* Clear success and failure results
* Stable identifiers
* Explicit destructive-operation safeguards
* No unnecessary interactive prompts
* Reliable behavior when called repeatedly

---

# 7. Experimentation and Recovery

This is where Driftr can evolve beyond automatic Git commits.

The goal is to make experimentation inexpensive and recovery predictable.

## 7.1 Checkpoint before risky operations

Agents and developers should be able to establish a known-good state before a substantial change.

Examples:

* Before a large refactor
* Before changing a database schema
* Before rewriting a complex module
* Before an automated migration
* Before trying an alternative implementation

Automatic checkpointing should handle routine work. Explicit checkpoints provide additional protection around meaningful operations.

## 7.2 Experiments

A future experiment abstraction could represent a bounded attempt to solve a problem.

An experiment might have:

* A starting checkpoint
* A session or agent association
* A sequence of snapshots
* Validation results
* A final outcome
* A decision to preserve or discard the result

An agent could compare two approaches without requiring both to become permanent branches or commits.

This may eventually enable safer autonomous experimentation.

However, experimentation should not become a separate branching framework prematurely. First establish whether existing Git worktrees, temporary commits, and snapshots are sufficient.

## 7.3 Recovery

Recovery should work at several levels:

**File recovery**

* Restore an individual file to an earlier state.
* Inspect deleted files.
* Compare historical versions.

**Checkpoint recovery**

* Return the working tree to a known snapshot.
* Preserve unrelated work where possible.

**Session recovery**

* Discard or restore changes associated with a particular session.
* Recover a session's last known-good state.

**Experiment recovery**

* Return to the experiment's starting point.
* Keep a successful intermediate result.
* Discard unsuccessful work.

The system must distinguish between restoring a snapshot and rewriting Git history.

Restoration should be explicit, reviewable, and safe in the presence of unrelated modifications.

---

# 8. Consolidation and Git Integration

Git is the long-term compatibility boundary.

Driftr should preserve standard Git behavior and avoid forcing users into a proprietary repository format.

## 8.1 Automatic Git workflow

For the initial product:

* Initialize repositories when requested.
* Detect and reuse existing repositories.
* Create identifiable automatic commits.
* Preserve standard Git metadata.
* Respect ignore rules and configured tracking scope.
* Provide history, diff, and restore commands.

## 8.2 Consolidation

The long-term workflow should distinguish temporary working history from durable Git history.

Potential operations:

* Consolidate successful snapshots into one commit.
* Preserve several meaningful milestones.
* Squash temporary commits.
* Drop unsuccessful experiments.
* Retain annotations and session metadata.
* Produce clean commit messages.

Consolidation must not rewrite published or shared history without explicit authorization.

The system should also recognize that not every task should result in exactly one commit. Some work may be better represented by several independently useful milestones.

## 8.3 Commit descriptions and annotations

Initially, generate deterministic commit messages based on file changes.

Examples:

* `Update architecture notes`
* `Add query planner tests`
* `Revise Claude skill instructions`

Later, optional AI summaries can incorporate task and session context.

Annotations may include:

* User descriptions
* Tags
* Session identifiers
* AI-generated summaries
* Validation results
* References to relevant tasks or conversations

Git notes are a possible storage mechanism for additional metadata, but should not be required for basic functionality.

## 8.4 Existing Git workflow compatibility

Important safeguards:

* Never silently overwrite user-staged changes.
* Detect active merges, rebases, and cherry-picks.
* Avoid unexpected branch changes.
* Handle concurrent Git processes safely.
* Make automatic commits identifiable.
* Provide pause and resume controls.
* Keep recovery possible through ordinary Git commands.

The shared Git index is a particularly important concern. Investigate temporary indexes or other safe staging strategies rather than assuming the watcher owns the repository's entire staging area.

---

# 9. CLI and User Experience

The CLI should be the primary interface for the initial release.

It should feel like a lightweight utility, not a new development environment.

## 9.1 Proposed commands

| Command             | Purpose                                    |
| ------------------- | ------------------------------------------ |
| `driftr init`       | Initialize or configure automatic tracking |
| `driftr watch`      | Start the filesystem watcher               |
| `driftr status`     | Show tracking and session status           |
| `driftr history`    | Browse checkpoints                         |
| `driftr diff`       | Compare historical states                  |
| `driftr restore`    | Restore a previous version                 |
| `driftr checkpoint` | Create an explicit checkpoint              |
| `driftr annotate`   | Add a description or tag                   |
| `driftr pause`      | Pause automatic tracking                   |
| `driftr resume`     | Resume tracking                            |
| `driftr session`    | Inspect agent sessions                     |
| `driftr milestone`  | Preserve a deliberate Git milestone        |

The MVP should implement only the commands necessary to initialize tracking, run the watcher, inspect history, and recover files.

## 9.2 Example initial workflow

```bash
driftr init
driftr watch
```

Then edit files normally.

Driftr observes changes and creates checkpoints automatically.

When needed:

```bash
driftr history README.md
driftr diff README.md
driftr restore README.md --revision <id>
```

A user should be able to get meaningful value without learning Git internals.

## 9.3 Background operation

A background service or startup integration could eventually make tracking completely invisible.

Potential behavior:

* Start automatically after initialization.
* Monitor configured directories.
* Persist configuration across restarts.
* Recover from crashes.
* Provide a simple way to inspect or stop the service.

The first prototype should use a foreground watcher. Background-service management introduces platform-specific complexity and should follow validation of the core behavior.

---

# 10. Technical Architecture

## 10.1 Language and dependencies

Rust is the preferred implementation language.

It offers:

* Small, portable executables
* Low resource consumption
* Good filesystem notification support
* Reliable subprocess management
* Strong CLI ecosystem
* A practical path to background services and integrations

Potential dependencies to evaluate:

| Dependency             | Purpose                                 |
| ---------------------- | --------------------------------------- |
| `clap`                 | CLI parsing                             |
| `notify`               | Filesystem watching                     |
| `ignore`               | Ignore patterns and directory traversal |
| `serde`                | Serialization                           |
| `toml`                 | Configuration                           |
| `anyhow` / `thiserror` | Error handling                          |
| `tempfile`             | Integration testing                     |
| `rusqlite`             | Optional future metadata index          |
| `tokio`                | Optional asynchronous orchestration     |

Avoid introducing a complex asynchronous runtime or database until the architecture demonstrates a need.

## 10.2 Git adapter

Use the Git CLI initially.

Build a small internal adapter for:

* Repository discovery
* Status inspection
* Diff operations
* Commit creation
* Revision lookup
* Restoration
* Branch and operation-state detection

Subprocess execution provides compatibility with the installed Git version and keeps the implementation relatively simple.

Consider libgit2 only when concrete performance or portability requirements justify the added dependency.

## 10.3 Core modules

A possible initial code structure:

```text
driftr/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── config.rs
│   ├── git.rs
│   ├── watcher.rs
│   ├── checkpoint.rs
│   ├── history.rs
│   ├── restore.rs
│   └── session.rs
├── tests/
│   ├── git_integration.rs
│   ├── watcher_integration.rs
│   └── recovery_integration.rs
└── README.md
```

The initial modules should remain cohesive and small. Session handling can begin as simple metadata attached to checkpoints rather than a sophisticated event-processing subsystem.

## 10.4 Metadata and persistence

Git should remain authoritative for committed file content.

Initially, persist only the minimum additional state needed for:

* Configuration
* Watcher state
* Checkpoint metadata, if not fully represented in Git
* Session identifiers and attribution
* Optional annotations

A local SQLite database may eventually be useful for querying sessions and events efficiently.

It should be rebuildable or recoverable wherever practical. It should not become a second authoritative version-control system without a compelling reason.

## 10.5 Reliability

Reliability is a defining product requirement.

The system should handle:

* Rapid successive writes
* Atomic editor file replacements
* Missed filesystem notifications
* Concurrent Git operations
* Concurrent agent modifications
* Watcher crashes
* Machine restarts
* Interrupted checkpoints
* Large directories
* Excluded and ignored files
* Manual edits during automatic tracking

An acknowledged checkpoint should be durable and recoverable.

Where attribution or restoration is ambiguous, the system should report uncertainty rather than silently discard or misclassify work.

---

# 11. Development Roadmap

The roadmap is deliberately staged. Each phase validates a core hypothesis before adding another layer of complexity.

## Phase 1: Automatic Git for documents

**Goal:** Demonstrate reliable, invisible versioning for Markdown files.

Implement:

* Rust CLI
* Repository detection and initialization
* Foreground watcher
* Markdown-focused tracking
* Idle-based checkpointing
* Deterministic commit messages
* Basic history inspection
* Single-file recovery
* Safe handling of manual Git activity

Start with a simple polling prototype if that shortens implementation time. Replace it with filesystem notifications once the behavior is understood.

**Success criteria:**

* Documents are tracked without manual commits.
* New files are automatically included.
* Rapid edits are grouped into sensible checkpoints.
* Previous versions can be restored.
* Existing Git workflows are not unexpectedly disrupted.

This phase establishes the initial product and provides a useful personal tool.

## Phase 2: Improve the recovery and history experience

**Goal:** Make automatic history understandable and useful.

Implement:

* Readable timeline
* Per-file history
* Revision comparison
* Better restore operations
* Deleted-file recovery
* Configurable tracking patterns
* Pause and resume
* Improved watcher reliability

**Success criteria:**

* A developer can recover from common mistakes without manually operating Git.
* History remains useful during frequent AI-assisted edits.
* The tool can run continuously with minimal resource usage.

## Phase 3: Evaluate working-history architectures

**Goal:** Determine whether temporary Git commits are sufficient or a separate working-history layer is justified.

Build competing prototypes or focused experiments:

* Temporary Git commits with later squashing
* Snapshot references and temporary metadata outside durable Git history
* Hybrid approaches using Git objects with separate checkpoint references

Evaluate:

* Checkpoint creation cost
* Disk growth
* Recovery performance
* Consolidation complexity
* Concurrent agent behavior
* Shared Git workflow interference

**Success criteria:**

* A documented architecture decision based on measured behavior.
* A clear understanding of the limitations of the selected approach.
* No unnecessary custom version-control implementation.

This is an important decision gate. Avoid prematurely committing to a separate snapshot store.

## Phase 4: Claude Code session integration

**Goal:** Associate changes with the agent sessions that produce them.

Implement:

* Optional Claude Code hooks
* Session start and end events
* Tool activity observation where useful
* Session identifiers
* File attribution
* Session-aware history
* Session-associated checkpoints

Test:

* Single-agent workflows
* Multiple sessions in one repository
* Human edits alongside agent activity
* Subagent activity
* Sessions that end unexpectedly
* Changes that cannot be confidently attributed

**Success criteria:**

* Session attribution is useful and accurate enough to improve history.
* Uncertainty is represented explicitly.
* Core versioning continues working without hooks.

## Phase 5: Task-aware history and consolidation

**Goal:** Move from time-based checkpoints toward task-oriented history.

Implement:

* Session grouping
* Explicit milestones
* Checkpoint annotations
* Optional AI-generated summaries
* Successful-work consolidation
* Temporary history cleanup
* Clear distinction between working and durable history

Explore:

* Squash-based consolidation
* Selective preservation of intermediate milestones
* Discarding failed experiments
* Session metadata in Git notes or an auxiliary index

**Success criteria:**

* The system can turn a noisy editing session into clean, useful Git history.
* Consolidation preserves intended work without losing unrelated changes.
* Users can inspect the proposed result before committing.

## Phase 6: Agent experimentation and safety

**Goal:** Give agents a reliable mechanism for experimentation and recovery.

Implement initial agent-facing operations:

* Explicit checkpoints
* Session history queries
* Snapshot comparisons
* Safe restoration
* Milestone creation

Explore:

* Experiment lifecycle
* Known-good state tracking
* Validation results associated with checkpoints
* Alternative implementation comparisons
* Selective rollback

**Success criteria:**

* Agents can use Driftr predictably through a scriptable interface.
* Experiments can be recovered or discarded safely.
* Agent operations do not silently destroy unrelated human or agent work.

## Phase 7: Concurrent agents and workflow optimization

**Goal:** Explore Driftr as an infrastructure layer for more autonomous development.

Investigate:

* Multiple agent sessions
* Session-specific working states
* Overlapping file ownership
* Isolated experiments
* Integration with Git worktrees
* Conflict detection
* Agent handoffs
* Task-level change summaries
* MCP integration
* Validation-aware consolidation

This phase may lead to a more substantial product direction, but it should be driven by real concurrency and workflow requirements.

**Success criteria:**

* Clear evidence that Driftr improves safety or productivity beyond automatic versioning.
* A well-defined model for overlapping agent work.
* A demonstrated advantage over conventional Git worktrees and existing agent orchestration tools.

---

# 12. MVP Scope and Non-Goals

## MVP: What to build first

The first usable release should include:

* Standalone Rust CLI
* Existing Git repository support
* Optional repository initialization
* Recursive filesystem watching
* Markdown-first tracking
* Automatic detection of new files
* Idle-based checkpoints
* Deterministic commit messages
* Basic status
* Basic history
* Diff integration
* Single-file restore
* Ignore and include configuration
* Safe handling of staged changes
* macOS and Linux support, subject to dependency validation

The MVP should work without an AI API, Claude hooks, a database, or a background daemon.

## Explicit non-goals

Do not initially build:

* A replacement for Git
* A custom distributed version-control protocol
* A cloud synchronization service
* A graphical editor
* A general-purpose backup platform
* A mandatory AI summarization pipeline
* An elaborate agent orchestration engine
* A custom branching and merging model
* A semantic code analysis engine
* A sophisticated multi-agent conflict resolver

These may become relevant later. None is necessary to validate the initial product.

---

# 13. Risks and Open Questions

## 13.1 Is automatic Git committing enough?

This is the central architectural question.

Automatic commits may solve most of the initial problem. They may also be sufficient for a surprisingly large part of the long-term vision.

A separate working-history layer should be introduced only when the need for temporary state, session isolation, or non-destructive consolidation is demonstrated.

## 13.2 How should Driftr handle shared working trees?

A filesystem watcher can observe changes but cannot inherently isolate ownership.

If multiple agents modify the same working tree, attribution and rollback become difficult.

Git worktrees may provide a practical isolation mechanism for independent agent tasks. Driftr could eventually manage or integrate with them.

However, we should not assume that every agent requires a separate worktree. That could impose substantial resource and workflow overhead.

## 13.3 How much should agents control?

There is a design tension between automatic protection and explicit agent control.

Too little control, and agents cannot take advantage of experimentation features.

Too much control, and the product becomes another complex API agents must learn.

The best initial interface may be deliberately small:

* Create checkpoint
* Inspect history
* Compare states
* Restore state
* Preserve milestone

More sophisticated operations should emerge from real agent usage.

## 13.4 How should AI summaries work?

Summaries could improve both human-readable history and agent context.

But AI-generated descriptions introduce:

* Latency
* API costs
* Privacy concerns
* Nondeterminism
* Potentially inaccurate interpretations

Core snapshot creation should never depend on a model response.

Start with deterministic descriptions. Add optional AI assistance once the basic history is useful.

## 13.5 Is session attribution accurate enough?

Hooks offer context, but filesystem changes and agent activity don't map perfectly.

Agents may modify files through subprocesses, scripts, or tools that don't produce the expected hook events.

Human edits can overlap with agent sessions.

Attribution should be modeled as evidence with varying confidence, not as an absolute property of a file.

## 13.6 What is the real competitive advantage?

Filesystem watching and automatic Git commits are not inherently novel.

The product's differentiation must come from the complete experience:

* Automatic and reliable checkpoints
* Session-aware history
* Easy recovery
* Safe experimentation
* Clean Git consolidation
* Useful agent-facing operations

The project should continuously validate which of these features creates meaningful value rather than simply accumulating capabilities.

---

# 14. Success Metrics

Measure outcomes that demonstrate trust and practical utility.

## Reliability

* Percentage of observed meaningful changes successfully checkpointed
* Number of missed or corrupted checkpoints
* Recovery success rate
* Watcher crash and restart behavior
* Incidents involving unrelated staged or uncommitted changes

## Usability

* Time from initialization to useful automatic tracking
* Frequency of manual Git operations required for document history
* Time to find and restore a previous version
* Percentage of checkpoints that users find meaningful

## Performance

* Idle CPU and memory usage
* File-change detection latency
* Checkpoint creation latency
* Storage overhead under realistic editing workloads

## Agent productivity and safety

Later-stage metrics might include:

* Recovery from failed agent experiments
* Number of successful experiments preserved
* Time spent resolving unwanted agent changes
* Incidents involving overlapping sessions
* Reduction in manual intervention during agent workflows
* Percentage of agent tasks successfully consolidated into clean Git history

The most important early signal is whether the tool becomes trustworthy enough that users leave it running continuously.

The most important long-term signal is whether agents can undertake more ambitious work with less risk and less human supervision.

---

# 15. Immediate Technical Experiments

Before investing in the full architecture, build small prototypes to answer the highest-value questions.

## Experiment A: Automatic checkpointing

Create a Rust prototype that:

* Watches a real Claude Code workspace
* Detects Markdown changes
* Coalesces rapid writes
* Creates ordinary Git commits after inactivity
* Displays recent checkpoints

Test against realistic Claude editing patterns rather than synthetic file writes alone.

**Question:** Can automatic commits provide a genuinely frictionless document history without interfering with normal Git use?

## Experiment B: Git consolidation

Create a repository with a sequence of automatic commits representing a simulated AI task.

Test:

* Squashing temporary commits
* Preserving intermediate milestones
* Dropping failed experiments
* Recovering previous snapshots
* Handling uncommitted and staged work
* Consolidating into a clean commit without rewriting shared history

**Question:** Can Git itself provide the working-history lifecycle we want?

## Experiment C: Claude hook attribution

Integrate a minimal Claude Code hook.

Record:

* Session start
* Tool invocations
* Relevant file paths
* Session end
* Observed filesystem modifications

Compare hook events against actual filesystem changes.

**Question:** How accurately can we associate modifications with agent sessions, and which events provide the most useful context?

## Experiment D: Concurrent agent behavior

Run two Claude sessions against a shared repository.

Have them:

* Edit independent files
* Edit overlapping files
* Finish at different times
* Trigger checkpoints while the other session is active

Observe:

* Attribution accuracy
* Checkpoint safety
* Git index interference
* Restoration behavior
* Opportunities for worktree-based isolation

**Question:** What additional architecture is genuinely required to support concurrent agents safely?

---

# 16. Initial Repository and Implementation Plan

Start with a small, functional Rust project.

Suggested structure:

```text
driftr/
├── Cargo.toml
├── README.md
├── LICENSE
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── config.rs
│   ├── git.rs
│   ├── watcher.rs
│   ├── checkpoint.rs
│   ├── history.rs
│   └── restore.rs
├── tests/
│   ├── git_integration.rs
│   ├── watcher_integration.rs
│   └── recovery_integration.rs
└── docs/
    ├── architecture.md
    ├── decisions.md
    └── roadmap.md
```

Begin with three commands:

```bash
driftr init
driftr watch
driftr history
```

The first working milestone is intentionally small:

1. Initialize or discover a Git repository.
2. Monitor Markdown files.
3. Detect and coalesce changes.
4. Wait for a configurable idle period.
5. Create a normal Git commit with a deterministic message.
6. Display the resulting history.

Then add diff and restore.

Once the prototype works reliably in a real development environment, investigate session hooks and consolidation.

Do not start with the database, MCP server, background daemon, or independent snapshot store.

---

# 17. Long-Term Vision

Driftr begins as a simple tool that eliminates manual versioning for documents.

But its underlying ideas have broader implications.

If every meaningful change is recoverable, if agent sessions can be understood independently, and if successful work can be consolidated cleanly, then the development workflow itself can become more flexible.

Agents can experiment without treating every failed attempt as a crisis. Developers can delegate larger tasks without losing visibility into how the work evolved. Multiple agents can potentially operate with clearer boundaries and more reliable recovery.

The long-term opportunity is to provide the change-management infrastructure that makes increasingly autonomous software development practical.

Git remains valuable as the shared, durable history of completed work. Driftr focuses on everything that happens before and around that history: continuous edits, experiments, sessions, intermediate states, and safe recovery.

**The fundamental product thesis: AI agents should be able to change software as freely as they can reason about it, because the cost of failure and recovery has been dramatically reduced.**

**Driftr — Let your code evolve. Keep every step.**

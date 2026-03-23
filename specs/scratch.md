Can you archive this sprint in `specs/sprints/<number>-<name>.md` and then create a new SPRINT for us to work on? I want you to pull in all those items you listed

---

One usecase pattern I want is to have a 1-click business where we can support a full business office of AI agents that handle everything from name generation, technical, CSO, CIO, etc. I want these decisions being made by AI agents all over the place

I also want a few examples in the `examples/` directory with READMEs

I'd love for you to help me figure out the gaps in workload 2

---

Can we make it so the lightweight version just runs the orchestrator? Is that a good idea? We want to be able to run it on a lightweight device, but don't necessarily need the full package where the runners use the queue?

---

Can we have it so that available tools are selected by AI by their name and description as well as configuring them?

---

Ready for review
Select text to add comments on the plan
Autonomous Agent Swarms — Parallel Execution, Sandboxing, Self-Management
Context
AgentZero's workflow executor currently runs agent nodes sequentially by topological level — all nodes in a level must complete before the next level starts. 1Agents execute in-process with no filesystem or process isolation. There is no mechanism for agents to autonomously decompose goals, spawn workers, or recover from failures.

This plan adds five capabilities that make AgentZero a self-managing swarm runtime: event-driven parallel execution, sandboxed agent isolation, cross-agent awareness, dead agent recovery, and autonomous goal decomposition. Inspired by competitive analysis of multi-agent coordination frameworks.

Intended outcome: agentzero swarm "Build a REST API with auth" decomposes the goal into a visual workflow graph, spawns sandboxed agents in parallel, coordinates their work with conflict detection, recovers from failures, and merges results — with the user able to observe and intervene via the UI at any point.

Phase 1: Event-Driven Task Unblocking (Small, High Impact)
Files: workflow_executor.rs

Replace level-based execution with fine-grained dependency tracking. When a node completes, immediately start any downstream node whose dependencies are all satisfied — don't wait for the entire level.

Change execute() from iterating levels to maintaining a ready-queue
Track pending_deps: HashMap<NodeId, HashSet<NodeId>>
On completion, remove the node from all dependents' sets; if a set becomes empty, push to ready-queue
Run ready nodes concurrently with tokio::JoinSet
Phase 2: Sandboxed Agent Execution (Security-First Isolation)
Files: workflow_executor.rs, new sandbox.rs + workspace.rs modules in agentzero-orchestrator

Each agent node executes in an isolated sandbox. This is the core security differentiator — agents can't escape their boundaries.

AgentSandbox trait — pluggable execution backends:

#[async_trait]
trait AgentSandbox: Send + Sync {
    async fn create(&self, config: SandboxConfig) -> Result<SandboxHandle>;
    async fn execute(&self, handle: &SandboxHandle, task: AgentTask) -> Result<AgentOutput>;
    async fn destroy(&self, handle: SandboxHandle) -> Result<()>;
}
Backends (layered by security level):

WorktreeSandbox (lightweight, default for local dev) — git worktree per agent on branch agentzero/wf/{workflow_id}/{node_id}. Filesystem isolation only. ToolSecurityPolicy enforced.
ContainerSandbox (medium, default for server) — Docker/Podman container per agent. Bind-mount the worktree. Network policy, memory/CPU limits, seccomp profile.
MicroVmSandbox (maximum, opt-in) — Firecracker/Cloud Hypervisor microVM per agent. Full kernel isolation. ~125ms boot. Ideal for untrusted plugins or code execution.
Workspace lifecycle (shared across all backends):

Create worktree → mount into sandbox → agent executes → collect output + diff → destroy sandbox → merge worktree
Merge strategy: sequential merge in topological order after all agents complete
Conflict detection: diff overlapping files, classify severity (high=same lines, medium=same file, low=same directory), report to user or invoke a resolver agent
Shipping order:

Phase 2a (ship first): WorktreeSandbox — parallel execution with filesystem isolation
Phase 2b (follow-up): ContainerSandbox with Docker — process/network isolation
Phase 2c (later): MicroVmSandbox with Firecracker — kernel-level isolation
Phase 3: Cross-Agent Context Awareness
Files: coordinator.rs, swarm.rs

When dispatching parallel agents, inject awareness of sibling agents' work to prevent conflicts and enable collaboration.

Before spawning agent N, collect: other agents running in parallel, their task descriptions, estimated file scopes, any known overlaps
Append context summary to agent's system prompt or initial message
On completion, publish a summary of files modified to the event bus
If file overlap detected mid-execution, notify affected agents via ConverseTool
Phase 4: Dead Agent Recovery
Files: coordinator.rs

Extend the existing PresenceStore heartbeat system to automatically reassign tasks from dead agents.

Configure heartbeat timeout per agent (default 60s)
On timeout: mark agent as Failed, destroy its sandbox, reset task to pending
Coordinator re-dispatches to a fresh agent instance (new sandbox)
Emit event for observability (UI shows failed → retrying transition)
Phase 5: Self-Managing Swarm — Goal Decomposition + Autonomous Spawning
Files: coordinator.rs, workflow_executor.rs, new goal_planner.rs module

The capstone: give AgentZero a goal and let it autonomously decompose it into a task DAG, spawn sandboxed agents, and manage execution.

What makes this elegant:

Plan is a visual graph — the decomposed goal produces a WorkflowGraph (same format as the visual builder). The user sees the plan live in the UI, can pause execution, edit nodes, re-route edges, then resume.

Sandboxed by default — every spawned agent runs in an AgentSandbox (Phase 2). Agents get only the capabilities they need.

Typed coordination, not CLI shelling — agents communicate via ConverseTool (in-process channels with oneshot responses) and the event bus. No serialization overhead, no race conditions from file-based state.

Adaptive re-planning is a graph edit — when an agent fails, the supervisor re-invokes the planner, which diffs the current graph and produces a patch (add nodes, re-route edges, adjust prompts). The user sees the diff in the UI and can approve/modify before resuming.

Design:

GoalPlanner — takes a natural language goal + an LLM, produces a WorkflowGraph
Structured output prompt → JSON nodes (agent tasks with role/prompt/tools/sandbox level), edges (dependencies), estimated file scopes per node
The planner is itself an agent — it can use ConverseTool to ask the user clarifying questions before finalizing
File scope estimation enables conflict prediction before execution starts
SwarmSupervisor — takes a WorkflowGraph and executes it using Phases 1-4
Creates sandboxes (Phase 2), injects cross-agent context (Phase 3), monitors heartbeats (Phase 4)
Supervisor loop watches for: stuck agents, dependency cycles, budget/token limits, conflict alerts
Publishes progress events → UI shows real-time node status (pending/running/done/failed)
Adaptive re-planning: On agent failure or scope expansion, supervisor pauses affected subgraph, invokes planner with current state, applies graph patch, resumes
CLI: agentzero swarm "Build a REST API with auth and rate limiting" — single command, streams progress
Gateway: POST /v1/swarm with { "goal": "...", "sandbox_level": "container" } — returns workflow ID, streams via SSE
UI: Goal input → live graph visualization → interactive editing during execution → merge review at end
Implementation Notes
Existing infrastructure to reuse:

StepDispatcher trait (workflow_executor.rs) — extend for sandbox-aware dispatch
PresenceStore (coordinator.rs) — already tracks heartbeats, extend for recovery
ConverseTool (swarm.rs) — already enables agent-to-agent communication
EventBus (4 backends) — publish swarm progress events
WorkflowStore / TemplateStore — persist generated workflow graphs
ToolSecurityPolicy (tools/lib.rs) — enforce per-sandbox capability gates
Kahn's algorithm in compile() — already builds the dependency graph, just change execution strategy
What needs updating:

specs/SPRINT.md — add new sprint section for this work
specs/plans/31-autonomous-agent-swarms.md — copy of this plan
Verification
Phase 1: Existing workflow tests pass. New test: diamond-dependency graph where C depends on A and B — C starts as soon as both complete, not waiting for unrelated D in the same level.
Phase 2a: Worktree creation/cleanup lifecycle. Parallel agents produce independent diffs. Merge with no conflicts. Merge with conflicts reports severity.
Phase 2b: Container sandbox creates/destroys cleanly. Network isolation enforced. Resource limits (memory/CPU) enforced.
Phase 2c: MicroVM boot/shutdown. Kernel isolation verified (no host filesystem access outside mount).
Phase 3: Parallel agents receive sibling context in their prompts. File overlap triggers notification.
Phase 4: Heartbeat timeout triggers sandbox destruction + task reassignment.
Phase 5: Goal decomposition produces valid workflow graph. Swarm executes end-to-end with 2-agent goal. Adaptive re-planning patches graph on agent failure.
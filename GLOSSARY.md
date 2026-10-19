# Ship Signal glossary

Plain-language definitions of product and technical terms used in this repository and its design discussions. Inclusion explains a term; the [product design](docs/01-grill-session/README.md), [technical decisions](docs/02-technical-decisions/README.md), and [ADRs](docs/03-adr/README.md) establish whether a particular approach is accepted, open, or deferred.

## Language

### Product and people

**B2C (business-to-consumer)**:
A business model in which a product serves individual consumers rather than other businesses.

**Customer team**:
The team that installs and operates Ship Signal for its own product. This is distinct from the individual customers whose feedback is analyzed.

**Feedback customer**:
An individual customer of the product being analyzed, identified by that application's customer ID when available. References to customer counts in demand calculations mean these people.
_Avoid_: Using "customer" without context when it could mean the customer team instead.

**Customer-hosted**:
An arrangement in which the customer team runs Ship Signal on infrastructure it controls.

**MVP (minimum viable product)**:
The initial, deliberately limited version intended to test whether Ship Signal delivers useful results. A discussed possibility is not automatically part of this version.

**Pilot**:
A limited trial with a real product and genuine customer feedback to assess usefulness, implementation reliability, human effort, and cost.

**Workflow**:
The connected sequence from feedback analysis through ticket creation, approval, implementation, and draft PR delivery.

**Analysis**:
The investigation of customer feedback, demand, and existing work that informs a proposed ticket.

**Implementation**:
The work of changing a repository to deliver an approved ticket's behavior, including verification and preparation of a draft PR.

### Feedback and demand

**Explicit feature request**:
A message directly asking for new or changed product behavior, rather than a need inferred from indirect evidence.

**Inferred need**:
A possible customer goal interpreted from feedback that does not directly request that behavior. For example, difficulty finding an export could suggest a discoverability problem.

**Underlying need**:
The customer goal shared by related requests. Being alerted when an item returns to stock is one need that could have email and SMS variants.

**Requested variant**:
A particular way a customer wants a need addressed, such as email rather than SMS notifications.

**Feature identity**:
The identity used to recognize a feature across messages, tickets, and weeks. Whether it represents a broad need or a particular behavior is one of the distinctions under discussion.

**Grouping**:
The organization of related requests around a shared need while retaining differences in requested behavior.

**Deduplication**:
Recognizing repeated contributions so they are counted only once for the relevant purpose. Feedback deduplication concerns customer counts; duplicate-write prevention concerns repeated tickets or PRs.

**Distinct customer**:
One identifiable feedback customer, regardless of how many attributable messages that person submitted.

**Unidentified feedback**:
Feedback that cannot yet be linked to an application's customer ID.

**Demand percentage**:
The percentage of that week's feature-requesting customers who requested a particular feature: distinct customers requesting it divided by distinct customers requesting any feature, multiplied by 100.

**Denominator**:
The number beneath the division in a calculation. For demand percentage, it is the number of distinct customers requesting any feature that week.

**Demand threshold**:
The comparison boundary used to decide whether weekly demand qualifies a feature for consideration. "Strictly greater than" excludes a percentage exactly equal to the boundary.

**Weekly interval**:
The span of submission times included in one week's analysis.

**Half-open interval**:
A time range that includes its start and excludes its end, written `[start, end)`. Adjacent ranges can then share a boundary without counting the same timestamp twice.

**Late arrival**:
A feedback record that becomes available after the analysis for its submission period has already run.

**Historical correction**:
A later change to information about past feedback, such as linking an earlier unidentified message to a customer.

**Historical rerun**:
A person-requested repeat of analysis for an earlier weekly interval using feedback currently available for that interval. Its corrected result is distinct from the original analysis.

**Classification**:
An assignment of meaning or category to feedback, such as feature request, bug report, or membership in a particular request group.

**Provenance**:
Information about where a result came from and how it was produced, such as source messages, model choice, and configuration.

**Reproducibility**:
The ability to repeat an analysis with the same recorded inputs and conditions and assess whether it produces the same result.

### Tickets, scope, and approval

**Evidence-backed ticket**:
A proposed work item supported by attributable feedback, demand counts, and investigation of related work.

**Acceptance criteria**:
Observable conditions used to judge whether the ticket's requested behavior has been delivered.

**Implementation scope**:
The behavior and changes included in a particular implementation, together with its explicit boundaries.

**Bounded change**:
A change with clear, limited scope that fits the supported capabilities and existing infrastructure.

**Expanded scope**:
Work beyond what was approved, such as adding another delivery channel to a ticket that covered only email.

**Configured approver**:
A person designated by the customer team as authorized to approve implementation.

**Approval actor**:
The person or identity responsible for an approval-related action. A ticket's status alone does not identify who authorized it.

**Approval provenance**:
Evidence of who approved work, when, and through which action.

**Scope snapshot**:
A saved copy of a ticket's scope and content at a particular moment, such as approval time.

**Versioned approval**:
Approval explicitly tied to an identified revision of the proposed scope.

**Approval receipt**:
A record connecting an approval action to the actor and the scope or version being approved.

**Reapproval**:
A new approval decision after an earlier approval, for example when the requested scope expands.

**Rejected-feature cooldown**:
The period following a "Not needed" decision during which proposals for the same or similar feature are suppressed. It is distinct from the time window used to count weekly feedback.

**Fresh approval**:
A separate approval for a new ticket, rather than authorization inherited from an older ticket about related work.

**Hard search boundary**:
A restriction defining which tickets may be considered, such as a configured product, team, or project.

**Contextual ranking**:
Ordering candidate matches by relevance using information such as labels or descriptions, without necessarily excluding every nonmatching item.

**Repository routing**:
The association of a proposed ticket with the repository or repositories responsible for delivering it.

### Storage and data contracts

**Feedback database**:
The customer's source of feedback records. It is distinct from Ship Signal's own application-state storage.

**Application state**:
Information Ship Signal maintains about its own work, such as run progress, schedules, repository claims, and external operations.

**Durable state / persistence**:
State saved so it can survive process termination and be loaded later. Persistence is the act or capability of retaining that state.

**Source of truth / authoritative record**:
The designated record that determines a particular fact when copies disagree, such as which run currently owns a repository.

**Logical schema / feedback contract**:
The agreed shape and meaning of feedback data exposed to Ship Signal, independent of how the customer's underlying tables are organized.

**Read-only view**:
A named database query exposed through a connection that permits reading its results without modifying the source records.

**Field type and nullability**:
The kind of value a field contains, such as text or a timestamp, and whether a missing value is allowed.

**Stable identifier**:
A value that continues to identify the same entity across reads or retries, rather than changing with its display name or position.

**Database transaction**:
A group of database operations treated as a unit that commits or rolls back together. Its boundary does not automatically include repository edits or requests to external services.

**Atomic operation**:
An operation treated as indivisible, with no partially completed outcome visible to competing work. Checking availability and claiming ownership together is an example of the intended boundary.

**Database constraint**:
A rule enforced by the database, such as requiring an identifier to be unique.

**SQLite**:
A relational database engine embedded in an application, commonly storing a database in a local file rather than a separate database server.

**PostgreSQL and MySQL**:
Relational database systems commonly run as separate servers that applications connect to. The repository discusses both as feedback sources and compares PostgreSQL with SQLite for application state.

**WAL (write-ahead log)**:
A log that records database changes before they are incorporated into the main database file. SQLite's WAL mode allows readers and a writer to work concurrently, while still allowing only one writer at a time.

**Network filesystem**:
Storage accessed over a network as though it were a local folder or file. Directly sharing a database file this way differs from connecting to a database server.

**Backup and restore**:
A backup is a recoverable copy of stored data; restoration is returning that copy to usable operation after loss or failure.

**Migration**:
A controlled change to a database's structure or stored data, such as adding a table or transforming existing records.

### Execution and concurrency

**Host / single-host deployment**:
A host is a machine or virtual machine running software. A single-host deployment keeps the installation's execution and local application-state coordination on one such host.

**Runtime**:
The environment and supporting machinery in which the program executes, including how tasks and processes are run.

**Execution service**:
The running Ship Signal component that coordinates implementation work.

**Worker**:
A running component assigned work to perform. The term describes a role and does not necessarily mean a separate machine.

**Executor / subprocess**:
An executor carries out implementation commands; a subprocess is a program launched by another process. A subprocess can sometimes remain alive after the process that launched it exits.

**Concurrency**:
Work whose execution overlaps in time, including tasks on the same host. Concurrent implementations can conflict even if their database writes happen one after another.

**Race condition**:
A situation where the outcome depends on the timing of competing work. Two workers both seeing a repository as free before either claims it is an example.

**Repository ownership / repository claim**:
Temporary permission for a particular run to perform implementation work in a repository. This execution permission is distinct from repository routing, which identifies where a ticket belongs.

**Lock**:
A mechanism granting exclusive access to a resource while held. A database write lock and an application lock covering repository work protect different resources.

**Lease**:
Temporary ownership that expires unless renewed. Expiration means renewal stopped, not necessarily that the previous worker or its subprocesses have stopped.

**Heartbeat**:
A periodic signal that a worker is still reporting, often used to renew a lease.

**Stale worker**:
A worker that continues after its ownership has expired or been replaced. Its continued activity can conflict with the replacement worker.

**Contention**:
Competition for a resource that cannot serve all contenders simultaneously, such as several tasks wanting to write to one SQLite database.

**Serialization of work**:
Making competing operations take turns. Serializing database writes does not, by itself, serialize complete implementations.

**Failover**:
Moving responsibility to another host or service instance after the active one fails.

### Retries and external-write recovery

**Run**:
One tracked execution of a workflow or part of it, with its own progress and outcome.

**Checkpoint**:
A saved description of workflow or agent progress from which execution can resume. It does not by itself prove what happened in an external service.

**Resumption**:
Continuing previously saved work from its recorded progress.

**Recovery**:
Establishing the actual state after an interruption and deciding how existing work can safely continue.

**Retry / attempt**:
A retry is another attempt at the same intended action after an earlier attempt failed or had an uncertain outcome. It is distinct from a new proposal for similar work.

**Bounded attempts**:
Attempts subject to explicit limits, such as a maximum count, elapsed time, or cost.

**Implementation retry**:
A human-authorized new attempt at failed or unfinished implementation work, with a fresh bounded budget. It is distinct from recovering an interrupted execution under its existing budget and from retrying an individual external API operation.

**External write / side effect**:
A change outside Ship Signal's application database, such as creating a Linear ticket or opening a GitHub PR. A side effect more broadly means a change made by an operation beyond returning a result.

**Operation intent**:
A record of an intended action, before its outcome is known.

**Operation identity**:
The stable identity of one intended action across all its attempts. A new ticket proposal after cooldown is a new action, even if it concerns the same feature.
_Avoid_: Treating the feature name or an individual attempt ID as interchangeable with the operation identity.

**Correlation marker**:
An identifier connecting a local operation record with the corresponding action or object in an external service.

**Idempotency / duplicate-safe operation**:
Idempotency means repeating an operation has the same intended effect as performing it once. For ticket creation, duplicate safety means recovery or retry does not create an extra ticket for the same intended action.

**Uncertain outcome / response loss**:
An uncertain outcome occurs when Ship Signal cannot tell whether an action completed; response loss is one possible cause. For example, Linear may create a ticket even though the success response never reaches Ship Signal.

**Reconciliation**:
Checking local records against external reality to resolve an uncertain or conflicting state, such as discovering which ticket an interrupted operation created.

### Scheduling

**Customer-configured timezone**:
The local time reference chosen by the customer team for analysis and execution schedules.

**Approval cutoff**:
The time boundary determining which approved tickets are eligible for automatic weekend execution, based on when approval actually occurred.

**Execution window / Sunday cutoff**:
The scheduled execution window is the regular period for automatic implementation. The Sunday cutoff ends admission of new implementations through the automatic queue; ongoing implementations can continue within a bounded grace period.

**Grace period**:
A bounded extension after Sunday's cutoff for implementations already underway. Starting an untouched ticket after the cutoff is manual execution, not use of the grace period.

**Manual execution**:
An explicit human request to run an approved ticket outside the scheduled execution window, with an end deadline. Approval alone is not a request to start immediately.

**Execution budget**:
The bounded allocation of attempts, time, and cost for implementation work. Automatic recovery preserves consumption of the existing budget; a human-authorized implementation retry receives a fresh one.

**Weekly cycle**:
The scheduled Friday analysis and its following weekend execution window, with their assigned timezone and scheduling boundaries.

**Queue**:
A collection of work waiting to be processed, together with whatever ordering policy applies.

**Weekend cohort**:
The set of tickets selected as eligible for one weekend's execution window.

**Catch-up**:
Running scheduled work late after service unavailability, while preserving the original scheduling boundaries and eligibility rules.

**Daylight-saving change**:
A seasonal change to a timezone's clock offset that can shift how a local scheduled time maps to UTC.

### Integrations, code, and verification

**API (application programming interface)**:
An interface through which software requests information or actions from another component or service.

**Webhook**:
A notification sent by a service to an application's endpoint when an event occurs, such as a ticket update. It differs from repeatedly asking the service whether something changed.

**HTTPS endpoint**:
A network address that accepts requests over an encrypted HTTPS connection, such as the address receiving Linear webhooks.

**Credentials**:
Information proving an identity or granting access, such as an API key or account password.

**Connector / adapter**:
Integration code translating between Ship Signal and another system's interface or data format.

**Repository**:
A version-controlled collection of a project's code and related files.

**Branch / base commit**:
A branch is a line of code development; the base commit is the recorded revision from which a change starts or against which it is compared.

**Draft PR (pull request)**:
A proposed set of repository changes opened in GitHub in draft status for visibility and review. Opening a draft does not establish that its checks passed or its behavior is correct.

**Merge**:
Incorporating a branch's changes into another branch after review and any required checks.

**Required checks**:
The verification results a repository expects before work can be considered successfully delivered, such as tests or build checks.

**Baseline failure**:
A check failure already present before the proposed changes, rather than one introduced by them.

**Verification manifest**:
An explicit description of how a repository is set up and verified, including commands, required checks, and supporting services.

**Deployment evidence**:
Information showing that behavior is available in a running product. Code on a branch or a completed ticket alone does not establish customer availability.

### Execution environment and isolation

**Execution environment**:
The files, tools, services, resources, and permissions available to commands performing implementation work.

**Execution isolation**:
Boundaries limiting what running commands can access or affect outside their intended environment.

**Container / Docker**:
A container is a way to package and run software with operating-system boundaries around its environment. Docker is a toolset for building and running containers; actual access restrictions depend on configuration.

**VM (virtual machine)**:
A software-defined machine with its own operating system, running on a host through virtualization.

**Disposable environment**:
An execution environment intended to be discarded after use rather than reused indefinitely.

**Application test account**:
A login to the customer's application used for testing its behavior. It is distinct from a database user and does not itself isolate host or database access.

**Shared-database interference**:
One activity affecting another through a shared database, for example by modifying test data or applying conflicting migrations.

### AI and orchestration

**LLM (large language model)**:
An AI model that processes and generates language and can be used for tasks such as feedback classification or code generation.

**AI provider / local model**:
An AI provider supplies access to models through a service; a local model runs in an environment controlled by the customer team rather than through an external provider's hosted inference service.

**Agent / tool calling**:
An agent uses a model to choose actions toward a task; tool calling lets it request application-provided functions, such as reading a file or querying tickets.

**Orchestration / state machine**:
Orchestration coordinates steps, decisions, pauses, and retries; a state machine describes allowed states and transitions between them.

**Structured output**:
A model response organized into an expected data shape, such as named fields, rather than only free-form prose.

**Serialization**:
Converting in-memory data into a representation that can be stored or transferred and later reconstructed. This differs from serializing competing work by making it take turns.

**LangGraph**:
A framework for organizing stateful agent workflows as connected steps, including persistence and pause/resume capabilities.

**LangChain**:
A library ecosystem for building applications around language models, including model-provider integrations.

**Rig**:
A Rust library for building LLM applications and agents with model integrations, tool calling, and structured outputs.

**Rust / CLI (command-line interface)**:
Rust is a programming language; a CLI is an interface operated through typed commands in a terminal. The implementation language and the workflow framework are separate choices.

**Model qualification**:
Evaluation of whether a particular model can reliably perform its assigned tasks, rather than merely whether its API connects successfully.

**Deterministic calculation**:
A calculation that produces the same result for the same inputs, such as a fixed demand-percentage formula. Model interpretation of the inputs is a separate source of variability.

### Records, retention, and decisions

**Artifact**:
A stored or copied output of work, such as a log, checkpoint, feedback excerpt, ticket, or PR.

**Retention policy**:
Rules defining how long different records or artifacts are kept and when they are deleted.

**Redaction**:
Removing or obscuring sensitive parts of content while retaining the remaining useful information.

**Disclosure / data boundary**:
Disclosure is making information available to another party; a data boundary separates the systems or parties that hold or receive it.

**ADR (Architecture Decision Record)**:
A document preserving a consequential architecture choice, the reasons for it, the alternatives considered, and its consequences.

**Proposed / Accepted**:
"Proposed" means a decision is under consideration; "Accepted" means it is the agreed direction. Acceptance does not mean implementation is complete.

**Rejected / Superseded / Deprecated**:
"Rejected" means a proposal was not adopted; "Superseded" means a later decision replaced it; "Deprecated" means it is no longer applicable without necessarily naming a replacement.

**Deferred scope**:
Work intentionally left out of the current version. Deferral does not by itself commit the product to delivering it later.

**Revisit trigger**:
A change in requirements or evidence that would justify reconsidering an accepted decision.

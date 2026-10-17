# ADR-0003: Use SQLite for single-host MVP application state

- **Status:** Accepted
- **Date:** 2026-10-17

## Context

Ship Signal needs durable writable storage for its own workflow state, separate from the customer's read-only PostgreSQL or MySQL feedback connection. It must preserve progress across restarts, run one implementation at a time per repository, and avoid duplicate Linear tickets and GitHub PRs when operations repeat or API responses are lost.

The technical review recommended PostgreSQL for shared application state and coordination. The subsequent discussion favored simpler customer-hosted setup for MVP, accepting a single-host deployment boundary instead of requiring a database server or distributed coordination infrastructure.

## Decision

Use SQLite for MVP application state. Each customer installation runs on one host with one execution service coordinating work and a SQLite database on persistent local storage. Different repositories may run independently; the existing limit of one implementation at a time per repository remains.

Do not support multiple active hosts coordinating the same installation through separate SQLite files or direct access to a database file on a network filesystem. The customer's feedback database can remain remote and can use either PostgreSQL or MySQL; its engine does not determine the application-state engine.

SQLite transactions and constraints must support explicit application ownership rules. Serializing database writes does not serialize repository edits, subprocesses, or external API calls. Restart recovery must account for implementation subprocesses that may outlive their controller before admitting replacement work. A lease expiration, if leases are used, is not proof that an old executor has stopped.

Retries of the same intended external action must retain its operation identity and reconcile uncertain outcomes before repeating a write. A genuinely new qualifying proposal after rejection cooldown must be distinguishable from a retry and still requires a new ticket and fresh approval. A local operation ID alone does not prevent duplicates in an external service.

## Alternatives considered

- **PostgreSQL application state and coordination:** provides a shared database suitable for multiple application hosts, but adds database setup and operation to MVP. Not selected for the single-host deployment.
- **Additional queue or coordination infrastructure:** can support distributed execution, but adds components and recovery responsibilities that are unnecessary for the chosen MVP boundary.

## Consequences

- Customers do not need to provision a separate database server for Ship Signal's application state.
- The application database must survive process restarts and redeployments. Backup and restore procedures must be documented.
- SQLite permits multiple readers but only one write transaction at a time. Database transactions should stay short rather than span coding sessions or network requests. Contention handling is still required.
- Single-host deployment simplifies ownership and process recovery, but does not eliminate concurrent tasks, duplicate launches, or surviving subprocesses. Multi-host execution and failover are outside this MVP decision.
- This decision selects storage and deployment boundaries, not a completed recovery design. Record schemas and authority, ownership/locking mechanisms, subprocess recovery, external correlation and reconciliation, SQLite configuration, and backup procedures remain to be specified and validated against crash and response-loss scenarios.
- This ADR does not select the programming language or replace the documented LangGraph/LangChain choice with Rig. That framework change remains a separate discussion.

## Revisit triggers

Reconsider this decision if an installation needs multiple active execution hosts, failover to another host, or sustained write contention that cannot be handled within the single-host design.

## References

- [Product design sections 10, 12, and 13: execution, deployment, and workflow state](../01-grill-session/README.md)
- [Technical decision point 3: durable state, concurrency, and external-write recovery](../02-technical-decisions/README.md)
- [SQLite deployment guidance](https://www.sqlite.org/whentouse.html): local storage, concurrent writes, and network filesystem limitations.
- [SQLite write-ahead logging](https://www.sqlite.org/wal.html): WAL concurrency and same-host requirements if WAL is selected.

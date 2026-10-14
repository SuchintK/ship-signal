# ADR-0001: Leave execution isolation to customers for MVP

- **Status:** Accepted
- **Date:** 2026-10-14

## Context

Ship Signal runs on customer infrastructure and executes repository setup commands, tests, and implementation code. The architecture review considered requiring restricted containers or dedicated isolated workers. The MVP prioritizes a simpler product and leaves deployment choices to the customer.

## Decision

Ship Signal will neither provide nor mandate execution isolation for MVP. Customers choose whether and how to isolate execution using their own Docker configuration or other infrastructure.

Restricted containers, disposable execution environments, and dedicated isolated workers are not installation requirements. Product-managed execution isolation is not a committed future feature.

## Alternatives considered

- **Require a restricted container for each implementation:** establishes a product-controlled execution boundary, but adds container configuration, lifecycle management, and support obligations to MVP.
- **Require a dedicated isolated VM or worker:** delegates provisioning to customers while enforcing a deployment boundary, but adds an installation constraint the MVP does not need.

## Consequences

- Customers can use their existing infrastructure without a Ship Signal isolation requirement.
- Ship Signal avoids implementing and supporting an execution-isolation subsystem in MVP.
- Commands execute with the permissions and access granted by the customer environment. Their effects can extend beyond the repository when that environment permits it.
- Isolation guarantees vary between deployments; Ship Signal cannot assume a restricted execution environment. Customer-provided application test accounts do not establish host isolation.
- Introducing a mandatory execution boundary later would change installation requirements and require a new architectural decision.

## References

- [Product design: deployment, credentials, and retention](../01-grill-session/README.md#12-deployment-credentials-and-retention)

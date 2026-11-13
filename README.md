# Ship Signal

Ship Signal turns recurring customer feature requests into evidence-backed Linear tickets and implements approved work through GitHub draft pull requests.

**Feedback → weekly analysis → ticket → human approval → weekend implementation → draft PR.**

## Project status

The first analysis workflow supports a chosen interval against a PostgreSQL feedback view. Scheduling, MySQL, ticket investigation, approval, and implementation remain future work.

## Planned capabilities

- PostgreSQL and MySQL feedback integrations through a uniform read-only schema.
- Weekly analysis of explicit customer feature requests.
- Linear ticket investigation, evidence collection, and approval checks.
- Customer-hosted weekend implementation with human-reviewed draft PRs.
- Rust & Rig orchestration and configurable models for analysis and implementation.

## Documentation

- [Glossary](GLOSSARY.md)
- [Documentation index](docs/README.md)

## PostgreSQL feedback analysis

Install Rust and PostgreSQL, then build with `cargo build`. This initial CLI supports an explicit interval; it does not schedule weekly runs. It stores reports in a separate local SQLite file. Back up that file using SQLite's online backup facilities or while Ship Signal is stopped. Restrict access to the file: it contains copied customer feedback and classifications. Retention and deletion policy remain open.

Expose a PostgreSQL **view** with these exact columns and types:

| Column | PostgreSQL type | Requirement |
| --- | --- | --- |
| `product_id` | `text` | Nonempty stable product ID |
| `feedback_source_id` | `text` | Nonempty stable channel/source ID |
| `source_message_id` | `text` | Nonempty stable ID within product and source |
| `customer_id` | `text` | Nonempty stable application customer ID, or NULL |
| `submitted_at` | `timestamp with time zone` | UTC-normalized instant |
| `feedback_text` | `text` | Nonempty message |
| `source_reference` | `text` | Optional locator, or NULL |

Cast existing IDs to text in the view. Map timestamps explicitly using the source's actual timezone, for example `submitted_local AT TIME ZONE 'America/New_York'` for a local timestamp. Never apply a timezone by guesswork. The setup flag confirms that you have made this mapping; the type check alone cannot prove it. Grant the connection account SELECT on the view and no write access to customer tables. TLS is supported via PostgreSQL connection settings, using `sslmode=require` to require TLS. The native TLS connector validates the server certificate and hostname against the system trust store.

```sh
export FEEDBACK_DATABASE_URL='postgresql://feedback_reader:feedback_reader@localhost:55439/ship_signal_feedback'
cargo run -- validate --timestamps-mapped-to-utc
export OPENAI_API_KEY='your-key'
export SHIP_SIGNAL_ANALYSIS_MODEL='your-qualified-model'
cargo run -- analyze --timestamps-mapped-to-utc \
  --start 2026-11-06T00:00:00Z --end 2026-11-13T00:00:00Z \
  --state ./ship-signal.sqlite
cargo run -- report --state ./ship-signal.sqlite 1
```

The interval is start-inclusive and end-exclusive. Only product IDs and message text are sent to the configured OpenAI model for classification; feedback text may itself contain personal information. The model must support Rig structured output and should be qualified against representative feedback before production use. Each completed report includes copied inputs, classifications, supporting message indices, variants, counts, percentages, and invalid-row diagnostics. Null submission times cannot be assigned to an interval, so they are reported as invalid for every read. Duplicate message identities within a capture are excluded as ambiguous. Identified customers are counted once per product and need, across sources; unidentified feedback remains evidence but does not enter the denominator. Within a report, needs are grouped per product by exact, case-sensitive model-provided name. These names are not stable cross-week identifiers; cross-week identity and matching remain open design work.

## Synthetic feedback database

The example is fictitious and uses a disposable local database; production customers supply and manage their own feedback source. With PostgreSQL utilities on `PATH`, create a local example with:

```sh
initdb -D /tmp/ship-signal-example -A trust --no-locale
pg_ctl -D /tmp/ship-signal-example -l /tmp/ship-signal-example.log -o '-p 55439 -h 127.0.0.1' start
createdb -h 127.0.0.1 -p 55439 ship_signal_feedback
psql -h 127.0.0.1 -p 55439 -d ship_signal_feedback -v ON_ERROR_STOP=1 -f fixtures/feedback.sql
```

This example uses trust authentication and is only for local development. The fixture explicitly maps its source timestamps from UTC. To run connector tests, also load `fixtures/test_invalid.sql` and set `SHIP_SIGNAL_TEST_POSTGRES` to the example reader URL. The connector tests are marked ignored by default. Run them explicitly with `cargo test --test postgres -- --ignored`; run the remaining suite with `cargo test`.

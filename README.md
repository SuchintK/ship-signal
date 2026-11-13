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

The interval is start-inclusive and end-exclusive. Only product IDs and message text are sent to the configured OpenAI model for classification; feedback text may itself contain personal information. The model must support Rig structured output and should be qualified against representative feedback before production use. Each completed report includes copied inputs, classifications, supporting message indices, variants, counts, percentages, and invalid-row diagnostics. Null submission times cannot be assigned to an interval, so they are reported as invalid for every read. Duplicate message identities within a capture are excluded as ambiguous. Identified customers are counted once per product and need, across sources; unidentified feedback remains evidence but does not enter the denominator.

### Need identity across reports

The current implementation uses human-reviewed mappings as its working identity policy; this is an implementation choice, not an accepted architectural decision. Each stable ID identifies an underlying need within one product. After reviewing the model labels, assign the same ID to labels that express the same need:

```sh
cargo run -- map-need --state ./ship-signal.sqlite --product demo-shop --label 'CSV export' --identity export-data
cargo run -- map-need --state ./ship-signal.sqlite --product demo-shop --label 'download data as CSV' --identity export-data
cargo run -- need-identities --state ./ship-signal.sqlite
cargo run -- review-report --state ./ship-signal.sqlite 1
```

Labels match exactly and are case-sensitive. Analysis combines labels mapped to the same ID for the same product while retaining the original classifications, variants, and messages. Labels with no mapping have `"identity": null`; a product with any unresolved labels has `"needs_identity_review": true`. Its grouping and demand are provisional, so review and map its labels before using its results for ticket decisions. Review is required even when an individual group exceeds the threshold: another label could belong to the same need.

After mapping labels, `review-report` creates a separate report from the saved feedback and classifications using the current mappings. It does not read the feedback source or call the model; `reviewed_from` identifies the original report. The original remains unchanged. To correct a mapping, run `map-need` with the current ID in `--previous-identity` and the corrected ID in `--identity`; the command fails if the current mapping has changed. Review the affected reports again as needed. The ID is not an implementation scope or a cooldown match. Earlier reports remain readable; those made before identity review was implemented are conservatively marked as requiring review. Ticket matching and cooldown similarity still require their own decisions in SUC-21 and SUC-22.

## Synthetic feedback database

The example is fictitious and uses a disposable local database; production customers supply and manage their own feedback source. With PostgreSQL utilities on `PATH`, create a local example with:

```sh
initdb -D /tmp/ship-signal-example -A trust --no-locale
pg_ctl -D /tmp/ship-signal-example -l /tmp/ship-signal-example.log -o '-p 55439 -h 127.0.0.1' start
createdb -h 127.0.0.1 -p 55439 ship_signal_feedback
psql -h 127.0.0.1 -p 55439 -d ship_signal_feedback -v ON_ERROR_STOP=1 -f fixtures/feedback.sql
```

This example uses trust authentication and is only for local development. The fixture explicitly maps its source timestamps from UTC. To run connector tests, also load `fixtures/test_invalid.sql` and set `SHIP_SIGNAL_TEST_POSTGRES` to the example reader URL. The connector tests are marked ignored by default. Run them explicitly with `cargo test --test postgres -- --ignored`; run the remaining suite with `cargo test`.

# Ship Signal

Ship Signal turns recurring customer feature requests into evidence-backed Linear tickets and implements approved work through GitHub draft pull requests.

**Feedback → weekly analysis → ticket → human approval → weekend implementation → draft PR.**

## Project status

The analysis workflow supports a chosen product and interval against a PostgreSQL or MySQL feedback view. Scheduling, ticket investigation, approval, and implementation remain future work.

## Planned capabilities

- PostgreSQL and MySQL feedback integrations through a uniform read-only schema.
- Weekly analysis of explicit customer feature requests.
- Linear ticket investigation, evidence collection, and approval checks.
- Customer-hosted weekend implementation with human-reviewed draft PRs.
- Rust & Rig orchestration and configurable models for analysis and implementation.

## Documentation

- [Glossary](GLOSSARY.md)
- [Documentation index](docs/README.md)

## Development checks

[GitHub Actions CI](.github/workflows/ci.yml) runs on pull requests targeting `main` and pushes to `main`. Two independent jobs run on standard Ubuntu runners:

- **Rust checks:** walkthrough tooling tests and document validation, then formatting, Clippy with warnings treated as errors, and application tests. Compilation is checked by Clippy and the tests.
- **Feedback database integration:** temporary PostgreSQL 17 and MySQL 8.4 services load synthetic fixtures and run both connector suites, including a saved-report comparison across engines.

Each job has a 20-minute timeout and a separate Rust build cache. A newer run for the same pull request or branch cancels its superseded run. Model responses are controlled by the tests; no AI provider credentials are needed. Formatting checks report differences without modifying files.

To run the Rust job's checks locally:

```sh
rustup component add rustfmt clippy
python3 -m unittest discover -s scripts/tests -v
python3 scripts/walkthrough.py check
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

For the integration job, load the fixtures into disposable databases as described below. Set `SHIP_SIGNAL_TEST_POSTGRES` and `SHIP_SIGNAL_TEST_MYSQL` to their reader URLs, then run:

```sh
cargo test --locked --test postgres -- --ignored
cargo test --locked --test mysql -- --ignored
```

The MySQL suite uses both databases for the report comparison. `--locked` keeps compilation on the versions recorded in `Cargo.lock`.

## Feedback analysis

Install Rust and build with `cargo build`. Choose a PostgreSQL or MySQL feedback database. This initial CLI supports an explicit interval; it does not schedule weekly runs. It stores reports in a separate local SQLite file. Back up that file using SQLite's online backup facilities or while Ship Signal is stopped. Restrict access to the file: it contains copied customer feedback and classifications. Retention and deletion policy remain open. Each saved report belongs to one product, recorded in its SQLite row alongside the report ID. This development schema expects a fresh SQLite file; no migration from earlier schemas is provided.

### Feedback view contract

Expose a **view** with these seven columns. The CLI checks the declared types and validates nullability and content on each selected row.

| Column | PostgreSQL type | MySQL type | Requirement |
| --- | --- | --- | --- |
| `product_id` | `text` | Character string | Nonblank stable product ID |
| `feedback_source_id` | `text` | Character string | Nonblank stable channel/source ID |
| `source_message_id` | `text` | Character string | Nonblank stable ID within product and source |
| `customer_id` | `text` | Character string | Nonblank stable application customer ID, or NULL |
| `submitted_at` | `timestamp with time zone` | `DATETIME` or `TIMESTAMP` | UTC-normalized instant |
| `feedback_text` | `text` | Character string | Nonblank message |
| `source_reference` | `text` | Character string | Optional locator, or NULL |

MySQL character strings are `CHAR`, `VARCHAR`, and the `TEXT` family. Cast numeric IDs and untyped NULL placeholders in the view, for example `CAST(id AS CHAR CHARACTER SET utf8mb4)` and `CAST(NULL AS CHAR(200))` in MySQL. Use `VARCHAR` or `TEXT` when trailing spaces in an ID are significant.

Map timestamps using the source's actual timezone. In PostgreSQL, use an expression such as `submitted_local AT TIME ZONE 'America/New_York'`. MySQL reads in a UTC session so `TIMESTAMP` values arrive as UTC. For MySQL `DATETIME`, expose values already stored as UTC or convert local time explicitly with `CONVERT_TZ(submitted_local, '+05:30', '+00:00')` for a source stored at that fixed offset. For named zones, load the MySQL timezone tables and use the source's actual zone so the conversion accounts for daylight-saving rules. The setup flag confirms the mapping. The type check alone cannot verify it.

Grant the connection account SELECT on the view only. Ship Signal captures feedback in a read-only transaction. PostgreSQL supports TLS through its connection settings, using `sslmode=require` to require TLS. MySQL requires verified TLS for TCP connections by default. For a private CA, pass a URL-encoded certificate path in `root_cert_path`, or install the CA in the system trust store. Use `--mysql-no-tls` for disposable local MySQL fixtures.

### Run the CLI

Set `FEEDBACK_DATABASE_URL` to choose the backend. PostgreSQL defaults to the `public` schema. MySQL uses the database in the `mysql://` URL. Use `--schema` to select another namespace or `--view` to override the default view name `ship_signal_feedback`.

```sh
export FEEDBACK_DATABASE_URL='postgresql://feedback_reader:feedback_reader@localhost:55439/ship_signal_feedback'
cargo run -- validate --product demo-shop --timestamps-mapped-to-utc
export OPENAI_API_KEY='your-key'
export SHIP_SIGNAL_ANALYSIS_MODEL='your-qualified-model'
cargo run -- analyze --product demo-shop --timestamps-mapped-to-utc \
  --start 2026-11-06T00:00:00Z --end 2026-11-13T00:00:00Z \
  --state ./ship-signal.sqlite
cargo run -- report --product demo-shop --state ./ship-signal.sqlite 1
```

For MySQL, select a MySQL URL and use the same commands. For example:

```sh
export FEEDBACK_DATABASE_URL='mysql://feedback_reader:password@mysql.example.com:3306/ship_signal_feedback'
cargo run -- validate --product demo-shop --timestamps-mapped-to-utc
```

Every command requires a nonblank `--product`. A shared view and SQLite file may contain multiple products, but each invocation operates on only the supplied product. The feedback database filters by product before Ship Signal captures messages, validates rows, or calls the classifier. Diagnostics cover only matching rows; rows with a different or NULL product ID are outside this read. Product IDs are exact, case-sensitive opaque strings. Run separate analyses for separate products.

Saved reports retain their product even when there is no feedback: an empty analysis contains one product result with a zero denominator and no needs. `report`, `review-report`, and `review-needs` require both the product and a report ID owned by that product; a mismatch fails before a model request or review decision. `need-identities` lists only the supplied product's mappings. `validate` checks the shared view's schema and timestamp-mapping confirmation; it does not check whether that product has rows. An absent product is valid and an analysis saves an empty result.

The interval is start-inclusive and end-exclusive. Only message indices and text from the selected product are sent to the configured OpenAI model for classification; feedback text may itself contain personal information. The model must support Rig structured output and should be qualified against representative feedback before production use. Each completed report includes copied inputs, classifications, supporting message indices, variants, counts, percentages, and invalid-row diagnostics. Null submission times and malformed MySQL calendar dates for the selected product cannot be assigned to an interval, so they are reported as invalid on every read for that product. Duplicate message identities within a capture are excluded as ambiguous. Identified customers are counted once per product and need, across sources; unidentified feedback remains evidence but does not enter the denominator.

### Classification response format

The prompt explicitly asks for JSON with one classification per input message:

```json
{"classifications":[{"message_index":0,"needs":[{"need":"Export orders","variant":"CSV"}]},{"message_index":1,"needs":[{"need":"Dark mode","variant":null}]},{"message_index":2,"needs":[]}]}
```

`need` is a nonblank, concise customer goal, using consistent wording and sentence case, such as `Export orders`. It is not a stable need ID. `variant` is a nonblank explicitly requested option, such as `CSV`, `PDF`, `Email`, or `SMS`, or JSON `null` when no option is stated. Preserve conventional acronym casing. Do not infer variants or use an empty string or the string `"null"`. If one message explicitly requests several variants of one need, return one entry per variant with the same need. A message may request several different needs; messages without an explicit feature request use `needs: []`. Rust checks the returned structure and indices; prompt instructions do not guarantee model interpretation or wording.

### Need identity across reports

The review queue asks the configured analysis model to suggest a match for each unresolved need. For each suggestion, it shows the candidate identities, supporting messages, requested variants, and distinct-customer count. A person confirms an existing match, chooses a different one, creates a new need ID, or leaves the need unresolved:

```sh
export OPENAI_API_KEY='your-key'
export SHIP_SIGNAL_ANALYSIS_MODEL='your-qualified-model'
cargo run -- review-needs --product demo-shop --state ./ship-signal.sqlite --report 1
```

Suggestions are never saved as mappings without a person choosing an action. Confirmed mappings and deferrals are saved as they are made; quit with `q` and run the same command to resume. Add `--include-deferred` to revisit needs previously left unresolved for that report. Once the queue has been reviewed, the command saves a separate report from the original feedback and classifications. The original report is unchanged. Deferred needs remain unresolved. For match suggestions the model receives the product ID, need text, candidate IDs and their mapped needs, supporting feedback text, variants, and the distinct-customer count. Feedback may contain personal information. Choose and qualify the model for the task before production use.

The working identity policy is an implementation choice, not an accepted architectural decision. Each stable ID identifies an underlying need within one product. Direct mappings are also available:

```sh
cargo run -- map-need --state ./ship-signal.sqlite --product demo-shop --need 'CSV export' --identity export-data
cargo run -- need-identities --product demo-shop --state ./ship-signal.sqlite
cargo run -- review-report --product demo-shop --state ./ship-signal.sqlite 1
```

Need-to-identity lookup ignores case using Rust Unicode lowercasing: `Export orders`, `export orders`, and `EXPORT ORDERS` reuse the same confirmed mapping within a product. Analyze, saved-report review, assignment, and guarded correction use this rule. Original need/variant text is preserved; product IDs and identity IDs remain case-sensitive. Matching does not trim spaces, remove punctuation, infer synonyms, or perform full Unicode case folding (for example, `Straße` and `STRASSE` remain different). Without a confirmed mapping, differently cased needs can still be separate unresolved groups. Case-only duplicate assignments to the same identity reuse the existing row; conflicting identities fail. Mapping changes are transactional, and duplicate case-equivalent rows introduced outside Ship Signal produce an error rather than an arbitrary match. Analysis combines differently worded needs mapped to the same ID for the same product while retaining the original classifications, variants, and messages. Needs with no mapping have `"identity": null`; a product with any unresolved needs has `"needs_identity_review": true`. Its grouping and demand are provisional, so review and map its needs before using its results for ticket decisions. Review is required even when an individual group exceeds the threshold: another request could describe the same need.

After mapping needs, `review-report` creates a separate report from the saved feedback and classifications using the current mappings. It does not read the feedback source or call the model; `reviewed_from` identifies the original report. The original remains unchanged. To correct a mapping, run `map-need` with the current ID in `--previous-identity` and the corrected ID in `--identity`; the command fails if the current mapping has changed. Review the affected reports again as needed. The ID is not an implementation scope or a cooldown match. Ticket matching and cooldown similarity still require their own decisions in SUC-21 and SUC-22.

## Synthetic feedback databases

The example is fictitious and uses a disposable local database; production customers supply and manage their own feedback source. With PostgreSQL utilities on `PATH`, create a local example with:

```sh
initdb -D /tmp/ship-signal-example -A trust --no-locale
pg_ctl -D /tmp/ship-signal-example -l /tmp/ship-signal-example.log -o '-p 55439 -h 127.0.0.1' start
createdb -h 127.0.0.1 -p 55439 ship_signal_feedback
psql -h 127.0.0.1 -p 55439 -d ship_signal_feedback -v ON_ERROR_STOP=1 -f fixtures/feedback.sql
```

This example uses trust authentication and is only for local development. The fixture explicitly maps its source timestamps from UTC. To run connector tests, also load `fixtures/test_invalid.sql` and set `SHIP_SIGNAL_TEST_POSTGRES` to the example reader URL. The connector tests are marked ignored by default. Run them explicitly with `cargo test --test postgres -- --ignored`; run the remaining suite with `cargo test`.

On a disposable MySQL 8.0 or 8.4 instance, load the fixture as an administrator. The fixture creates the database and reader account and sets a non-UTC server default to exercise UTC mapping.

```sh
mysql --host=127.0.0.1 --port=3306 --user=root --password < fixtures/mysql_feedback.sql
export SHIP_SIGNAL_TEST_MYSQL='mysql://feedback_reader:feedback_reader@127.0.0.1:3306/ship_signal_feedback?prefer_socket=false'
FEEDBACK_DATABASE_URL="$SHIP_SIGNAL_TEST_MYSQL" cargo run -- validate \
  --product demo-shop --timestamps-mapped-to-utc --mysql-no-tls
```

The current development schema uses `need` consistently in classification/report JSON, mapping and deferral columns, and `map-need --need`. It expects a fresh SQLite state file; no migration or old-field aliases are provided for the earlier `name`/`label` format.

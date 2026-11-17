-- Edge cases for the connector integration test.
CREATE VIEW ship_signal_invalid_feedback AS
SELECT * FROM (VALUES
 ('shop'::text, 'email'::text, 'valid'::text, 'alice'::text, '2026-11-07T00:00:00Z'::timestamptz, 'Please add CSV.'::text, NULL::text),
 ('shop', 'email', 'duplicate', 'alice', '2026-11-07T00:00:00Z'::timestamptz, 'One', NULL),
 ('shop', 'email', 'duplicate', 'bob', '2026-11-07T00:00:00Z'::timestamptz, 'Two', NULL),
 ('shop', 'email', 'empty', 'bob', '2026-11-07T00:00:00Z'::timestamptz, ' ', NULL),
 ('shop', 'email', 'missing-time', 'bob', NULL::timestamptz, 'Please add a feature', NULL)
) AS f(product_id, feedback_source_id, source_message_id, customer_id, submitted_at, feedback_text, source_reference);
GRANT SELECT ON ship_signal_invalid_feedback TO feedback_reader;

-- A source stored as local wall-clock time must be explicitly mapped.
CREATE VIEW ship_signal_local_time_feedback AS
SELECT 'shop'::text AS product_id, 'email'::text AS feedback_source_id,
 'local-time'::text AS source_message_id, 'alice'::text AS customer_id,
 timestamp '2026-11-07 09:00:00' AT TIME ZONE 'Asia/Kolkata' AS submitted_at,
 'Please add CSV.'::text AS feedback_text, NULL::text AS source_reference;
GRANT SELECT ON ship_signal_local_time_feedback TO feedback_reader;

-- Product-scoped reads must isolate both valid feedback and invalid-row diagnostics.
CREATE OR REPLACE VIEW ship_signal_product_scoped_feedback AS
SELECT * FROM (VALUES
 ('shop'::text, 'email'::text, 'shared'::text, 'alice'::text, '2026-11-07T00:00:00Z'::timestamptz, 'Please add CSV.'::text, NULL::text),
 ('other-shop', 'email', 'shared', 'mallory', '2026-11-07T00:00:00Z'::timestamptz, 'FOREIGN_PRODUCT_MESSAGE', NULL),
 ('shop', 'email', 'missing-time', 'bob', NULL::timestamptz, 'Please add a feature', NULL),
 ('other-shop', 'email', 'foreign-missing-time', 'mallory', NULL::timestamptz, 'Foreign invalid time', NULL),
 ('other-shop', 'email', 'foreign-empty', 'mallory', '2026-11-07T00:00:00Z'::timestamptz, ' ', NULL),
 (NULL, 'email', 'unattributable', 'unknown', '2026-11-07T00:00:00Z'::timestamptz, 'No product', NULL)
) AS f(product_id, feedback_source_id, source_message_id, customer_id, submitted_at, feedback_text, source_reference);
GRANT SELECT ON ship_signal_product_scoped_feedback TO feedback_reader;

-- Synthetic customer application data. Never use these credentials for a real source.
CREATE TABLE customer_feedback (
  id integer PRIMARY KEY,
  customer_id text,
  channel text NOT NULL,
  submitted_local timestamp NOT NULL,
  message text NOT NULL
);
INSERT INTO customer_feedback VALUES
 (1, 'alice', 'email', '2026-11-06 10:00:00', 'Please add CSV exports.'),
 (2, 'alice', 'chat', '2026-11-07 11:00:00', 'I would love a CSV download.'),
 (3, 'bob', 'email', '2026-11-08 12:00:00', 'Please add PDF exports.'),
 (4, 'carol', 'chat', '2026-11-09 13:00:00', 'Please add dark mode.'),
 (5, NULL, 'email', '2026-11-10 14:00:00', 'Please add CSV exports.'),
 (6, 'dave', 'chat', '2026-11-11 15:00:00', 'The checkout page crashes.');
-- The synthetic source timestamps are explicitly in UTC.
CREATE VIEW ship_signal_feedback AS
SELECT 'demo-shop'::text AS product_id, channel::text AS feedback_source_id,
 id::text AS source_message_id, customer_id::text AS customer_id,
 submitted_local AT TIME ZONE 'UTC' AS submitted_at, message::text AS feedback_text,
 NULL::text AS source_reference
FROM customer_feedback;
CREATE ROLE feedback_reader LOGIN PASSWORD 'feedback_reader';
GRANT USAGE ON SCHEMA public TO feedback_reader;
GRANT SELECT ON ship_signal_feedback TO feedback_reader;

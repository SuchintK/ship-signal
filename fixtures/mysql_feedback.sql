-- Synthetic data and credentials for a disposable MySQL server only.
CREATE DATABASE IF NOT EXISTS ship_signal_feedback CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
USE ship_signal_feedback;

-- Exercise the reader against a non-UTC server default.
SET GLOBAL time_zone = '+09:00';
SET SESSION time_zone = '+00:00';

CREATE TABLE customer_feedback (
  id integer PRIMARY KEY,
  customer_id varchar(100),
  channel varchar(100) NOT NULL,
  submitted_local datetime(6) NOT NULL,
  message text NOT NULL
);
INSERT INTO customer_feedback VALUES
 (1, 'alice', 'email', '2026-11-06 10:00:00', 'Please add CSV exports.'),
 (2, 'alice', 'chat', '2026-11-07 11:00:00', 'I would love a CSV download.'),
 (3, 'bob', 'email', '2026-11-08 12:00:00', 'Please add PDF exports.'),
 (4, 'carol', 'chat', '2026-11-09 13:00:00', 'Please add dark mode.'),
 (5, NULL, 'email', '2026-11-10 14:00:00', 'Please add CSV exports.'),
 (6, 'dave', 'chat', '2026-11-11 15:00:00', 'The checkout page crashes.');
CREATE VIEW ship_signal_feedback AS
SELECT 'demo-shop' AS product_id, channel AS feedback_source_id,
 CAST(id AS CHAR CHARACTER SET utf8mb4) AS source_message_id, customer_id,
 CONVERT_TZ(submitted_local, '+00:00', '+00:00') AS submitted_at,
 message AS feedback_text, CAST(NULL AS CHAR(200)) AS source_reference
FROM customer_feedback;

CREATE TABLE scoped_feedback (
 product_id varchar(100),
 feedback_source_id varchar(100),
 source_message_id varchar(100),
 customer_id varchar(100),
 submitted_at datetime(6),
 feedback_text text,
 source_reference varchar(200)
);
INSERT INTO scoped_feedback VALUES
 ('shop', 'email', 'valid', 'alice', '2026-11-07 00:00:00', 'Please add CSV.', NULL),
 ('shop', 'email', 'duplicate', 'alice', '2026-11-07 00:00:00', 'One', NULL),
 ('shop', 'email', 'duplicate', 'bob', '2026-11-07 00:00:00', 'Two', NULL),
 ('shop', 'email', 'empty', 'bob', '2026-11-07 00:00:00', ' ', NULL),
 ('shop', 'email', 'missing-time', 'bob', NULL, 'Please add a feature', NULL),
 ('shop', 'email', 'shared', 'alice', '2026-11-07 00:00:00', 'Please add CSV.', NULL),
 ('other-shop', 'email', 'shared', 'mallory', '2026-11-07 00:00:00', 'FOREIGN_PRODUCT_MESSAGE', NULL),
 ('other-shop', 'email', 'foreign-missing-time', 'mallory', NULL, 'Foreign invalid time', NULL),
 ('other-shop', 'email', 'foreign-empty', 'mallory', '2026-11-07 00:00:00', ' ', NULL),
 (NULL, 'email', 'unattributable', 'unknown', '2026-11-07 00:00:00', 'No product', NULL),
 ('SHOP', 'email', 'case-variant', 'mallory', '2026-11-07 00:00:00', ' ', NULL),
 ('shop ', 'email', 'space-variant', 'mallory', '2026-11-07 00:00:00', ' ', NULL),
 ('shóp', 'email', 'accent-variant', 'mallory', '2026-11-07 00:00:00', ' ', NULL);
CREATE VIEW ship_signal_invalid_feedback AS
SELECT * FROM scoped_feedback WHERE source_message_id IN ('valid', 'duplicate', 'empty', 'missing-time');
CREATE VIEW ship_signal_product_scoped_feedback AS
SELECT * FROM scoped_feedback WHERE source_message_id NOT IN ('valid', 'duplicate', 'empty');

CREATE VIEW ship_signal_local_time_feedback AS
SELECT 'shop' AS product_id, 'email' AS feedback_source_id,
 'local-time' AS source_message_id, CAST(NULL AS CHAR(100)) AS customer_id,
 CONVERT_TZ(CAST('2026-11-07 09:00:00.123456' AS DATETIME(6)), '+05:30', '+00:00') AS submitted_at,
 'Please add CSV.' AS feedback_text, 'support/42' AS source_reference;

CREATE TABLE timestamp_feedback (submitted_at timestamp(6) NOT NULL);
INSERT INTO timestamp_feedback VALUES ('2026-11-07 03:30:00.123456');
CREATE VIEW ship_signal_timestamp_feedback AS
SELECT 'shop' AS product_id, 'email' AS feedback_source_id,
 'local-time' AS source_message_id, CAST(NULL AS CHAR(100)) AS customer_id,
 submitted_at, 'Please add CSV.' AS feedback_text, 'support/42' AS source_reference
FROM timestamp_feedback;

SET SESSION sql_mode = 'ALLOW_INVALID_DATES';
CREATE TABLE legacy_feedback (submitted_at datetime(6));
INSERT INTO legacy_feedback VALUES ('0000-00-00'), ('2026-10-00'), ('2026-02-31');
CREATE VIEW legacy_dates AS
SELECT 'shop' AS product_id, 'email' AS feedback_source_id,
 'legacy' AS source_message_id, CAST(NULL AS CHAR(100)) AS customer_id,
 submitted_at, 'Please add CSV.' AS feedback_text, CAST(NULL AS CHAR(200)) AS source_reference
FROM legacy_feedback;

CREATE VIEW missing_column AS SELECT product_id FROM ship_signal_feedback;
CREATE VIEW wrong_type AS SELECT product_id, feedback_source_id, source_message_id,
 1 AS customer_id, submitted_at, feedback_text, source_reference FROM ship_signal_feedback;
CREATE VIEW no_select AS SELECT * FROM ship_signal_feedback;

CREATE USER 'feedback_reader'@'%' IDENTIFIED BY 'feedback_reader';
GRANT SELECT ON ship_signal_feedback TO 'feedback_reader'@'%';
GRANT SELECT ON ship_signal_invalid_feedback TO 'feedback_reader'@'%';
GRANT SELECT ON ship_signal_product_scoped_feedback TO 'feedback_reader'@'%';
GRANT SELECT ON ship_signal_local_time_feedback TO 'feedback_reader'@'%';
GRANT SELECT ON ship_signal_timestamp_feedback TO 'feedback_reader'@'%';
GRANT SELECT ON legacy_dates TO 'feedback_reader'@'%';
GRANT SELECT ON missing_column TO 'feedback_reader'@'%';
GRANT SELECT ON wrong_type TO 'feedback_reader'@'%';

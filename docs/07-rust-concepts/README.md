# Rust concepts

Beginner-friendly explanations of Rust concepts encountered while reading Ship Signal's code. Open each HTML lesson in a browser; each file is self-contained and works offline.

## Lessons

Read in order, or jump to the concept you need.

| Lesson | Topic | What it explains |
| --- | --- | --- |
| 01 | [Variables and bindings](01-variables-and-bindings.html) | Understand what a name refers to, what mut allows, and how a variable differs from a reference. |
| 02 | [Shared and mutable references](02-references-and-borrowing.html) | Borrow a value to read it or change it without taking ownership. |
| 03 | [Structs and fields](03-structs-and-fields.html) | Read the named pieces of a Rust value, and distinguish an in-memory type from a database table. |
| 04 | [Option and optional-value validation](04-option-and-validation.html) | Understand Some, None, and why an optional string can still need validation. |
| 05 | [Result, error propagation, and ensure!](05-results-and-errors.html) | Read success and failure paths without confusing an ordinary error with a panic. |
| 06 | [Functions, closures, and the classifier boundary](06-functions-and-closures.html) | See how the CLI supplies model behavior while Workflow keeps orchestration and counting in one place. |
| 07 | [Vectors, validation, and sorting](07-vectors-validation-and-sorting.html) | Understand why sorting changes order, why length is checked first, and how message indices connect classifications to feedback. |

Ship Signal source excerpts are pinned to commit `217ee8f20d7e561a25062331f99dc8fba99a1d59`. Links to those excerpts open GitHub, so the lessons can also be read from `main` before the implementation branch is merged.

Add future concepts as separate HTML files in this folder and link them in this index. Each explanation should introduce the concept, show a small Rust example, and connect it to Ship Signal where useful.

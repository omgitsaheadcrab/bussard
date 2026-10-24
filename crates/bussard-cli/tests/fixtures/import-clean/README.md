# import-clean

Expectation files of the corpus test `tests/it/import_corpus_clean.rs`
(issue #348): `bussard validate` after `bussard import` of a corpus project
reports zero errors and zero warnings, except the `CODE location` lines its
`<stem>.expect` lists. Each file states the ETS-level reason. A stem's
characters outside `[A-Za-z0-9._-]` become `_`.

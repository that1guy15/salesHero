-- 0003_declared_documents.sql — a document can be declared before its content
-- is available.
--
-- docs/DECISIONS.md D13. `03-DATA-MODEL.md` §3 embeds document content in the
-- `.playbook` zip, and `documents.bytes` is NOT NULL because that content is
-- the thing being stored. But the unit of import today is a bare
-- `playbook.yaml` (the zip container is deferred to M7), and a bare YAML
-- carries only a *manifest* of documents: id, title, filename, sha256.
--
-- With bytes NOT NULL there was nowhere to put that manifest, so the
-- `documents:` block and every `doc_refs:` entry were parsed and silently
-- dropped, and `objection_docs` was never written to at all.
--
-- `bytes` and `ingested_at` become nullable: NULL means "declared by a
-- playbook, content not yet ingested". M3 and M7 fill them in.
--
-- SQLite cannot drop a NOT NULL constraint in place, so this rebuilds the three
-- tables. That is safe here without a data copy for a specific reason: nothing
-- in the codebase has ever written to `documents`, `doc_chunks` or
-- `objection_docs` — that omission is the bug this migration exists to fix — so
-- every database at schema version 2 has all three empty. Do not copy this
-- pattern for a table that holds data.
--
-- Children first, so the drops do not trip foreign key checks.

DROP TABLE objection_docs;
DROP TABLE doc_chunks;
DROP TABLE documents;

CREATE TABLE documents (
    id              TEXT PRIMARY KEY,
    playbook_id     TEXT NOT NULL REFERENCES playbooks(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    filename        TEXT NOT NULL,
    mime            TEXT NOT NULL,
    sha256          TEXT NOT NULL,          -- '' until the content is ingested
    bytes           BLOB,                   -- NULL = declared, not ingested
    declared_at     INTEGER NOT NULL,
    ingested_at     INTEGER                 -- NULL until bytes arrive
);

CREATE TABLE doc_chunks (
    id              INTEGER PRIMARY KEY,
    document_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    ord             INTEGER NOT NULL,
    text            TEXT NOT NULL,
    token_estimate  INTEGER NOT NULL,
    embedding       BLOB,
    embed_model     TEXT
);

CREATE INDEX idx_doc_chunks_document ON doc_chunks(document_id);

CREATE TABLE objection_docs (
    objection_id    TEXT NOT NULL REFERENCES objections(id) ON DELETE CASCADE,
    document_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    PRIMARY KEY (objection_id, document_id)
);

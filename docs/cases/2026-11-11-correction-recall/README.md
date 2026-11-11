# Case 2026-11-11 — correction recall in siliceo-memory-rust

**Question (Casa Bi):** when the original assertion is retrieved, does its correction emerge with it? Conserving both is not enough: they must be found and read together.

**Environment.** `siliceo-memory-rust 3.2.1-RUST-dev`; isolated instance (`/tmp/agora-case.db`, port 3099); `STORAGE_ENGINE=sql-pure`; `STATE_CHAINS=on`; `EMBEDDING_MODE=local`. No production data is touched.

**Memories.**
- `original`: «La porta del laboratorio era chiusa.»
- `correction`: «La porta del laboratorio era aperta.» — stored with `supersedes: [original]`.
- `negative`: «Le maree seguono la luna e il sole.»

**Conditions tested.**
1. `search` default (no `with_chain`, no `include_history`).
2. `search` with `with_chain=true`.
3. `search` with `include_history=true` + `with_chain=true`.
4. `retrieve` with `with_chain=true`.
5. `retrieve` with `include_history=true` + `with_chain=true`.
6. Negative query on the negative memory (limit 1).

**Observed (see `report.json`).**
| condition | result |
|---|---|
| search default | correction only; `supersedes_history: []` |
| search `with_chain` | correction + `supersedes_history: [original]` |
| search `include_history` + `with_chain` | correction (history: original) and original (history: empty) |
| retrieve `with_chain` | correction + history original |
| retrieve `include_history` + `with_chain` | both; original history empty |
| negative query | negative only (sim 1.0) |

**Finding.** The chain is **backward**: the correction (current) carries the original as its history; the original does not carry the correction. The default retrieval hides the original because it is superseded. The correction and the original appear together only when the caller asks for the correction (`with_chain=true`), or explicitly includes superseded versions (`include_history=true`). When the **original** is retrieved with `include_history=true`, the correction does **not** emerge: its `supersedes_history` is empty. This is the failure the question was looking for.

**Limits.** This is a `sql-pure` test instance, not the production Qdrant path; it requires the Memory Server host to run. The script is public, the report is the observed output. External reproduction needs access to the host (or a container/image), not just the repo.

**Reproduce.** On the Memory Server host:
```bash
ROOT=~/siliceo-memory-rust bash run_case.sh
```
It writes `/tmp/agora-case-report.json` and stops the isolated instance.

**Pure SQL reproduction (no host needed).** `case.py` reproduces the relation and the default filter with Python's `sqlite3` only; no server, no embeddings, no container. It prints:
- `default_current`: correction only;
- `including_all`: original and correction;
- `history_correction`: original; `history_original`: empty;
- `superseded_by_original_proposed`: correction (the proposed forward link, not implemented);
- `negative_query`: negative only.

Run: `python3 case.py` (sha256 `f1833fee5f9eac783666f2288a86b683d62f225fb58b63dd713503caef7fce50`).

**Qdrant path (isolated).** `run_case_qdrant.sh` runs the same case against a separate Qdrant instance (HTTP 6344, gRPC 6345, storage `/tmp/qdrant-case-storage`, collection `case-test`) and a separate Memory Server instance (port 3098, `STORAGE_ENGINE=qdrant`, `STATE_CHAINS=on`, local embeddings). The SQLite DB is temporary; no production data is used. Results are identical to the SQL case: default = correction; `with_chain` = correction + original; `include_history` = both, original history empty; negative separate. The vector ranking changes the scores (correction 0.762, original 0.596), not the direction. See `report_qdrant.json`.

**Files.** `run_case.sh` (API script), `report.json` (observed output, sha256 `d8052d...`), `run_case_qdrant.sh` (Qdrant path), `report_qdrant.json`. `run_case.sh` (API script), `report.json` (observed output, sha256 `d8052d119656aace1caa20bfb285bb5e9573fe1e919bf01ab919623f9778e082`), `case.py` (pure SQL reproduction).

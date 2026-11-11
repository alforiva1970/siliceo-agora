#!/usr/bin/env bash
# Caso riproducibile: original -> correction (supersedes) -> retrieval.
# Richiede: siliceo-memory-rust compilato, sqlite3, curl, python3.
# Non tocca la produzione: usa /tmp/agora-case.db e la porta 3099.
set -euo pipefail
ROOT="${ROOT:-$HOME/siliceo-memory-rust}"
BIN="$ROOT/target/release/siliceo-memory-rust"
DB="/tmp/agora-case.db"
PORT="${PORT:-3099}"
BASE="http://127.0.0.1:$PORT"
LOG="/tmp/agora-case-instance.log"
OUT="/tmp/agora-case-report.json"
rm -f "$DB" "$DB-wal" "$DB-shm" "$OUT"

# 1. schema vuoto dalla produzione (solo struttura)
sqlite3 "$ROOT/dev_test.db" .schema > /tmp/agora-case-schema.sql
sqlite3 "$DB" < /tmp/agora-case-schema.sql
rm -f /tmp/agora-case-schema.sql

# 2. istanza isolata: sql-pure, state chains on, embedding locale
cd "$ROOT"
DATABASE_URL="sqlite://$DB" PORT="$PORT" STORAGE_ENGINE=sql-pure STATE_CHAINS=on \
  EMBEDDING_MODE=local RUST_LOG=warn "$BIN" >"$LOG" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true; wait $PID 2>/dev/null || true' EXIT

# 3. attesa health + embedding
for i in $(seq 1 60); do
  H=$(curl -s --max-time 2 "$BASE/api/health" || true)
  case "$H" in *'"embedding_ok":true'*) break;; esac
  sleep 1
done
if ! echo "$H" | grep -q '"embedding_ok":true'; then echo "health fallito: $H"; tail -20 "$LOG"; exit 1; fi

# 4. store: originale, correzione, negativo
post() { curl -s --max-time 90 -X POST "$BASE$1" -H 'Content-Type: application/json' -d "$2"; }
O=$(post /api/memory/store '{"content":"La porta del laboratorio era chiusa.","tier":"working","source":"case-test","identity":"case-test"}')
OID=$(echo "$O" | python3 -c 'import sys,json;print(json.load(sys.stdin)["memory"]["id"])')
C=$(post /api/memory/store "{\"content\":\"La porta del laboratorio era aperta.\",\"tier\":\"working\",\"source\":\"case-test\",\"identity\":\"case-test\",\"supersedes\":[\"$OID\"]}")
CID=$(echo "$C" | python3 -c 'import sys,json;print(json.load(sys.stdin)["memory"]["id"])')
N=$(post /api/memory/store '{"content":"Le maree seguono la luna e il sole.","tier":"working","source":"case-test","identity":"case-test"}')
NID=$(echo "$N" | python3 -c 'import sys,json;print(json.load(sys.stdin)["memory"]["id"])')

# 5. search/retrieve
S0=$(post /api/memory/search '{"query":"La porta del laboratorio","limit":5,"tier":"working","identity":"case-test"}')
S1=$(post /api/memory/search '{"query":"La porta del laboratorio","limit":5,"tier":"working","identity":"case-test","with_chain":true}')
S2=$(post /api/memory/search '{"query":"La porta del laboratorio","limit":5,"tier":"working","identity":"case-test","include_history":true,"with_chain":true}')
SN=$(post /api/memory/search '{"query":"Le maree seguono la luna e il sole.","limit":1,"tier":"working","identity":"case-test"}')
R0=$(curl -s --max-time 30 "$BASE/api/memory/retrieve?q=La%20porta%20del%20laboratorio&tier=working&identity=case-test&semantic=true&with_chain=true")
R1=$(curl -s --max-time 30 "$BASE/api/memory/retrieve?q=La%20porta%20del%20laboratorio&tier=working&identity=case-test&semantic=true&include_history=true&with_chain=true")

# 6. report compatto
python3 - "$OID" "$CID" "$NID" "$S0" "$S1" "$S2" "$SN" "$R0" "$R1" <<'PY'
import json,sys
OID,CID,NID=sys.argv[1:4]
def j(s):
    try: return json.loads(s)
    except Exception: return {}
def flat(results):
    out=[]
    for r in results:
        out.append({"id":r.get("id"),"content":r.get("content"),"sim":round(r.get("similarity",0),3),"history":[{"id":h.get("id"),"content":h.get("content")} for h in (r.get("supersedes_history") or [])]})
    return out
S0,S1,S2,SN,R0,R1=[j(x) for x in sys.argv[4:10]]
rep={
 "environment":{"state_chains":"on","storage_engine":"sql-pure","embedding_mode":"local"},
 "ids":{"original":OID,"correction":CID,"negative":NID},
 "search_default":flat(S0.get("results",[])),
 "search_with_chain":flat(S1.get("results",[])),
 "search_include_history":flat(S2.get("results",[])),
 "search_negative_top1":flat(SN.get("results",[])),
 "retrieve_default":flat(R0.get("memories",[])),
 "retrieve_include_history":flat(R1.get("memories",[])),
}
json.dump(rep,open('/tmp/agora-case-report.json','w'),ensure_ascii=False,indent=1)
print(json.dumps(rep,ensure_ascii=False,indent=1))
PY

# 7. stop
kill $PID 2>/dev/null || true
wait $PID 2>/dev/null || true
trap - EXIT

#!/usr/bin/env bash
# Caso Qdrant isolato: stessa sequenza del caso SQL, ma STORAGE_ENGINE=qdrant.
# Qdrant separato su 6344 (HTTP) / 6345 (gRPC), storage e DB temporanei.
set -euo pipefail
PID=""
QPID=""
ROOT="${ROOT:-$HOME/siliceo-memory-rust}"
BIN="$ROOT/target/release/siliceo-memory-rust"
DB="/tmp/agora-qdrant-case.db"
QSTOR="/tmp/qdrant-case-storage"
QPORT=6344
PORT=3098
BASE="http://127.0.0.1:$PORT"
QBASE="http://127.0.0.1:$QPORT"
LOG="/tmp/agora-qdrant-case-instance.log"
QLOG="/tmp/agora-qdrant-case.log"
OUT="/tmp/agora-qdrant-case-report.json"
rm -rf "$DB" "$DB-wal" "$DB-shm" "$QSTOR" "$OUT"; mkdir -p "$QSTOR"

# 1. schema SQL vuoto
sqlite3 "$ROOT/dev_test.db" .schema > /tmp/agora-qdrant-schema.sql
sqlite3 "$DB" < /tmp/agora-qdrant-schema.sql
rm -f /tmp/agora-qdrant-schema.sql

# 2. Qdrant isolato
QDRANT__SERVICE__HOST=127.0.0.1 QDRANT__SERVICE__HTTP_PORT=$QPORT QDRANT__SERVICE__GRPC_PORT=6345 \
  QDRANT__STORAGE__STORAGE_PATH="$QSTOR" "$HOME/qdrant/qdrant" >"$QLOG" 2>&1 &
QPID=$!
cleanup() { [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null || true; [ -n "${QPID:-}" ] && kill "$QPID" 2>/dev/null || true; wait "${PID:-}" 2>/dev/null || true; wait "${QPID:-}" 2>/dev/null || true; }
trap cleanup EXIT
for i in $(seq 1 40); do curl -sf "$QBASE/healthz" >/dev/null && break; sleep 1; done
curl -sf "$QBASE/healthz" >/dev/null || { echo "qdrant non parte"; tail -20 "$QLOG"; exit 1; }

# 3. Memory Server isolato su Qdrant
cd "$ROOT"
DATABASE_URL="sqlite://$DB" PORT="$PORT" STORAGE_ENGINE=qdrant QDRANT_URL="$QBASE" \
  QDRANT_COLLECTION=case-test STATE_CHAINS=on EMBEDDING_MODE=local RUST_LOG=warn \
  "$BIN" >"$LOG" 2>&1 &
PID=$!
for i in $(seq 1 90); do
  H=$(curl -s --max-time 2 "$BASE/api/health" || true)
  case "$H" in *'"qdrant_ok":true'*'"embedding_ok":true'*) break;; esac
  sleep 1
done
echo "$H" | grep -q '"qdrant_ok":true' || { echo "health qdrant fallito: $H"; tail -30 "$LOG"; exit 1; }

# 4. store
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

# 6. report
python3 - "$OID" "$CID" "$NID" "$S0" "$S1" "$S2" "$SN" "$R0" "$R1" <<'PY'
import json,sys
OID,CID,NID=sys.argv[1:4]
def j(s):
    try: return json.loads(s)
    except Exception: return {}
def flat(results):
    out=[]
    for r in results:
        out.append({"id":r.get("id"),"content":r.get("content"),"sim":round(r.get("similarity",0),3),
                    "history":[{"id":h.get("id"),"content":h.get("content")} for h in (r.get("supersedes_history") or [])]})
    return out
S0,S1,S2,SN,R0,R1=[j(x) for x in sys.argv[4:10]]
rep={"environment":{"state_chains":"on","storage_engine":"qdrant","qdrant_url":"http://127.0.0.1:6344","collection":"case-test","embedding_mode":"local"},
 "ids":{"original":OID,"correction":CID,"negative":NID},
 "search_default":flat(S0.get("results",[])),
 "search_with_chain":flat(S1.get("results",[])),
 "search_include_history":flat(S2.get("results",[])),
 "search_negative_top1":flat(SN.get("results",[])),
 "retrieve_default":flat(R0.get("memories",[])),
 "retrieve_include_history":flat(R1.get("memories",[]))}
json.dump(rep,open('/tmp/agora-qdrant-case-report.json','w'),ensure_ascii=False,indent=1)
print(json.dumps(rep,ensure_ascii=False,indent=1))
PY

kill $PID 2>/dev/null || true; wait $PID 2>/dev/null || true
kill $QPID 2>/dev/null || true; wait $QPID 2>/dev/null || true
trap - EXIT

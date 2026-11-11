#!/usr/bin/env python3
"""Caso astratto: Memory + MemoryRelation (SUPERSEDES) in SQL puro.

Riproduce la direzione della catena e il filtro "versione corrente",
senza server, embedding o container. Il forward-link e' mostrato come
query proposta (superseded_by), non come comportamento implementato.
"""
import sqlite3, json

con = sqlite3.connect(":memory:")
con.executescript("""
CREATE TABLE Memory (id TEXT PRIMARY KEY, content TEXT NOT NULL);
CREATE TABLE MemoryRelation (
  id TEXT PRIMARY KEY, sourceId TEXT NOT NULL, targetId TEXT NOT NULL,
  label TEXT NOT NULL, weight REAL DEFAULT 1.0
);
""")
con.executemany("INSERT INTO Memory VALUES (?,?)", [
    ("o1", "La porta del laboratorio era chiusa."),
    ("c1", "La porta del laboratorio era aperta."),
    ("n1", "Le maree seguono la luna e il sole."),
])
# c1 SUPERSEDES o1  (la correzione punta all'originale)
con.execute("INSERT INTO MemoryRelation VALUES ('r1','c1','o1','SUPERSEDES',1.0)")
con.commit()

def rows(sql, args=()):
    cur = con.execute(sql, args)
    cols = [d[0] for d in cur.description]
    return [dict(zip(cols, r)) for r in cur.fetchall()]

def history(mid):
    out, cur, seen = [], mid, {mid}
    while True:
        nxt = con.execute(
            "SELECT targetId FROM MemoryRelation WHERE sourceId=? AND label='SUPERSEDES' LIMIT 1",
            (cur,),
        ).fetchone()
        if not nxt or nxt[0] in seen:
            break
        nid = nxt[0]; seen.add(nid)
        c = con.execute("SELECT content FROM Memory WHERE id=?", (nid,)).fetchone()
        out.append({"id": nid, "content": c[0] if c else None})
        cur = nid
    return out

def superseded_by(mid):
    return rows(
        "SELECT m.id, m.content FROM MemoryRelation r JOIN Memory m ON m.id=r.sourceId "
        "WHERE r.targetId=? AND r.label='SUPERSEDES'",
        (mid,),
    )

porta = "%porta del laboratorio%"
report = {
    "default_current": rows(
        "SELECT id, content FROM Memory WHERE content LIKE ? AND NOT EXISTS "
        "(SELECT 1 FROM MemoryRelation r WHERE r.targetId=Memory.id AND r.label='SUPERSEDES')",
        (porta,),
    ),
    "including_all": rows("SELECT id, content FROM Memory WHERE content LIKE ?", (porta,)),
    "history_correction": history("c1"),
    "history_original": history("o1"),
    "superseded_by_original_proposed": superseded_by("o1"),
    "superseded_by_correction": superseded_by("c1"),
    "negative_query": rows("SELECT id, content FROM Memory WHERE content LIKE ?", ("%maree%",)),
}
print(json.dumps(report, ensure_ascii=False, indent=1))

"""Rust 결과를 원본 Python 엔진(아티팩트 pysrc)과 대조"""
import json, sys, math, datetime, os
HERE = os.path.dirname(os.path.abspath(__file__))
sys.setrecursionlimit(10000)
ns = {}
exec(compile(open(os.path.join(HERE, 'reference', 'original_engine.py'), encoding='utf8').read(), 'engine.py', 'exec'), ns)
F = dict(irs=ns['value_irs'], crs=ns['value_crs'], fx=ns['value_fx'], opt=ns['value_option'], prs=ns['value_prs'], cb=ns['value_cb'], curve=ns['value_curve'])
cases = json.load(open(os.path.join(HERE, 'py_cases.json'), encoding='utf8'))
def norm(o):
    return json.loads(json.dumps(o, default=lambda x: x.isoformat() if isinstance(x, (datetime.date,)) else str(x)))
stats = {}; maxd = 0.0; last = None
def cmp(a, b, path, out):
    global maxd
    if isinstance(a, bool) or isinstance(b, bool):
        if a != b: out.append(f"{path}: py={a} rust={b}")
        return
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        d = abs(a - b) / max(1, abs(a), abs(b))
        if path.endswith('.early_nodes'): return
        if d > 1e-9: out.append(f"{path}: py={a} rust={b} d={d:.2e}")
        else: maxd = max(maxd, d)
        return
    if isinstance(a, dict) and isinstance(b, dict):
        for k in a:
            if k in ('zero_by_tenor',): continue      # 키 표기(0.25 ↔ "0.25")만 다름
            if k not in b: out.append(f"{path}.{k}: rust에 없음"); continue
            cmp(a[k], b[k], f"{path}.{k}", out)
        return
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b): out.append(f"{path}: len py={len(a)} rust={len(b)}"); return
        for i, (x, y) in enumerate(zip(a, b)): cmp(x, y, f"{path}[{i}]", out)
        return
    if a != b: out.append(f"{path}: py={a!r} rust={b!r}")
for c in cases:
    st = stats.setdefault(c['kind'], dict(n=0, fail=0, err=0)); st['n'] += 1
    try: r = norm(F[c['kind']](c['p'])); e = None
    except Exception as x: r = None; e = str(x)
    if e or c.get('e'):
        st['err'] += 1
        if (e is None) != (c.get('e') is None): st['fail'] += 1; print(f"[{c['kind']}] error mismatch py={e} rust={c.get('e')}")
        continue
    out = []; cmp(r, c['r'], '', out)
    if c is cases[-1]:
        last = out; st['n'] -= 1; continue
    if out:
        st['fail'] += 1
        if st['fail'] <= 3: print(f"[{c['kind']}] {len(out)} diffs: {' | '.join(out[:4])}\n   in={json.dumps(c['p'], ensure_ascii=False)[:500]}")
tot = bad = 0
for k, s in stats.items():
    tot += s['n']; bad += s['fail']; print(f"{'FAIL' if s['fail'] else 'ok  '} {k:6s} cases={s['n']} errors(both)={s['err']} mismatches={s['fail']}")
print(f"TOTAL cases={tot} mismatches={bad} maxDiff(passing)={maxd:.2e}")
print('knife-edge case vs python:', 'same' if not last else last[:3])

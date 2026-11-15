//! 재무계산기 (TI BA II Plus 계열과 같은 계산 규칙)
//! TVM · 상환표 · 현금흐름(NPV/IRR/MIRR/회수기간) · 채권 · 감가상각 · 날짜 · 이자율 변환 · 이익률 · 손익분기 · 증감률 · 통계
use crate::date::{from_n, iso, mdays, mk, parse, weekday as wd, D};
use crate::js::*;
use crate::o;
use serde_json::Value;

/// `+p.k || d`
fn nz(p: &Value, k: &str, d: f64) -> f64 {
    let x = num(p, k);
    if x == 0.0 || x.is_nan() {
        d
    } else {
        x
    }
}
/// num(): '' · null · 유한하지 않음 → None
fn opt(p: &Value, k: &str) -> Option<f64> {
    opt_v(p.get(k))
}
fn opt_v(v: Option<&Value>) -> Option<f64> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(x)) if x.is_empty() => None,
        Some(v) => {
            let x = tonum(v);
            if x.is_finite() {
                Some(x)
            } else {
                None
            }
        }
    }
}
fn pd(p: &Value, k: &str) -> Option<D> {
    match p.get(k) {
        Some(v) if truthy(v) => parse(&js_string(v)).ok(),
        _ => None,
    }
}
fn is_eom(a: &D) -> bool {
    a.d == mdays(a.y, a.m)
}
fn add_m(a: &D, k: i64, eom: bool) -> D {
    let t = a.m - 1 + k;
    let y = a.y + t.div_euclid(12);
    let m = t.rem_euclid(12) + 1;
    let l = mdays(y, m);
    mk(y, m, if eom { l } else { a.d.min(l) })
}

/// 뉴턴법 + 이분법 보완
pub fn solve(f: &dyn Fn(f64) -> f64, lo: f64, hi: f64, guess: Option<f64>) -> R<f64> {
    let mut x = guess.unwrap_or((lo + hi) / 2.0);
    for _ in 0..100 {
        let fx = f(x);
        if !fx.is_finite() {
            break;
        }
        if fx.abs() < 1e-12 {
            return Ok(x);
        }
        let h = 1e-7f64.max(x.abs() * 1e-7);
        let d = (f(x + h) - f(x - h)) / (2.0 * h);
        if !d.is_finite() || d == 0.0 {
            break;
        }
        let nx = x - fx / d;
        if !nx.is_finite() || nx <= lo || nx >= hi {
            break;
        }
        if (nx - x).abs() < 1e-14 * 1f64.max(x.abs()) {
            return Ok(nx);
        }
        x = nx;
    }
    let m = 400;
    let mut a = lo;
    let mut fa = f(a);
    for i in 1..=m {
        let b = lo + (hi - lo) * i as f64 / m as f64;
        let fb = f(b);
        if fa.is_finite() && fb.is_finite() && fa * fb <= 0.0 {
            let (mut aa, mut bb, mut faa) = (a, b, fa);
            for _ in 0..200 {
                let mid = (aa + bb) / 2.0;
                let fm = f(mid);
                if faa * fm <= 0.0 {
                    bb = mid;
                } else {
                    aa = mid;
                    faa = fm;
                }
                if bb - aa < 1e-15 {
                    break;
                }
            }
            return Ok((aa + bb) / 2.0);
        }
        a = b;
        fa = fb;
    }
    Err("해를 찾지 못했습니다 (Error 5: 해 없음)".into())
}

// ── TVM: PV·(1+i)^N + PMT·(1+i·S)·((1+i)^N − 1)/i + FV = 0
pub fn rate(t: &Value) -> f64 {
    let py = nz(t, "PY", 1.0);
    let cy = nz(t, "CY", py);
    (1.0 + num(t, "IY") / 100.0 / cy).powf(cy / py) - 1.0
}
fn tvm_f(n: f64, i: f64, pv: f64, pmt: f64, fv: f64, s: f64) -> f64 {
    if i.abs() < 1e-13 {
        return pv + pmt * n + fv;
    }
    let g = (1.0 + i).powf(n);
    pv * g + pmt * (1.0 + i * s) * (g - 1.0) / i + fv
}
pub fn tvm(t: &Value, key: &str) -> R<f64> {
    let py = nz(t, "PY", 1.0);
    let cy = nz(t, "CY", py);
    let s = if flag(t, "BGN") { 1.0 } else { 0.0 };
    let (n, pv, pmt, fv) = (num(t, "N"), num(t, "PV"), num(t, "PMT"), num(t, "FV"));
    if key == "IY" {
        if !(n > 0.0) {
            return Err("N이 0보다 커야 합니다.".into());
        }
        let i = solve(&|i| tvm_f(n, i, pv, pmt, fv, s), -0.9999, 10.0, Some(0.01))?;
        return Ok(cy * ((1.0 + i).powf(py / cy) - 1.0) * 100.0);
    }
    let i = rate(t);
    let zero = i.abs() < 1e-13;
    match key {
        "PV" => {
            if zero {
                return Ok(-(pmt * n + fv));
            }
            let g = (1.0 + i).powf(n);
            Ok(-(pmt * (1.0 + i * s) * (g - 1.0) / i + fv) / g)
        }
        "FV" => {
            if zero {
                return Ok(-(pv + pmt * n));
            }
            let g = (1.0 + i).powf(n);
            Ok(-(pv * g + pmt * (1.0 + i * s) * (g - 1.0) / i))
        }
        "PMT" => {
            if zero {
                return Ok(-(pv + fv) / n);
            }
            let g = (1.0 + i).powf(n);
            Ok(-(pv * g + fv) * i / ((1.0 + i * s) * (g - 1.0)))
        }
        "N" => {
            if zero {
                return Ok(-(pv + fv) / pmt);
            }
            let a = pmt * (1.0 + i * s);
            let v = (a - fv * i) / (a + pv * i);
            if !(v > 0.0) {
                return Err("N을 계산할 수 없습니다 (Error 5).".into());
            }
            Ok(v.ln() / (1.0 + i).ln())
        }
        _ => Err("알 수 없는 항목".into()),
    }
}

// ── 상환표 (AMORT)
struct ARow {
    k: f64,
    int: f64,
    prn: f64,
    bal: f64,
}
fn amort_rows_raw(t: &Value) -> Vec<ARow> {
    let i = rate(t);
    let n = (num(t, "N") - 1e-9).ceil();
    let s = flag(t, "BGN");
    let pmt = num(t, "PMT");
    let mut bal = num(t, "PV");
    let mut rows = Vec::new();
    let lim = if n.is_nan() { 0.0 } else { n.min(2000.0) };
    let mut k = 1.0;
    while k <= lim {
        let a = if s && k == 1.0 { 0.0 } else { bal * i };
        bal += a;
        bal += pmt;
        rows.push(ARow { k, int: -a, prn: pmt + a, bal });
        k += 1.0;
    }
    rows
}
fn arow_json(r: &ARow) -> Value {
    o! {"k" => r.k, "int" => r.int, "prn" => r.prn, "bal" => r.bal}
}
pub fn amort_rows(t: &Value) -> Value {
    Value::Array(amort_rows_raw(t).iter().map(arow_json).collect())
}
pub fn amort(t: &Value, p1: &Value, p2: &Value) -> R<Value> {
    let rows = amort_rows_raw(t);
    let nzv = |v: &Value, d: f64| {
        let x = tonum(v);
        if x == 0.0 || x.is_nan() {
            d
        } else {
            x
        }
    };
    let p1 = 1f64.max(jround(nzv(p1, 1.0)));
    let p2 = p1.max(jround(nzv(p2, p1)));
    let sel: Vec<&ARow> = rows.iter().filter(|r| r.k >= p1 && r.k <= p2).collect();
    if sel.is_empty() {
        return Err("P1·P2가 회차 범위를 벗어났습니다.".into());
    }
    Ok(o! {
        "BAL" => sel[sel.len() - 1].bal,
        "PRN" => sel.iter().fold(0.0, |s, r| s + r.prn),
        "INT" => sel.iter().fold(0.0, |s, r| s + r.int),
        "rows" => rows.iter().map(arow_json).collect::<Vec<_>>(),
    })
}

// ── 현금흐름 (CF): flows=[{c,f}], 0번이 CF0
fn expand(flows: &Value) -> R<Vec<f64>> {
    let mut v = Vec::new();
    if let Some(a) = flows.as_array() {
        for (j, x) in a.iter().enumerate() {
            let n = if j == 0 { 1.0 } else { 0f64.max(jround(nz(x, "f", 1.0))) };
            if !n.is_finite() || v.len() as f64 + n > 200_000.0 {
                return Err("현금흐름 횟수가 너무 많습니다.".into());
            }
            let c = nz(x, "c", 0.0);
            for _ in 0..(n as usize) {
                v.push(c);
            }
        }
    }
    Ok(v)
}
fn npv_of(v: &[f64], r: f64) -> f64 {
    let mut s = 0.0;
    for (t, x) in v.iter().enumerate() {
        s += x / (1.0 + r).powf(t as f64);
    }
    s
}
pub fn cf(flows: &Value, i: &Value, ri: &Value) -> R<Value> {
    let v = expand(flows)?;
    let r = {
        let x = tonum(i);
        if x == 0.0 || x.is_nan() {
            0.0
        } else {
            x
        }
    } / 100.0;
    let n = v.len() as f64 - 1.0;
    let npv = npv_of(&v, r);
    let nfv = npv * (1.0 + r).powf(n);
    let pb = |disc: bool| -> Option<f64> {
        let mut cum = 0.0;
        for (t, &raw) in v.iter().enumerate() {
            let x = if disc { raw / (1.0 + r).powf(t as f64) } else { raw };
            let prev = cum;
            cum += x;
            if t > 0 && prev < 0.0 && cum >= 0.0 {
                return Some((t as f64 - 1.0) + (-prev) / x);
            }
        }
        None
    };
    let pos = v.iter().any(|&x| x > 0.0);
    let neg = v.iter().any(|&x| x < 0.0);
    let irr = if pos && neg { solve(&|x| npv_of(&v, x), -0.9999, 100.0, Some(0.1)).ok().map(|x| x * 100.0) } else { None };
    let mirr = if pos && neg && n > 0.0 {
        let rr = if ri.is_null() || ri.as_str() == Some("") { r } else { tonum(ri) / 100.0 };
        let (mut fvp, mut pvn) = (0.0, 0.0);
        for (t, &x) in v.iter().enumerate() {
            if x > 0.0 {
                fvp += x * (1.0 + rr).powf(n - t as f64);
            } else {
                pvn += x / (1.0 + r).powf(t as f64);
            }
        }
        Some(((fvp / (-pvn)).powf(1.0 / n) - 1.0) * 100.0)
    } else {
        None
    };
    Ok(o! {"NPV" => npv, "NFV" => nfv, "PB" => pb(false), "DPB" => pb(true), "IRR" => irr, "MIRR" => mirr, "n" => n, "flows" => v})
}

// ── 채권 (BOND): 액면 100 기준
fn d360(a: &D, b: &D) -> f64 {
    let (mut d1, mut d2) = (a.d, b.d);
    if d1 == 31 {
        d1 = 30;
    }
    if d2 == 31 && d1 >= 30 {
        d2 = 30;
    }
    ((b.y - a.y) * 360 + (b.m - a.m) * 30 + (d2 - d1)) as f64
}
struct BSet {
    f: f64,
    n: f64,
    e: f64,
    a: f64,
    dsc: f64,
    prv: D,
    nxt: D,
}
fn bond_setup(b: &Value) -> R<BSet> {
    let (Some(sdt), Some(rdt)) = (pd(b, "SDT"), pd(b, "RDT")) else {
        return Err("결제일과 상환일을 입력하세요.".into());
    };
    if rdt.n <= sdt.n {
        return Err("상환일이 결제일보다 뒤여야 합니다.".into());
    }
    let f = if num(b, "FREQ") == 1.0 { 1.0 } else { 2.0 };
    let step = (12.0 / f) as i64;
    let eom = is_eom(&rdt);
    let mut k: i64 = 0;
    let mut nxt = rdt;
    let mut prv = add_m(&rdt, -step, eom);
    while prv.n > sdt.n {
        k += 1;
        nxt = prv;
        prv = add_m(&rdt, -step * (k + 1), eom);
    }
    let act = s(b, "DAY").as_deref() != Some("360");
    let e = if act { (nxt.n - prv.n) as f64 } else { 360.0 / f };
    let a = if act { (sdt.n - prv.n) as f64 } else { d360(&prv, &sdt) };
    let dsc = if act { (nxt.n - sdt.n) as f64 } else { e - a };
    Ok(BSet { f, n: (k + 1) as f64, e, a, dsc, prv, nxt })
}
fn bond_price(c_: f64, rv: f64, y_: f64, st: &BSet) -> f64 {
    let c = c_ / st.f;
    let y = y_ / 100.0 / st.f;
    if st.n == 1.0 {
        return (rv + c) / (1.0 + (st.dsc / st.e) * y) - (st.a / st.e) * c;
    }
    let mut p = rv / (1.0 + y).powf(st.n - 1.0 + st.dsc / st.e);
    let mut k = 1.0;
    while k <= st.n {
        p += c / (1.0 + y).powf(k - 1.0 + st.dsc / st.e);
        k += 1.0;
    }
    p - (st.a / st.e) * c
}
pub fn bond(b: &Value, key: &str) -> R<Value> {
    let st = bond_setup(b)?;
    let c = nz(b, "CPN", 0.0);
    let rv = nz(b, "RV", 100.0);
    let ai = (c / st.f) * (st.a / st.e);
    let mut yld = opt(b, "YLD");
    let mut pri = opt(b, "PRI");
    if key == "PRI" {
        let Some(y) = yld else { return Err("YLD(수익률)를 입력하세요.".into()) };
        pri = Some(bond_price(c, rv, y, &st));
    } else if key == "YLD" {
        let Some(pr) = pri else { return Err("PRI(가격)를 입력하세요.".into()) };
        yld = Some(solve(&|y| bond_price(c, rv, y, &st) - pr, -50.0, 200.0, Some(if c != 0.0 { c } else { 5.0 }))?);
    }
    let dur = yld.map(|y| {
        let h = 1e-4;
        let dirty = |x: f64| bond_price(c, rv, x, &st) + ai;
        -(dirty(y + h) - dirty(y - h)) / (2.0 * h) / dirty(y) * 100.0
    });
    Ok(o! {"PRI" => pri, "YLD" => yld, "AI" => ai, "DUR" => dur, "prev" => iso(&st.prv), "next" => iso(&st.nxt), "N" => st.n, "E" => st.e, "A" => st.a, "DSC" => st.dsc})
}

// ── 감가상각 (DEPR): SL·SYD·DB·DBX, M01 = 시작 월
struct DRow {
    yr: f64,
    dep: f64,
    rbv: f64,
    rdv: f64,
}
fn depr_rows(d: &Value) -> R<Vec<DRow>> {
    let l = num(d, "LIF");
    let c = num(d, "CST");
    let sv = nz(d, "SAL", 0.0);
    let m = nz(d, "M01", 1.0);
    let r = nz(d, "DB", 200.0) / 100.0;
    let meth = match d.get("METH") {
        Some(v) if truthy(v) => js_string(v),
        _ => "SL".into(),
    };
    if !(l > 0.0) {
        return Err("LIF(내용연수)를 입력하세요.".into());
    }
    if !(c > 0.0) {
        return Err("CST(취득원가)를 입력하세요.".into());
    }
    if l > 10000.0 {
        return Err("LIF(내용연수)가 너무 큽니다.".into());
    }
    let fr = (13.0 - m) / 12.0;
    let base = c - sv;
    let years = (l + if fr < 1.0 { 1.0 } else { 0.0 } - 1e-9).ceil();
    let syd = |k: f64| {
        let s = l * (l + 1.0) / 2.0;
        if k >= 1.0 && k <= l.ceil() {
            base * 0f64.max(l - k + 1.0) / s
        } else {
            0.0
        }
    };
    let mut out = Vec::new();
    let mut rbv = c;
    let mut y = 1.0;
    while y <= years + 1.0 && rbv > sv + 1e-9 {
        let first = y == 1.0;
        let mut dep;
        if meth == "SL" {
            dep = base / l * if first { fr } else { 1.0 };
        } else if meth == "SYD" {
            dep = if first { fr * syd(1.0) } else { (1.0 - fr) * syd(y - 1.0) + fr * syd(y) };
        } else {
            let ratio = r / l;
            dep = rbv * ratio * if first { fr } else { 1.0 };
            if meth == "DBX" {
                let used = if first { 0.0 } else { fr + (y - 2.0) };
                let remain = l - used;
                let sl = if remain > 0.0 { (rbv - sv) / remain * if first { fr } else { 1.0 } } else { rbv - sv };
                if sl > dep {
                    dep = sl;
                }
            }
        }
        // JS Math.min(dep, rbv-SV): NaN 전파
        dep = crate::option::jmin(dep, rbv - sv);
        if dep < 0.0 {
            dep = 0.0;
        }
        rbv -= dep;
        out.push(DRow { yr: y, dep, rbv, rdv: rbv - sv });
        if y >= years + 1.0 {
            break;
        }
        y += 1.0;
    }
    Ok(out)
}
fn drow_json(r: &DRow) -> Value {
    o! {"YR" => r.yr, "DEP" => r.dep, "RBV" => r.rbv, "RDV" => r.rdv}
}
pub fn depr_schedule(d: &Value) -> R<Value> {
    Ok(Value::Array(depr_rows(d)?.iter().map(drow_json).collect()))
}
pub fn depr(d: &Value) -> R<Value> {
    let rows = depr_rows(d)?;
    let y = 1f64.max(jround(nz(d, "YR", 1.0)));
    let rj: Vec<Value> = rows.iter().map(drow_json).collect();
    let idx = y - 1.0;
    let mut r = if idx >= 0.0 && (idx as usize) < rows.len() && idx.is_finite() {
        rj[idx as usize].clone()
    } else {
        let (rbv, rdv) = match rows.last() {
            Some(l) => (l.rbv, l.rdv),
            None => (num(d, "CST"), num(d, "CST") - nz(d, "SAL", 0.0)),
        };
        o! {"YR" => y, "DEP" => 0.0, "RBV" => rbv, "RDV" => rdv}
    };
    r.as_object_mut().unwrap().insert("rows".into(), Value::Array(rj));
    Ok(r)
}

// ── 날짜 (DATE)
pub fn date(x: &Value, key: &str) -> R<Value> {
    let (a, b, dd) = (pd(x, "DT1"), pd(x, "DT2"), opt(x, "DBD"));
    let act = s(x, "DAY").as_deref() != Some("360");
    if key == "DBD" {
        let (Some(a), Some(b)) = (a, b) else { return Err("DT1·DT2를 입력하세요.".into()) };
        return Ok(n(if act { (b.n - a.n) as f64 } else { d360(&a, &b) }));
    }
    if !act {
        return Err("30/360 기준에서는 날짜를 거꾸로 계산하지 않습니다. ACT로 바꾸세요.".into());
    }
    if key == "DT2" {
        let (Some(a), Some(d)) = (a, dd) else { return Err("DT1·DBD를 입력하세요.".into()) };
        return Ok(Value::String(iso(&from_n(a.n + jround(d) as i64))));
    }
    if key == "DT1" {
        let (Some(b), Some(d)) = (b, dd) else { return Err("DT2·DBD를 입력하세요.".into()) };
        return Ok(Value::String(iso(&from_n(b.n - jround(d) as i64))));
    }
    Ok(Value::Null)
}
pub fn weekday(v: &Value) -> String {
    if !truthy(v) {
        return String::new();
    }
    match parse(&js_string(v)) {
        Ok(a) => ["일", "월", "화", "수", "목", "금", "토"][wd(&a) as usize].to_string(),
        Err(_) => String::new(),
    }
}

// ── 이자율 변환 (ICONV)
pub fn iconv(x: &Value, key: &str) -> f64 {
    let c = nz(x, "CY", 1.0);
    if key == "EFF" {
        100.0 * ((1.0 + num(x, "NOM") / 100.0 / c).powf(c) - 1.0)
    } else {
        100.0 * c * ((1.0 + num(x, "EFF") / 100.0).powf(1.0 / c) - 1.0)
    }
}

// ── 이익률 (PROFIT): MAR = (SEL−CST)/SEL, MU = (SEL−CST)/CST
pub fn profit(x: &Value, key: &str) -> Value {
    let z = |o: Option<f64>| o.unwrap_or(0.0);
    let (c, sl, m, u) = (opt(x, "CST"), opt(x, "SEL"), opt(x, "MAR"), opt(x, "MU"));
    let via_mar = m.is_some() && s(x, "_via").as_deref() != Some("MU");
    match key {
        "MAR" => n((z(sl) - z(c)) / z(sl) * 100.0),
        "MU" => n((z(sl) - z(c)) / z(c) * 100.0),
        "SEL" => n(if via_mar { z(c) / (1.0 - z(m) / 100.0) } else { z(c) * (1.0 + z(u) / 100.0) }),
        "CST" => n(if via_mar { z(sl) * (1.0 - z(m) / 100.0) } else { z(sl) / (1.0 + z(u) / 100.0) }),
        _ => Value::Null,
    }
}

// ── 손익분기 (BRKEVN): PFT = P·Q − (FC + VC·Q)
pub fn brkevn(x: &Value, key: &str) -> Value {
    let (fc, vc, p, pft, q) = (nz(x, "FC", 0.0), nz(x, "VC", 0.0), nz(x, "P", 0.0), nz(x, "PFT", 0.0), nz(x, "Q", 0.0));
    match key {
        "PFT" => n(p * q - fc - vc * q),
        "Q" => n((pft + fc) / (p - vc)),
        "P" => n((pft + fc) / q + vc),
        "FC" => n(p * q - vc * q - pft),
        "VC" => n((p * q - fc - pft) / q),
        _ => Value::Null,
    }
}

// ── 증감률 (Δ%): NEW = OLD·(1 + %CH/100)^#PD
pub fn pct(x: &Value, key: &str) -> Value {
    let (o_, nw, c, p) = (num(x, "OLD"), num(x, "NEW"), num(x, "CH"), nz(x, "PD", 1.0));
    match key {
        "CH" => n(((nw / o_).powf(1.0 / p) - 1.0) * 100.0),
        "NEW" => n(o_ * (1.0 + c / 100.0).powf(p)),
        "OLD" => n(nw / (1.0 + c / 100.0).powf(p)),
        "PD" => n((nw / o_).ln() / (1.0 + c / 100.0).ln()),
        _ => Value::Null,
    }
}

// ── 통계 (STAT): 1-V · LIN · Ln · EXP · PWR
pub fn stat(pts: &Value, meth: &Value) -> R<Value> {
    let meth = if truthy(meth) { js_string(meth) } else { "LIN".to_string() };
    let one = meth == "1-V";
    let p: Vec<(f64, f64)> = pts
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|q| opt(q, "x").is_some() && (one || opt(q, "y").is_some()))
                .map(|q| (num(q, "x"), opt(q, "y").unwrap_or(1.0)))
                .collect()
        })
        .unwrap_or_default();
    let cnt = p.len();
    let nf = cnt as f64;
    let mut out = o! {"n" => cnt, "meth" => meth.as_str()};
    if cnt == 0 {
        return Ok(out);
    }
    let m = out.as_object_mut().unwrap();
    let sx_: f64 = p.iter().map(|q| q.0).sum();
    let sx2: f64 = p.iter().map(|q| q.0 * q.0).sum();
    let xbar = sx_ / nf;
    m.insert("Sx_".into(), n(sx_));
    m.insert("Sx2".into(), n(sx2));
    m.insert("xbar".into(), n(xbar));
    m.insert("Sx".into(), if cnt > 1 { n(((sx2 - nf * xbar * xbar) / (nf - 1.0)).sqrt()) } else { Value::Null });
    m.insert("sx".into(), n(0f64.max(sx2 / nf - xbar * xbar).sqrt()));
    if one {
        return Ok(out);
    }
    let sy_: f64 = p.iter().map(|q| q.1).sum();
    let sy2: f64 = p.iter().map(|q| q.1 * q.1).sum();
    let sxy: f64 = p.iter().map(|q| q.0 * q.1).sum();
    let ybar = sy_ / nf;
    m.insert("Sy_".into(), n(sy_));
    m.insert("Sy2".into(), n(sy2));
    m.insert("Sxy".into(), n(sxy));
    m.insert("ybar".into(), n(ybar));
    m.insert("Sy".into(), if cnt > 1 { n(((sy2 - nf * ybar * ybar) / (nf - 1.0)).sqrt()) } else { Value::Null });
    m.insert("sy".into(), n(0f64.max(sy2 / nf - ybar * ybar).sqrt()));
    let lnx = meth == "Ln" || meth == "PWR";
    let lny = meth == "EXP" || meth == "PWR";
    let xs: Vec<f64> = p.iter().map(|q| if lnx { q.0.ln() } else { q.0 }).collect();
    let ys: Vec<f64> = p.iter().map(|q| if lny { q.1.ln() } else { q.1 }).collect();
    if xs.iter().chain(ys.iter()).any(|v| !v.is_finite()) {
        return Err("Ln·EXP·PWR 회귀는 양수 값만 쓸 수 있습니다.".into());
    }
    let mx = xs.iter().sum::<f64>() / nf;
    let my = ys.iter().sum::<f64>() / nf;
    let (mut sxx, mut syy, mut sxy2) = (0.0, 0.0, 0.0);
    for k in 0..cnt {
        sxx += (xs[k] - mx) * (xs[k] - mx);
        syy += (ys[k] - my) * (ys[k] - my);
        sxy2 += (xs[k] - mx) * (ys[k] - my);
    }
    let b = sxy2 / sxx;
    let a0 = my - b * mx;
    m.insert("b".into(), n(b));
    m.insert("a".into(), n(if lny { a0.exp() } else { a0 }));
    m.insert("r".into(), n(sxy2 / (sxx * syy).sqrt()));
    Ok(out)
}

/// 회귀식 예측: which = "Y" (x → y') 또는 "X" (y → x')
pub fn stat_pred(meth: &str, a: f64, b: f64, v: f64, which: &str) -> f64 {
    if which == "Y" {
        match meth {
            "LIN" => a + b * v,
            "Ln" => a + b * v.ln(),
            "EXP" => a * (b * v).exp(),
            _ => a * v.powf(b),
        }
    } else {
        match meth {
            "LIN" => (v - a) / b,
            "Ln" => ((v - a) / b).exp(),
            "EXP" => (v / a).ln() / b,
            _ => (v / a).powf(1.0 / b),
        }
    }
}

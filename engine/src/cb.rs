//! 내재파생상품 사채 (CB·EB·BW·콜/풋 옵션부 사채) 공정가치
//! 권면 1만원 기준 계산 후 권면총액만큼 곱함. BDT 금리트리 + CRR 주가트리(TF 분해).
use crate::date::{edate, from_n, iso, mdays, parse, D};
use crate::js::*;
use crate::o;
use crate::option::{jmax, jmin};
use crate::prs::{interp_nodes, Gc};
use serde_json::{Map, Value};

const TEN2: [f64; 10] = [0.25, 0.5, 0.75, 1., 2., 3., 5., 10., 20., 30.];
const TEN1: [f64; 8] = [0.25, 1., 2., 3., 5., 10., 20., 30.];
const GRID: usize = 120;
const F: f64 = 10000.0;
const RV_EXP: [f64; 9] = [1. / 12., 0.25, 0.5, 1., 2., 3., 5., 7., 10.];
const RV_TEN: [f64; 6] = [1., 2., 3., 5., 7., 10.];

fn ten_of(n: usize) -> &'static [f64] {
    if n == TEN1.len() {
        &TEN1
    } else {
        &TEN2
    }
}
fn mb(a: &D, b: &D) -> f64 {
    ((b.y - a.y) * 12 + (b.m - a.m)) as f64 + (b.d - a.d) as f64 / 30.0
}
fn rnd(x: f64) -> f64 {
    (x + 0.5).floor()
}
fn g(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}

fn arr_len(v: &Value) -> usize {
    v.as_array().map(|a| a.len()).unwrap_or(0)
}

pub fn fill(vals: &Value) -> R<Vec<f64>> {
    let ten = ten_of(arr_len(vals));
    let a = vals.as_array().cloned().unwrap_or_default();
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for (i, t) in ten.iter().enumerate() {
        if !blank(a.get(i)) {
            pts.push((*t, tonum(&a[i]) / 100.0));
        }
    }
    if pts.is_empty() {
        return Err("국고채 YTM을 하나 이상 입력하세요.".into());
    }
    let last = pts.len() - 1;
    Ok(ten
        .iter()
        .map(|&t| {
            if t <= pts[0].0 {
                return pts[0].1;
            }
            if t >= pts[last].0 {
                return pts[last].1;
            }
            for k in 0..last {
                let ((a, ya), (b, yb)) = (pts[k], pts[k + 1]);
                if a <= t && t <= b {
                    return ya + (yb - ya) * (t - a) / (b - a);
                }
            }
            f64::NAN
        })
        .collect())
}

pub fn bootstrap(ytm: &Value, freq: f64) -> R<Gc> {
    let ten = ten_of(arr_len(ytm));
    let nt = ten.len();
    let y = fill(ytm)?;
    let mut grid = Vec::new();
    let mut node: Vec<Option<f64>> = Vec::new();
    let mut sum = 0.0;
    for k in 1..=GRID {
        let t = k as f64 / 4.0;
        grid.push(t);
        let cf = (t * freq - jround(t * freq)).abs() < 1e-9;
        let yt = if t >= ten[nt - 1] {
            y[nt - 1]
        } else {
            let mut dd = 0;
            for (i, tn) in ten.iter().enumerate() {
                if *tn <= t + 1e-12 {
                    dd = i;
                }
            }
            y[dd] + (t - ten[dd]) * (y[dd + 1] - y[dd]) / (ten[dd + 1] - ten[dd])
        };
        let df = if t <= 1.0 {
            Some((1.0 + yt).powf(-t))
        } else if cf {
            let c = yt / freq;
            Some((1.0 - c * sum) / (1.0 + c))
        } else {
            None
        };
        if let (Some(d), true) = (df, cf) {
            sum += d;
        }
        node.push(df.map(|d| d.powf(-1.0 / t) - 1.0));
    }
    let spot = interp_nodes(&grid, &node);
    Ok(Gc { t: grid.to_v(), ytm: y.to_v(), spot })
}

pub fn at(s: &[f64], t: f64) -> f64 {
    if t <= 0.25 {
        return g(s, 0);
    }
    if t >= GRID as f64 / 4.0 {
        return g(s, GRID - 1);
    }
    if t.is_nan() {
        return f64::NAN;
    }
    let k = (t * 4.0).floor();
    let ki = k as usize;
    g(s, ki - 1) + (t * 4.0 - k) * (g(s, ki) - g(s, ki - 1))
}

pub fn accreted(c: f64, pm: f64, y: f64, months: f64) -> f64 {
    if pm <= 0.0 {
        return F * (1.0 + y).powf(months / 12.0);
    }
    let m = 12.0 / pm;
    let k = months / pm;
    let paid = (k + 1e-9).floor();
    let mut s = 0.0;
    let mut i = 1.0;
    while i <= paid {
        s += (1.0 + y / m).powf(k - i);
        i += 1.0;
    }
    F * (1.0 + y / m).powf(k) - (c * F / m) * s
}

fn solve(f: &dyn Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> Option<f64> {
    if f(lo) < 0.0 || f(hi) > 0.0 {
        return None;
    }
    for _ in 0..100 {
        let m = (lo + hi) / 2.0;
        if f(m) > 0.0 {
            lo = m;
        } else {
            hi = m;
        }
    }
    Some((lo + hi) / 2.0)
}

pub fn zspread(price: f64, flows: &[(f64, f64)], spot: &[f64], shift: f64) -> Option<f64> {
    solve(&|s| flows.iter().fold(0.0, |a, (t, cf)| a + cf * (1.0 + at(spot, *t) + shift + s).powf(-t)) - price, -0.5, 3.0)
}
fn irr(price: f64, flows: &[(f64, f64)]) -> Option<f64> {
    solve(&|y| flows.iter().fold(0.0, |a, (t, cf)| a + cf * (1.0 + y).powf(-t)) - price, -0.9, 5.0)
}

fn lin(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let l = xs.len() - 1;
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[l] {
        return g(ys, ys.len() - 1);
    }
    for k in 0..l {
        if xs[k] <= x && x <= xs[k + 1] {
            return g(ys, k) + (g(ys, k + 1) - g(ys, k)) * (x - xs[k]) / (xs[k + 1] - xs[k]);
        }
    }
    g(ys, ys.len() - 1)
}

fn rate_sigmas(p: &Value, n: usize, dt: f64, t_: f64) -> (Vec<f64>, &'static str) {
    let fixed = jmax(1e-6, num_nc(p, "rate_vol", 0.15));
    let m = if s(p, "rate_vol_mode").as_deref() == Some("matrix") { p.get("rate_vol_matrix").filter(|v| truthy(v)) } else { None };
    let Some(m) = m else { return (vec![fixed; n], "fixed") };
    let ex: Vec<f64> = match m.get("exp") {
        Some(v) if truthy(v) => arr_num(Some(v)),
        _ => RV_EXP.to_vec(),
    };
    let tn: Vec<f64> = match m.get("ten") {
        Some(v) if truthy(v) => arr_num(Some(v)),
        _ => RV_TEN.to_vec(),
    };
    let vv: Vec<Vec<f64>> = m
        .get("vol")
        .and_then(|v| v.as_array())
        .map(|rows| rows.iter().map(|r| arr_num(Some(r)).into_iter().map(|x| x / 100.0).collect()).collect())
        .unwrap_or_default();
    let mut sig = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 * dt;
        let ten = jmax(t_ - t, 1e-9);
        let col: Vec<f64> = vv.iter().map(|r| lin(&tn, r, ten)).collect();
        sig.push(jmax(1e-6, if col.is_empty() { f64::NAN } else { lin(&ex, &col, t) }));
    }
    (sig, "matrix")
}

fn date_or(p: &Value, k: &str, alt: &str) -> R<D> {
    // JS: parse(p.k || p.alt)
    match p.get(k) {
        Some(v) if truthy(v) => parse(&js_string(v)),
        _ => parse(&s(p, alt).unwrap_or_default()),
    }
}

pub fn value(p: &Value, detail: bool) -> R<Value> {
    value_gc(p, detail, None)
}

pub fn value_gc(p: &Value, detail: bool, gov_in: Option<&Gc>) -> R<Value> {
    let kind = match p.get("kind") {
        Some(v) if truthy(v) => js_string(v),
        _ => "CB".into(),
    };
    let val = parse(&s(p, "valuation").unwrap_or_default())?;
    let iss = parse(&s(p, "issue").unwrap_or_default())?;
    let mat = parse(&s(p, "maturity").unwrap_or_default())?;
    if mat.n <= val.n {
        return Err("만기일은 평가일보다 뒤여야 합니다.".into());
    }
    if val.n < iss.n {
        return Err("평가일은 발행일 이후여야 합니다.".into());
    }
    let units = num_nc(p, "face_total", 1e10) / F;
    let c = num_or(p, "coupon", 0.0);
    let pm = {
        let x = num_nc(p, "pay_months", 3.0);
        if x == 0.0 || x.is_nan() {
            0.0
        } else {
            x
        }
    };
    let ytp = num_or(p, "ytp", 0.0);
    let bp = num_or(p, "rate_shift_bp", 0.0) / 1e4;
    let cs = num_nc(p, "credit_spread", 0.03) + num_or(p, "spread_shift_bp", 0.0) / 1e4;
    let s0 = num_nc(p, "spot", 10000.0) * (1.0 + num_or(p, "spot_chg", 0.0));
    let sig = jmax(1e-6, num_nc(p, "vol", 0.3) + num_or(p, "vol_shift", 0.0));
    let q = num_or(p, "div_yield", 0.0);
    let own;
    let gov: &Gc = if let Some(gc) = gov_in {
        gc
    } else if flag(p, "_gov") {
        own = crate::prs::gc_from(&p["_gov"]);
        &own
    } else {
        own = bootstrap(p.get("gov_ytm").unwrap_or(&Value::Null), num_or(p, "coupon_freq", 2.0))?;
        &own
    };
    let z = |t: f64| at(&gov.spot, t) + bp;
    let dfrf = |t: f64| (1.0 + z(t)).powf(-t);
    let dfrk = |t: f64| (1.0 + z(t) + cs).powf(-t);
    let days = (mat.n - val.n) as f64;
    let t_ = days / 365.0;
    let sd = jmax(1.0, num_or(p, "step_days", 7.0).floor());
    let nf = jmax(2.0, jmin(num_or(p, "max_steps", 300.0).floor(), (days / sd).ceil()));
    if !nf.is_finite() || nf > 5000.0 {
        return Err("트리 스텝 수가 올바르지 않습니다 (최대 5000).".into());
    }
    let n = nf as usize;
    let dt = t_ / n as f64;
    let stp = |d: &D| -> usize {
        let x = rnd(((d.n - val.n) as f64 / 365.0) / dt);
        jmax(0.0, jmin(n as f64, x)) as usize
    };
    let tt: Vec<f64> = (0..=n).map(|i| i as f64 * dt).collect();
    let (sr, rv_mode) = rate_sigmas(p, n, dt, t_);
    let step_on = flag(p, "step_on");
    let (s_start, s_rate, s_inc, s_cap) = (num_nc(p, "step_start", 6.0), num_nc(p, "step_rate", 0.08), num_nc(p, "step_inc", 0.01), num_nc(p, "step_cap", 0.20));
    let rate_from = |d0: &D| -> f64 {
        if step_on {
            let ys = mb(&iss, d0) / 12.0;
            if ys >= s_start - 1e-9 {
                return jmin(s_cap, s_rate + (ys - s_start + 1e-9).floor() * s_inc);
            }
        }
        c
    };
    let mut coupon_at = vec![0.0; n + 1];
    let mut cpn_step = vec![false; n + 1];
    let mut flows: Vec<(f64, f64)> = Vec::new();
    let mut acc = 0.0;
    let total_m = mb(&iss, &mat);
    if pm > 0.0 {
        if pm.trunc() != pm {
            return Err("이자지급주기(개월)가 올바르지 않습니다.".into());
        }
        let mut k: i64 = 1;
        let mut prev = iss;
        loop {
            let d = edate(&iss, k * pm as i64);
            if d.n > mat.n {
                break;
            }
            let amt = rate_from(&prev) * F * pm / 12.0;
            if d.n > val.n {
                cpn_step[stp(&d)] = true;
                if amt > 0.0 {
                    coupon_at[stp(&d)] += amt;
                    flows.push(((d.n - val.n) as f64 / 365.0, amt));
                }
                if prev.n <= val.n {
                    acc = amt * (val.n - prev.n) as f64 / ((d.n - prev.n).max(1)) as f64;
                }
            }
            prev = d;
            k += 1;
        }
    }
    let redemption = if step_on {
        F
    } else if pm > 0.0 {
        jmax(F, accreted(c, pm, ytp, rnd(total_m)))
    } else {
        F * (1.0 + ytp).powf((mat.n - iss.n) as f64 / 365.0)
    };
    flows.push((t_, redemption));
    let mut put_at: Vec<Option<f64>> = vec![None; n + 1];
    let mut sched = Vec::new();
    let put_on = flag(p, "put_on");
    if put_on {
        let py = num_nc(p, "put_yield", ytp);
        let every = jmax(1.0, num_or(p, "put_every", 3.0).floor());
        let d0 = parse(&s(p, "put_start").unwrap_or_default())?;
        let mut j: i64 = 0;
        loop {
            let d = edate(&d0, j * every as i64);
            if d.n >= mat.n {
                break;
            }
            if d.n >= val.n {
                let amt = accreted(c, pm, py, rnd(mb(&iss, &d)));
                put_at[stp(&d)] = Some(amt);
                sched.push(o! {"type" => "풋(조기상환)", "date" => iso(&d), "price" => amt});
            }
            j += 1;
        }
    }
    let mut call_at: Vec<Option<f64>> = vec![None; n + 1];
    let mut alpha = 0.0;
    if flag(p, "call_on") {
        alpha = jmin(1.0, jmax(0.0, num_nc(p, "call_ratio", 0.3)));
        let cy = num_nc(p, "call_yield", ytp);
        let par = flag(p, "call_par");
        let cpn_only = flag(p, "call_coupon_only");
        let a = parse(&s(p, "call_start").unwrap_or_default())?;
        let b0 = parse(&s(p, "call_end").unwrap_or_default())?;
        let b = if b0.n < mat.n { b0 } else { mat };
        for i in 0..=n {
            let d = from_n(val.n + rnd(tt[i] * 365.0) as i64);
            if a.n <= d.n && d.n <= b.n && (!cpn_only || cpn_step[i]) {
                call_at[i] = Some(if par { F } else { accreted(c, pm, cy, mb(&iss, &d)) });
            }
        }
        sched.push(o! {
            "type" => format!("콜({} {}%)", if par { "중도상환" } else { "매도청구" }, to_fixed(alpha * 100.0, 0)),
            "date" => format!("{} ~ {}", iso(&a), iso(&b)),
            "price" => if par { F } else { accreted(c, pm, cy, mb(&iss, &a)) },
        });
    }
    let drf: Vec<f64> = (0..n).map(|i| dfrf(tt[i + 1]) / dfrf(tt[i])).collect();
    let drk: Vec<f64> = (0..n).map(|i| dfrk(tt[i + 1]) / dfrk(tt[i])).collect();
    let sb = flows.iter().fold(0.0, |s, (t, cf)| s + cf * dfrk(*t));
    // BDT 금리트리를 위험할인커브에 보정
    let sq = dt.sqrt();
    let mut uu = vec![0.0; n];
    let mut qa = vec![1.0];
    for i in 0..n {
        let pp = dfrk(tt[i + 1]);
        let mut u = -drk[i].ln() / dt;
        for _ in 0..60 {
            let (mut fs, mut fp) = (0.0, 0.0);
            for j in 0..=i {
                let e = (sr[i] * (2.0 * j as f64 - i as f64) * sq).exp();
                let df = (-u * e * dt).exp();
                fs += qa[j] * df;
                fp += qa[j] * df * (-e * dt);
            }
            let st = (fs - pp) / fp;
            u -= st;
            if st.abs() < 1e-14 {
                break;
            }
        }
        uu[i] = u;
        let mut qn = vec![0.0; i + 2];
        for j in 0..=i {
            let df = (-u * (sr[i] * (2.0 * j as f64 - i as f64) * sq).exp() * dt).exp();
            qn[j] += 0.5 * qa[j] * df;
            qn[j + 1] += 0.5 * qa[j] * df;
        }
        qa = qn;
    }
    let rate_tree = |use_put: bool, use_call: bool| -> f64 {
        let mut v = vec![redemption + coupon_at[n]; n + 1];
        for i in (0..n).rev() {
            for j in 0..=i {
                let r = uu[i] * (sr[i] * (2.0 * j as f64 - i as f64) * sq).exp();
                let mut x = 0.5 * (v[j] + v[j + 1]) * (-r * dt).exp();
                if use_call {
                    if let Some(ca) = call_at[i] {
                        if x > ca {
                            x = ca;
                        }
                    }
                }
                if use_put {
                    if let Some(pa) = put_at[i] {
                        if x < pa {
                            x = pa;
                        }
                    }
                }
                v[j] = x + if i > 0 { coupon_at[i] } else { 0.0 };
            }
        }
        v[0]
    };
    let host_none = rate_tree(false, false);
    let host_put = if put_on { rate_tree(true, false) } else { host_none };
    let host_pc = if alpha > 0.0 { alpha * rate_tree(true, true) + (1.0 - alpha) * host_put } else { host_put };
    // 주가트리
    let u = (sig * sq).exp();
    let d = 1.0 / u;
    let pr: Vec<f64> = (0..n).map(|i| ((-q * dt).exp() / drf[i] - d) / (u - d)).collect();
    let cp0 = num_nc(p, "conv_price", 10000.0);
    let refix = flag(p, "refix_on");
    let floor = num_nc(p, "refix_floor", 0.7) * cp0;
    let k_ref = if refix && flag(p, "refix_start") { stp(&parse(&s(p, "refix_start").unwrap())?) } else { 0 };
    let ce = date_or(p, "conv_end", "maturity")?;
    let k_cs = stp(&date_or(p, "conv_start", "issue")?);
    let k_ce = stp(&ce);
    let cs_ok = |i: usize| k_cs <= i && i <= k_ce && ce.n >= val.n;
    let ratio = num_nc(p, "bw_ratio", 1.0);
    let cp_at = |i: usize, sv: f64| if refix && i >= k_ref { jmax(floor, jmin(cp0, sv)) } else { cp0 };
    let sub = kind == "BW" && s(p, "bw_pay").as_deref() == Some("대용");
    let cr = if sub { ratio } else { 1.0 };
    let pw: Vec<f64> = (0..=2 * n).map(|k| u.powf(k as f64 - n as f64)).collect();
    let s_at = |i: usize, j: usize| s0 * pw[2 * j + n - i];
    let tf_tree = |use_call: bool| -> (f64, f64) {
        let mut e = vec![0.0; n + 1];
        let mut dd = vec![0.0; n + 1];
        for j in 0..=n {
            let sv = s_at(n, j);
            let bond = redemption + coupon_at[n];
            let cv = F * cr / cp_at(n, sv) * sv;
            if cs_ok(n) && cv > bond {
                e[j] = cv;
                dd[j] = 0.0;
            } else {
                e[j] = 0.0;
                dd[j] = bond;
            }
        }
        for i in (0..n).rev() {
            let (p_, a_, b_) = (pr[i], drf[i], drk[i]);
            let conv = cs_ok(i);
            let ca = if use_call { call_at[i] } else { None };
            let pa = put_at[i];
            for j in 0..=i {
                let mut ee = a_ * (p_ * e[j + 1] + (1.0 - p_) * e[j]);
                let mut d2 = b_ * (p_ * dd[j + 1] + (1.0 - p_) * dd[j]);
                let sv = s_at(i, j);
                let cv = if conv { F * cr / cp_at(i, sv) * sv } else { 0.0 };
                if let Some(ca) = ca {
                    if ee + d2 > ca {
                        if conv && cv >= ca {
                            ee = cv;
                            d2 = 0.0;
                        } else {
                            ee = 0.0;
                            d2 = ca;
                        }
                    }
                }
                if let Some(pa) = pa {
                    if pa > ee + d2 {
                        ee = 0.0;
                        d2 = pa;
                    }
                }
                if conv && cv > ee + d2 {
                    ee = cv;
                    d2 = 0.0;
                }
                e[j] = ee;
                dd[j] = d2 + if i > 0 { coupon_at[i] } else { 0.0 };
            }
        }
        (e[0] + dd[0], e[0])
    };
    let warrant_tree = || -> f64 {
        let mut v = vec![0.0; n + 1];
        for j in 0..=n {
            let sv = s_at(n, j);
            let x = cp_at(n, sv);
            v[j] = if cs_ok(n) { (F * ratio / x) * jmax(sv - x, 0.0) } else { 0.0 };
        }
        for i in (0..n).rev() {
            let (p_, a_) = (pr[i], drf[i]);
            let ok = cs_ok(i);
            for j in 0..=i {
                let mut x = a_ * (p_ * v[j + 1] + (1.0 - p_) * v[j]);
                if ok {
                    let sv = s_at(i, j);
                    let xx = cp_at(i, sv);
                    x = jmax(x, (F * ratio / xx) * jmax(sv - xx, 0.0));
                }
                v[j] = x;
            }
        }
        v[0]
    };
    let (total, option, eq_part);
    if kind == "CB" || kind == "EB" || sub {
        let (nc, enc) = tf_tree(false);
        if alpha > 0.0 {
            let (wc, ewc) = tf_tree(true);
            total = alpha * wc + (1.0 - alpha) * nc;
            eq_part = alpha * ewc + (1.0 - alpha) * enc;
        } else {
            total = nc;
            eq_part = enc;
        }
        option = total - host_pc;
    } else if kind == "BW" {
        option = warrant_tree();
        total = host_pc + option;
        eq_part = 0.0;
    } else {
        option = 0.0;
        total = host_pc;
        eq_part = 0.0;
    }
    let parity = if kind != "SB" { (F / cp0) * s0 } else { 0.0 };
    let sr_rest: &[f64] = if sr.len() > 1 { &sr[1..] } else { &sr };
    let smin = sr_rest.iter().fold(f64::INFINITY, |a, &b| jmin(a, b));
    let smax = sr_rest.iter().fold(f64::NEG_INFINITY, |a, &b| jmax(a, b));
    let mut out = o! {
        "kind" => kind.as_str(), "unit" => F, "units" => units, "total" => total, "total_amt" => total * units, "sb" => sb,
        "host_none" => host_none, "host_put" => host_put, "host_pc" => host_pc, "put_value" => host_put - host_none, "call_value" => host_pc - host_put,
        "option" => option, "eq_part" => eq_part, "accrued" => acc, "clean" => total - acc, "parity" => parity,
        "premium" => if parity > 0.0 { Some(total / parity - 1.0) } else { None }, "redemption" => redemption,
        "z_total" => zspread(total, &flows, &gov.spot, bp), "z_host" => zspread(host_pc, &flows, &gov.spot, bp), "z_sb" => zspread(sb, &flows, &gov.spot, bp),
        "ytm" => irr(total, &flows), "N" => n as i64, "dt" => dt, "T" => t_, "days" => days, "sigma" => sig, "S0" => s0, "cs" => cs, "p0" => pr[0],
        "calib_err" => (host_none - sb).abs(), "rate_vol_mode" => rv_mode, "rate_sig_min" => smin, "rate_sig_max" => smax,
    };
    if detail {
        let m: &mut Map<String, Value> = out.as_object_mut().unwrap();
        m.insert("flows".into(), Value::Array(flows.iter().map(|(t, cf)| o! {"t" => *t, "cf" => *cf}).collect()));
        m.insert("sched".into(), Value::Array(sched));
        m.insert("curve".into(), o! {"t" => gov.t.clone(), "spot" => &gov.spot, "ytm" => gov.ytm.clone()});
        m.insert("rate_sigs".into(), Value::Array(sr.iter().enumerate().map(|(i, sg)| o! {"t" => tt[i], "sigma" => *sg}).collect()));
    }
    Ok(out)
}

pub fn oas(p: &Value, price: f64, lo: f64, hi: f64) -> R<Option<f64>> {
    let mut q = p.clone();
    if let Some(m) = q.as_object_mut() {
        m.insert("spread_shift_bp".into(), Value::from(0));
        m.remove("_gov");
    }
    let gov = if flag(p, "_gov") { crate::prs::gc_from(&p["_gov"]) } else { bootstrap(p.get("gov_ytm").unwrap_or(&Value::Null), num_or(p, "coupon_freq", 2.0))? };
    let f = |s: f64| -> R<f64> {
        let mut qq = q.clone();
        qq.as_object_mut().unwrap().insert("credit_spread".into(), n(s));
        let r = value_gc(&qq, false, Some(&gov))?;
        Ok(tonum(&r["total"]) - price)
    };
    let (mut a, mut b) = (lo, hi);
    let (mut fa, mut fb) = (f(a)?, f(b)?);
    if fa < 0.0 || fb > 0.0 {
        return Ok(None);
    }
    for _ in 0..60 {
        let m = b - fb * (b - a) / (fb - fa);
        let fm = f(m)?;
        if fm.abs() < 1e-7 || (b - a).abs() < 1e-10 {
            return Ok(Some(m));
        }
        if fm * fb < 0.0 {
            a = b;
            fa = fb;
        } else {
            fa /= 2.0;
        }
        b = m;
        fb = fm;
    }
    Ok(Some(b))
}

pub fn bootstrap_v(ytm: &Value, freq: f64) -> R<Value> {
    let gc = bootstrap(ytm, freq)?;
    Ok(o! {"t" => gc.t, "spot" => gc.spot.to_v(), "ytm" => gc.ytm})
}

pub fn _unused() -> i64 {
    mdays(2000, 1)
}

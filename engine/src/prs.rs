//! PRS(주가수익스왑) 공정가치 — 엑셀 'PRS 공정가치 평가 계산기' (금액 단위: 백만원)
use crate::date::{edate, from_n, iso, parse, weekday, D};
use crate::js::*;
use crate::o;
use crate::option::jmax;
use serde_json::Value;

pub const TEN: [f64; 12] = [0.25, 0.5, 0.75, 1., 1.5, 2., 2.5, 3., 4., 5., 7., 10.];

fn fill(v: &Value) -> R<Vec<f64>> {
    let mut out: Vec<Option<f64>> = Vec::new();
    let mut last: Option<f64> = None;
    if let Value::Array(a) = v {
        for x in a {
            if blank(Some(x)) {
                out.push(last);
            } else {
                let y = tonum(x) / 100.0;
                last = Some(y);
                out.push(last);
            }
        }
    }
    if out.first().copied().flatten().is_none() {
        return Err("3M YTM은 비워둘 수 없습니다.".into());
    }
    // 빈칸 뒤 값이 없으면 JS에서 null → 계산 시 0
    Ok(out.into_iter().map(|x| x.unwrap_or(0.0)).collect())
}

pub struct Gc {
    pub t: Value,
    pub ytm: Value,
    pub spot: Vec<f64>,
}

fn g(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}

pub fn bootstrap(ytm: &Value, freq: f64) -> R<Gc> {
    let y = fill(ytm)?;
    let mut grid = Vec::new();
    let mut yg = Vec::new();
    let mut node: Vec<Option<f64>> = Vec::new();
    let mut sum = 0.0;
    for k in 1..=40 {
        let t = k as f64 / 4.0;
        grid.push(t);
        let cf = (t * freq - jround(t * freq)).abs() < 1e-9;
        let yt = if t >= TEN[11] {
            g(&y, 11)
        } else {
            let mut dd = 0;
            for (i, tn) in TEN.iter().enumerate() {
                if *tn <= t + 1e-12 {
                    dd = i;
                }
            }
            g(&y, dd) + (t - TEN[dd]) * (g(&y, dd + 1) - g(&y, dd)) / (TEN[dd + 1] - TEN[dd])
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
        yg.push(yt);
        node.push(df.map(|d| d.powf(-1.0 / t) - 1.0));
    }
    let spot = interp_nodes(&grid, &node);
    Ok(Gc { t: grid.to_v(), ytm: yg.to_v(), spot })
}

/// 비어 있는 그리드 점은 앞뒤 노드 사이 선형보간 (뒤 노드가 없으면 앞 노드 값)
pub fn interp_nodes(grid: &[f64], node: &[Option<f64>]) -> Vec<f64> {
    (0..node.len())
        .map(|i| {
            if let Some(v) = node[i] {
                return v;
            }
            let a = (0..i).rev().find(|&k| node[k].is_some());
            let b = (i + 1..node.len()).find(|&k| node[k].is_some());
            match (a, b) {
                (Some(a), Some(b)) => {
                    let (na, nb) = (node[a].unwrap(), node[b].unwrap());
                    na + (grid[i] - grid[a]) * (nb - na) / (grid[b] - grid[a])
                }
                (Some(a), None) => node[a].unwrap(),
                (None, Some(b)) => node[b].unwrap(),
                _ => f64::NAN,
            }
        })
        .collect()
}

pub fn gc_from(v: &Value) -> Gc {
    Gc {
        t: v.get("t").cloned().unwrap_or(Value::Null),
        ytm: v.get("ytm").cloned().unwrap_or(Value::Null),
        spot: arr_num(v.get("spot")),
    }
}

pub fn at(s: &[f64], t: f64) -> f64 {
    if t <= 0.25 {
        return g(s, 0);
    }
    if t >= 10.0 {
        return g(s, 39);
    }
    if t.is_nan() {
        return f64::NAN;
    }
    let k = (t * 4.0).floor();
    let ki = k as usize;
    g(s, ki - 1) + (t * 4.0 - k) * (g(s, ki) - g(s, ki - 1))
}

fn roll(a: &D) -> D {
    match weekday(a) {
        6 => from_n(a.n + 2),
        0 => from_n(a.n + 1),
        _ => *a,
    }
}

pub fn value(p: &Value, detail: bool) -> R<Value> {
    let val = parse(&s(p, "valuation").unwrap_or_default())?;
    let start = parse(&s(p, "start").unwrap_or_default())?;
    let term = num_or(p, "term_years", 4.0);
    let mat = if flag(p, "maturity") { parse(&s(p, "maturity").unwrap())? } else { edate(&start, (12.0 * term).trunc() as i64) };
    if mat.n <= start.n {
        return Err("계약만료일이 계약개시일보다 뒤여야 합니다.".into());
    }
    let basis = num_or(p, "day_basis", 365.0);
    let k = num(p, "notional");
    let prem = num(p, "premium");
    let pm = num_or(p, "pay_months", 3.0);
    if !(pm >= 1.0) || pm.trunc() != pm {
        return Err("Premium 지급주기(개월)가 올바르지 않습니다.".into());
    }
    let pm = pm as i64;
    let bp = num_or(p, "rate_shift_bp", 0.0) / 10000.0;
    let s0 = num(p, "usd_equity") * num(p, "stake") * num(p, "fx") / 1e6 * (1.0 + num_or(p, "equity_chg", 0.0));
    let sigma = num(p, "vol");
    let cost = num_nc(p, "cost_dispose", 0.01) + num_nc(p, "cost_tax", 0.0035);
    let freq = num_or(p, "coupon_freq", 2.0);
    let gov = if flag(p, "_gov") { gc_from(&p["_gov"]) } else { bootstrap(p.get("gov_ytm").unwrap_or(&Value::Null), freq)? };
    let aa = if flag(p, "_aa") { gc_from(&p["_aa"]) } else { bootstrap(p.get("aa_ytm").unwrap_or(&Value::Null), freq)? };
    let gf = |t: f64| at(&gov.spot, t) + bp;
    let af = |t: f64| at(&aa.spot, t) + bp;
    let model = match p.get("model") {
        Some(v) if truthy(v) => js_string(v),
        _ => "excel".into(),
    };
    let crr = model.to_lowercase() == "crr";
    let fxd = |t: f64| if crr { gf(t) } else { af(t) };
    let timing = match p.get("pay_timing") {
        Some(v) if truthy(v) => js_string(v),
        _ => "선급".into(),
    };
    let arrears = timing == "후급" || timing == "arrears";
    let biz_roll = !matches!(p.get("biz_roll"), Some(Value::Bool(false)))
        && !matches!(p.get("biz_roll"), Some(Value::Number(x)) if x.as_f64() == Some(0.0))
        && !matches!(p.get("biz_roll"), Some(Value::String(x)) if x == "0" || x == "false");
    let adj = |a: D| if biz_roll { roll(&a) } else { a };

    let mut rows = Vec::new();
    let mut a_sum = 0.0;
    let mut left = 0.0;
    let mut npay: i64 = 0;
    let mut n_live: i64 = 0;
    let mut b: i64 = 0;
    while edate(&start, b * pm).n < mat.n {
        npay += 1;
        let pay = edate(&start, b * pm);
        let n1 = edate(&start, (b + 1) * pm);
        let nxt = if n1.n < mat.n { n1 } else { mat };
        let days = (nxt.n - pay.n) as f64;
        let amt = k * prem * days / basis;
        let pd0 = if arrears { nxt } else { pay };
        let pd = adj(pd0);
        let live = pd.n >= val.n;
        let t = if live { (pd.n - val.n) as f64 / basis } else { 0.0 };
        let sp = if live { fxd(t) } else { 0.0 };
        let df = if live { (1.0 + sp).powf(-t) } else { 0.0 };
        let pv = if live { -amt * df } else { 0.0 };
        a_sum += pv;
        if live {
            left += amt;
        }
        if detail {
            if live {
                n_live += 1;
            }
            rows.push(o! {"n" => b, "pay" => iso(&pay), "next" => iso(&nxt), "paydate" => iso(&pd), "rolled" => pd.n != pd0.n, "days" => days, "amount" => amt, "live" => live, "t" => t, "spot" => sp, "df" => df, "pv" => pv});
        }
        b += 1;
    }
    let settle = adj(mat);
    let days_left = (settle.n - val.n) as f64;
    if days_left <= 0.0 {
        return Err("평가기준일이 만기 이후입니다.".into());
    }
    let gw = num_or(p, "gap_days", 5.0);
    let ms = num_or(p, "max_steps", 300.0);
    let gap = jmax(gw, (days_left / ms).ceil());
    let mut dt = gap / basis;
    let mut nn = (days_left / gap).floor();
    if crr && nn >= 1.0 {
        // 원본 Python 엔진의 round()와 같게: 정확히 .5면 짝수 쪽 (예: 90.5 → 90)
        nn = 1f64.max((days_left / gap).round_ties_even());
        dt = days_left / nn / basis;
    }
    if !(nn >= 1.0) {
        return Err("만기까지 남은 기간이 트리 간격보다 짧습니다.".into());
    }
    if nn > 20000.0 {
        return Err("트리 스텝이 너무 많습니다. 트리 간격을 늘리세요.".into());
    }
    let n = nn as usize;
    let u = (sigma * dt.sqrt()).exp();
    let d = 1.0 / u;
    let gs: Vec<f64> = (0..=n).map(|i| gf(i as f64 * dt)).collect();
    let as_: Vec<f64> = (0..=n).map(|i| af(i as f64 * dt)).collect();
    let mut fg = Vec::with_capacity(n);
    let mut fa = Vec::with_capacity(n);
    let mut pp = Vec::with_capacity(n);
    for i in 0..n {
        let fi = i as f64;
        fg.push(((1.0 + gs[i + 1]).powf((fi + 1.0) * dt) / (1.0 + gs[i]).powf(fi * dt)).powf(1.0 / dt) - 1.0);
        fa.push(((1.0 + as_[i + 1]).powf((fi + 1.0) * dt) / (1.0 + as_[i]).powf(fi * dt)).powf(1.0 / dt) - 1.0);
        pp.push(((1.0 + fg[i]).powf(dt) - d) / (u - d));
    }
    let tax_rate = jmax(0.0, num_or(p, "gain_tax", 0.0));
    let tax_bearer = if s(p, "tax_bearer").as_deref() == Some("지급자") { "지급자" } else { "수령자" };
    let recv = tax_bearer == "수령자";
    let keep = 1.0 - if recv { tax_rate } else { 0.0 };
    let mut v = vec![0.0; n + 1];
    let mut q = vec![0.0; n + 1];
    let mut tx = vec![0.0; n + 1];
    let mut xp = vec![0.0; n + 1];
    let mut xn = vec![0.0; n + 1];
    for j in 0..=n {
        let sv = s0 * u.powf(n as f64 - 2.0 * j as f64) * (1.0 - cost);
        v[j] = sv - if crr { 0.0 } else { k };
        q[j] = if sv - k > 0.0 { 1.0 } else { 0.0 };
        tx[j] = tax_rate * jmax(sv - k, 0.0);
        xp[j] = keep * jmax(sv - k, 0.0);
        xn[j] = crate::option::jmin(sv - k, 0.0);
    }
    for i in (0..n).rev() {
        let (pi, gg, aa_) = (pp[i], fg[i], fa[i]);
        for j in 0..=i {
            let qq = pi * q[j] + (1.0 - pi) * q[j + 1];
            let disc = (1.0 + if crr { gg } else { qq * gg + (1.0 - qq) * aa_ }).powf(dt);
            v[j] = (pi * v[j] + (1.0 - pi) * v[j + 1]) / disc;
            tx[j] = (pi * tx[j] + (1.0 - pi) * tx[j + 1]) / disc;
            xp[j] = (pi * xp[j] + (1.0 - pi) * xp[j + 1]) / disc;
            xn[j] = (pi * xn[j] + (1.0 - pi) * xn[j + 1]) / disc;
            q[j] = qq;
        }
    }
    let tax_pv = tx[0];
    let tax_ded = if recv { tax_pv } else { 0.0 };
    v[0] -= tax_ded;
    let kpv = k * (1.0 + gs[n]).powf(-(n as f64) * dt);
    let bb = if crr { v[0] - kpv } else { v[0] };
    let mut out = o! {
        "A" => a_sum, "B" => bb, "fv" => a_sum + bb, "k_pv" => kpv,
        "leg_float" => if crr { Some(v[0]) } else { None }, "leg_fixed" => if crr { Some(a_sum - kpv) } else { None },
        "S0" => s0, "K" => k, "q0" => q[0], "tax_rate" => tax_rate, "tax_bearer" => tax_bearer, "tax_pv" => tax_pv, "tax_ded" => tax_ded,
        "B_gross" => bb + tax_ded, "B_pos" => xp[0], "B_neg" => xn[0], "model" => if crr { "crr" } else { "excel" },
        "pay_timing" => if arrears { "후급" } else { "선급" }, "maturity" => iso(&mat), "settle" => iso(&settle), "biz_roll" => biz_roll,
        "term_days" => mat.n - start.n, "n_pay" => npay, "N" => n as i64, "gap" => gap, "gap_auto" => gap > gw,
        "dt" => dt, "u" => u, "d" => d, "p0" => pp[0], "fg0" => fg[0], "fa0" => fa[0], "days" => days_left, "cost" => cost, "sigma" => sigma,
        "premium_left" => left, "n_live" => n_live,
    };
    if detail {
        let m = out.as_object_mut().unwrap();
        m.insert("rows".into(), Value::Array(rows));
        m.insert("curve".into(), o! {"t" => gov.t.clone(), "gov" => &gov.spot, "aa" => &aa.spot, "gov_ytm" => gov.ytm.clone(), "aa_ytm" => aa.ytm.clone()});
    }
    Ok(out)
}

pub fn bootstrap_v(ytm: &Value, freq: f64) -> R<Value> {
    let gc = bootstrap(ytm, freq)?;
    Ok(o! {"t" => gc.t, "ytm" => gc.ytm, "spot" => gc.spot.to_v()})
}

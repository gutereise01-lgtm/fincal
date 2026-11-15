//! 옵션 이항모형 (CRR) — 엑셀 '옵션 이항모형 계산기'
use crate::date::parse;
use crate::js::*;
use crate::o;
use serde_json::Value;

const MAX_N: i64 = 1000;

fn comp(m: &str) -> f64 {
    match m {
        "단리(연1회복리)" => 1.0,
        "연 2회 복리" => 2.0,
        "연 4회 복리" => 4.0,
        "연 12회 복리(월복리)" => 12.0,
        _ => 2.0,
    }
}
fn ann(f: &str) -> f64 {
    match f {
        "일별" => 252.0,
        "주별" => 52.0,
        "월별" => 12.0,
        _ => 252.0,
    }
}

/// JS: Math.floor(Math.abs(x)+0.5)*(x>=0?1:-1)
fn xround(x: f64) -> f64 {
    (x.abs() + 0.5).floor() * if x >= 0.0 { 1.0 } else { -1.0 }
}

pub fn cont_rate(q: f64, m: &str) -> f64 {
    let k = comp(m);
    k * (1.0 + q / k).ln()
}

pub fn hist_vol(prices: &Value, freq: &str, n_obs: Option<f64>) -> Value {
    let mut px: Vec<f64> = match prices {
        Value::Array(a) => a
            .iter()
            .filter(|v| !v.is_null() && v.as_str() != Some("") && tonum(v).is_finite())
            .map(tonum)
            .collect(),
        _ => vec![],
    };
    if let Some(no) = n_obs {
        let k = 2f64.max(no.min(500.0));
        let k = if k.is_nan() { 2.0 } else { k };
        let end = (k.max(0.0) as usize).min(px.len());
        px.truncate(end);
    }
    let mut rets = Vec::new();
    for i in 1..px.len() {
        if px[i] > 0.0 && px[i - 1] > 0.0 {
            rets.push((px[i] / px[i - 1]).ln());
        }
    }
    if rets.len() < 2 {
        return o! {"sd" => Value::Null, "sigma" => 0.2, "count" => rets.len()};
    }
    let m = rets.iter().sum::<f64>() / rets.len() as f64;
    let sd = (rets.iter().map(|r| (r - m) * (r - m)).sum::<f64>() / (rets.len() as f64 - 1.0)).sqrt();
    o! {"sd" => sd, "sigma" => sd * ann(freq).sqrt(), "count" => rets.len()}
}

fn pay(call: bool, s: f64, k: f64) -> f64 {
    if call {
        (s - k).max(0.0)
    } else {
        (k - s).max(0.0)
    }
}

/// JS Math.max(a,b): NaN이 하나라도 있으면 NaN
pub fn jmax(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}
pub fn jmin(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

fn jpay(call: bool, s: f64, k: f64) -> f64 {
    if call {
        jmax(s - k, 0.0)
    } else {
        jmax(k - s, 0.0)
    }
}

pub fn value(p: &Value, preview: i64) -> R<Value> {
    let _ = pay;
    let days = (parse(&s(p, "maturity").unwrap_or_default())?.n - parse(&s(p, "valuation").unwrap_or_default())?.n) as f64;
    let t = days / 365.0;
    let s0 = num(p, "spot");
    let k0 = num(p, "strike");
    let irr_mode = s(p, "strike_mode").as_deref() == Some("이자율연동(IRR)");
    let irr = num_or(p, "strike_irr", 0.0);
    let kind = match p.get("kind") {
        Some(v) if truthy(v) => js_string(v),
        _ => "콜".into(),
    };
    let style = match p.get("style") {
        Some(v) if truthy(v) => js_string(v),
        _ => "유러피언".into(),
    };
    let call = kind == "콜";
    let r = match p.get("r") {
        Some(v) if !v.is_null() => tonum(v),
        _ => {
            let rc = match p.get("rate_comp") {
                Some(v) if truthy(v) => js_string(v),
                _ => "연 2회 복리".into(),
            };
            cont_rate(num(p, "rate_quoted"), &rc)
        }
    };
    let sigma = num(p, "sigma");
    let n_raw = if s(p, "step").as_deref() == Some("일간") { days } else { xround(days / 7.0) };
    let n_f = 1f64.max(n_raw.min(MAX_N as f64));
    if !n_f.is_finite() {
        return Err("만기일·평가일을 확인하세요.".into());
    }
    let n = n_f as usize;
    let dt = t / n as f64;
    let u = (sigma * dt.sqrt()).exp();
    let d = 1.0 / u;
    let pu = ((r * dt).exp() - d) / (u - d);
    let disc = (-r * dt).exp();
    let kt = if irr_mode { k0 * (1.0 + irr).powf(t) } else { k0 };
    let kat = |tt: usize| if irr_mode { k0 * (1.0 + irr).powf(tt as f64 * dt) } else { k0 };
    let lp = if pu > 0.0 { pu.ln() } else { f64::NEG_INFINITY };
    let lq = if pu < 1.0 { (1.0 - pu).ln() } else { f64::NEG_INFINITY };
    let mut lnf = vec![0.0f64; n + 1];
    for i in 1..=n {
        lnf[i] = lnf[i - 1] + (i as f64).ln();
    }
    let mut prob_sum = 0.0;
    let mut wsum = 0.0;
    for j in 0..=n {
        let jf = j as f64;
        let nj = (n - j) as f64;
        // JS: j*lp — 0 * -Infinity = NaN 그대로
        let lw = lnf[n] - lnf[j] - lnf[n - j] + jf * lp + nj * lq;
        let w = if lw > -745.0 { lw.exp() } else { 0.0 };
        prob_sum += w;
        wsum += w * jpay(call, s0 * u.powf(jf) * d.powf(nj), kt);
    }
    let euro = wsum * (-r * t).exp();
    let mut v = vec![0.0f64; n + 1];
    for j in 0..=n {
        v[j] = jpay(call, s0 * u.powf((n - j) as f64) * d.powf(j as f64), kat(n));
    }
    let pv = (n as i64).min(preview).max(-1);
    let mut keep: Vec<Option<Vec<f64>>> = vec![None; (pv + 1).max(0) as usize];
    if (n as i64) <= preview && pv >= 0 {
        keep[n] = Some(v[..=n].to_vec());
    }
    let mut early: i64 = 0;
    let american = style == "아메리칸";
    for tt in (0..n).rev() {
        let k_t = kat(tt);
        for j in 0..=tt {
            let mut c = disc * (pu * v[j] + (1.0 - pu) * v[j + 1]);
            if american {
                let ex = jpay(call, s0 * u.powf((tt - j) as f64) * d.powf(j as f64), k_t);
                if ex > c {
                    early += 1;
                    c = ex;
                }
            }
            v[j] = c;
        }
        if (tt as i64) <= preview && (tt as i64) <= pv {
            keep[tt] = Some(v[..=tt].to_vec());
        }
    }
    let tree = v[0];
    let price = if style == "유러피언" { euro } else { tree };
    let mut prev_s = Vec::new();
    let mut prev_v = Vec::new();
    for tt in 0..=(pv.max(0) as usize) {
        if pv < 0 {
            break;
        }
        let row: Vec<f64> = (0..=tt).map(|j| s0 * u.powf((tt - j) as f64) * d.powf(j as f64)).collect();
        prev_s.push(row.to_v());
        prev_v.push(keep[tt].clone().unwrap_or_default().to_v());
    }
    let qty = {
        let q = num_or(p, "quantity", 1.0);
        if q == 0.0 || q.is_nan() {
            1.0
        } else {
            q
        }
    };
    Ok(o! {
        "price" => price, "quantity" => qty, "total" => price * qty, "euro" => euro, "tree" => tree,
        "method" => if style == "유러피언" { "닫힌형(만기 분포 확률가중합)" } else { "이항트리 역산(매 스텝 조기행사 판단)" },
        "T" => t, "days" => days, "n_raw" => n_raw, "n" => n as i64, "capped" => n_raw > MAX_N as f64,
        "excel_tree_limit" => american && n > 500, "dt" => dt, "u" => u, "d" => d, "p" => pu, "r" => r, "sigma" => sigma,
        "K_T" => kt, "prob_sum" => prob_sum, "early_nodes" => early, "intrinsic" => jpay(call, s0, k0),
        "preview_S" => prev_s, "preview_V" => prev_v,
    })
}

//! 공통 금리커브: 고시 수익률(par) → 선형보간 → 부트스트랩 → 제로금리·할인계수·선도금리
use crate::js::*;
use crate::o;
use crate::prs::interp_nodes;
use serde_json::Value;

const STEP: f64 = 0.25;
pub const FWD_START: [f64; 10] = [0., 0.25, 0.5, 0.75, 1., 2., 3., 5., 7., 10.];
pub const FWD_TENOR: [f64; 7] = [0.25, 0.5, 0.75, 1., 2., 3., 5.];
pub const KIS_TENORS: [f64; 8] = [0.25, 0.5, 0.75, 1., 1.5, 2., 3., 5.];
const KTB_LONG: [f64; 3] = [10., 20., 30.];

/// 양 끝은 끝값, 사이는 선형 (닫힌 구간)
pub fn interp(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    if xs.is_empty() || ys.is_empty() {
        return f64::NAN;
    }
    let l = xs.len() - 1;
    let gy = |i: usize| ys.get(i).copied().unwrap_or(f64::NAN);
    if x <= xs[0] {
        return gy(0);
    }
    if x >= xs[l] {
        return gy(ys.len() - 1);
    }
    for i in 0..l {
        if xs[i] <= x && x <= xs[i + 1] {
            return gy(i) + (gy(i + 1) - gy(i)) * (x - xs[i]) / (xs[i + 1] - xs[i]);
        }
    }
    gy(ys.len() - 1)
}

fn points(tenors: &Value, rates: &Value) -> R<(Vec<f64>, Vec<f64>)> {
    let ts = tenors.as_array().cloned().unwrap_or_default();
    let rs = rates.as_array().cloned().unwrap_or_default();
    let mut pts: Vec<(f64, f64)> = Vec::new();
    for (i, t) in ts.iter().enumerate() {
        let r = rs.get(i);
        if blank(r) || !tonum(r.unwrap()).is_finite() {
            continue;
        }
        pts.push((tonum(t), tonum(r.unwrap()) / 100.0));
    }
    if pts.is_empty() {
        return Err("금리를 하나 이상 입력하세요.".into());
    }
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok((pts.iter().map(|p| p.0).collect(), pts.iter().map(|p| p.1).collect()))
}

pub struct Built {
    pub t: Vec<f64>,
    pub par: Vec<f64>,
    pub zero: Vec<f64>,
    pub df: Vec<f64>,
    pub fwd: Vec<f64>,
    pub freq: f64,
    pub src_t: Vec<f64>,
    pub src_r: Vec<f64>,
}

pub fn build(tenors: &Value, rates: &Value, freq: f64, max_t: Option<f64>) -> R<Built> {
    let (xs, ys) = points(tenors, rates)?;
    let max_t = match max_t {
        Some(m) if m != 0.0 && !m.is_nan() => m,
        _ => xs[xs.len() - 1],
    };
    let nf = 1f64.max(jround((max_t / STEP - 1e-9).ceil()));
    if !nf.is_finite() || nf > 4000.0 {
        return Err("커브 만기가 너무 깁니다.".into());
    }
    let n = nf as usize;
    let grid: Vec<f64> = (0..n).map(|k| (k as f64 + 1.0) * STEP).collect();
    let par: Vec<f64> = grid.iter().map(|&t| interp(t, &xs, &ys)).collect();
    let mut node: Vec<Option<f64>> = Vec::new();
    let mut s = 0.0;
    for (i, &t) in grid.iter().enumerate() {
        let y = par[i];
        let cf = (t * freq - jround(t * freq)).abs() < 1e-9;
        let df = if t <= 1.0 + 1e-12 {
            Some((1.0 + y).powf(-t))
        } else if cf {
            let c = y / freq;
            Some((1.0 - c * s) / (1.0 + c))
        } else {
            None
        };
        if let (Some(d), true) = (df, cf) {
            s += d;
        }
        node.push(df.map(|d| d.powf(-1.0 / t) - 1.0));
    }
    let zero = interp_nodes(&grid, &node);
    let df: Vec<f64> = grid.iter().zip(&zero).map(|(t, z)| (1.0 + z).powf(-t)).collect();
    let fwd: Vec<f64> = (0..n)
        .map(|i| if i == 0 { zero[0] } else { (df[i - 1] / df[i]).powf(1.0 / (grid[i] - grid[i - 1])) - 1.0 })
        .collect();
    Ok(Built { t: grid, par, zero, df, fwd, freq, src_t: xs, src_r: ys })
}

impl Built {
    pub fn json(&self) -> Value {
        o! {
            "t" => &self.t, "par" => &self.par, "zero" => &self.zero,
            "zero_cc" => self.zero.iter().map(|z| (1.0 + z).ln()).collect::<Vec<f64>>(),
            "df" => &self.df, "fwd" => &self.fwd, "freq" => self.freq, "src_t" => &self.src_t, "src_r" => &self.src_r,
        }
    }
}

pub fn df_at(t: &[f64], zero: &[f64], x: f64) -> f64 {
    if x <= 0.0 {
        1.0
    } else {
        (1.0 + interp(x, t, zero)).powf(-x)
    }
}

pub fn fwd_at(t: &[f64], zero: &[f64], t1: f64, t2: f64) -> Option<f64> {
    if t2 <= t1 {
        None
    } else {
        Some((df_at(t, zero, t1) / df_at(t, zero, t2)).powf(1.0 / (t2 - t1)) - 1.0)
    }
}

pub fn fwd_matrix(t: &[f64], zero: &[f64]) -> Value {
    let tm = t[t.len() - 1];
    Value::Array(
        FWD_START
            .iter()
            .map(|&s| FWD_TENOR.iter().map(|&m| if s + m <= tm + 1e-9 { fwd_at(t, zero, s, s + m) } else { None }).collect::<Vec<_>>().to_v())
            .collect(),
    )
}

pub fn value(p: &Value) -> R<Value> {
    let max_t = match p.get("max_t") {
        Some(v) if truthy(v) => Some(tonum(v)),
        _ => None,
    };
    let c = build(p.get("tenors").unwrap_or(&Value::Null), p.get("rates").unwrap_or(&Value::Null), num_or(p, "freq", 2.0), max_t)?;
    let mut j = c.json();
    let m = j.as_object_mut().unwrap();
    m.insert("fwd_matrix".into(), fwd_matrix(&c.t, &c.zero));
    m.insert("fwd_start".into(), FWD_START.to_vec().to_v());
    m.insert("fwd_tenor".into(), FWD_TENOR.to_vec().to_v());
    Ok(j)
}

/// 국고채: 3M~5Y는 한국신용평가(KIS) 민평, 10·20·30Y는 ECOS. KIS가 없으면 ECOS 전부.
pub fn ktb_points(ecos: &Value, kis_gov: &Value) -> Value {
    let mut pts: Vec<(f64, Value)> = Vec::new();
    let mut put = |t: f64, v: Value| {
        if let Some(e) = pts.iter_mut().find(|e| e.0 == t) {
            e.1 = v;
        } else {
            pts.push((t, v));
        }
    };
    let eget = |t: f64| -> Option<Value> {
        let o = ecos.as_object()?;
        o.iter().find(|(k, _)| str_to_num(k) == t).map(|(_, v)| v.clone()).filter(|v| !v.is_null())
    };
    if truthy(kis_gov) {
        for (i, &t) in KIS_TENORS.iter().enumerate() {
            if let Some(v) = kis_gov.get(i) {
                if !v.is_null() {
                    put(t, v.clone());
                }
            }
        }
        for &t in KTB_LONG.iter() {
            if let Some(v) = eget(t) {
                put(t, v);
            }
        }
    } else if let Some(o) = ecos.as_object() {
        for (k, v) in o {
            if !v.is_null() {
                put(str_to_num(k), v.clone());
            }
        }
    }
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    Value::Array(vec![pts.iter().map(|p| p.0).collect::<Vec<f64>>().to_v(), Value::Array(pts.into_iter().map(|p| p.1).collect())])
}

/// 회사채 등급: 5Y 이하는 표 그대로, 5Y 초과는 5Y 신용스프레드를 국고채에 더함
pub fn corp_points(gv: &Value, kis_gov: &Value, kt: &Value, kr: &Value) -> Value {
    let g = gv.as_array().cloned().unwrap_or_default();
    let kg = arr_num(Some(kis_gov));
    let sp5 = g.last().map(tonum).unwrap_or(f64::NAN) - kg.last().copied().unwrap_or(f64::NAN);
    let mut ts: Vec<f64> = KIS_TENORS.to_vec();
    let mut rs: Vec<Value> = g;
    let (ktn, krn) = (arr_num(Some(kt)), arr_num(Some(kr)));
    for (i, &t) in ktn.iter().enumerate() {
        if t > 5.0 {
            ts.push(t);
            rs.push(n(krn.get(i).copied().unwrap_or(f64::NAN) + sp5));
        }
    }
    Value::Array(vec![ts.to_v(), Value::Array(rs)])
}

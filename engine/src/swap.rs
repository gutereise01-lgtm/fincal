//! 금리스왑(IRS) · 통화스왑(CRS) · 통화선도(FX Forward)
//! 엑셀 모델: 부트스트래핑(Par → Zero) → 제로커브 선형보간 → EDATE 스케줄 → Day Count → 연복리 할인 → Leg PV / NPV
use crate::date::{edate, iso, parse, D};
use crate::js::*;
use crate::o;
use serde_json::{Map, Value};

pub const PAR_TENORS: [f64; 13] = [0.25, 0.5, 0.75, 1., 2., 3., 4., 5., 7., 10., 15., 20., 30.];
pub const PAR_TENORS_V1: [f64; 12] = [0.25, 0.5, 1., 2., 3., 4., 5., 7., 10., 15., 20., 30.];
const CURVE_TENORS: [f64; 12] = [0.25, 0.5, 0.75, 1., 2., 3., 5., 7., 10., 15., 20., 30.];

fn days360(a: &D, b: &D) -> f64 {
    let mut d1 = a.d;
    let (mut y2, mut m2, mut d2) = (b.y, b.m, b.d);
    if d1 == 31 || (a.m == 2 && d1 == crate::date::mdays(a.y, 2)) {
        d1 = 30;
    }
    if d2 == 31 {
        if d1 < 30 {
            d2 = 1;
            m2 += 1;
            if m2 > 12 {
                m2 = 1;
                y2 += 1;
            }
        } else {
            d2 = 30;
        }
    }
    ((y2 - a.y) * 360 + (m2 - a.m) * 30 + (d2 - d1)) as f64
}

fn yf(a: &D, b: &D, dc: &str) -> f64 {
    match dc {
        "30/360" => days360(a, b) / 360.0,
        "ACT/360" => (b.n - a.n) as f64 / 360.0,
        _ => (b.n - a.n) as f64 / 365.0,
    }
}

/// 첫 점 이하·끝 점 이상은 끝값, 그 사이 선형 (구간 [xs[i], xs[i+1]) )
pub fn interp(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    let last = xs.len() - 1;
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[last] {
        return ys[ys.len() - 1];
    }
    for i in 0..last {
        if xs[i] <= x && x < xs[i + 1] {
            return ys[i] + (ys[i + 1] - ys[i]) * (x - xs[i]) / (xs[i + 1] - xs[i]);
        }
    }
    ys[ys.len() - 1]
}

pub struct Boot {
    pub json: Value,
    pub curve: Vec<(f64, f64)>,
}

fn get_or_nan(v: &[f64], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(f64::NAN)
}

pub fn bootstrap(par: &[f64]) -> Boot {
    let pt: Vec<f64> = if par.len() == PAR_TENORS_V1.len() { PAR_TENORS_V1.to_vec() } else { PAR_TENORS.to_vec() };
    let ys: Vec<f64> = (0..pt.len()).map(|i| get_or_nan(par, i)).collect();
    let mut yearly = Vec::new();
    let mut zeros = Vec::new();
    let mut cum = 0.0;
    for n in 1..=30 {
        let p = interp(n as f64, &pt, &ys);
        let df = (1.0 - p * cum) / (1.0 + p);
        let z = df.powf(-1.0 / n as f64) - 1.0;
        yearly.push(o! {"year" => n as i64, "par" => p, "cum_df" => cum, "df" => df, "zero" => z});
        zeros.push(z);
        cum += df;
    }
    let mut zbt_map = Map::new();
    let mut zbt: Vec<(f64, f64)> = Vec::new();
    for (i, &t) in pt.iter().enumerate() {
        let v = if t < 1.0 { ys[i] } else { zeros[t as usize - 1] };
        zbt.push((t, v));
        zbt_map.insert(fmt_num(t), n(v));
    }
    let curve: Vec<(f64, f64)> = CURVE_TENORS
        .iter()
        .filter_map(|t| zbt.iter().find(|(x, _)| x == t).map(|(_, v)| (*t, *v)))
        .collect();
    let cj: Vec<Value> = curve.iter().map(|(t, z)| Value::Array(vec![n(*t), n(*z)])).collect();
    Boot { json: o! {"yearly" => yearly, "zero_by_tenor" => Value::Object(zbt_map), "curve" => cj}, curve }
}

pub struct Curve {
    xs: Vec<f64>,
    ys: Vec<f64>,
}
impl Curve {
    pub fn new(c: &[(f64, f64)]) -> Self {
        Curve { xs: c.iter().map(|p| p.0).collect(), ys: c.iter().map(|p| p.1).collect() }
    }
    pub fn zero(&self, t: f64) -> f64 {
        interp(t, &self.xs, &self.ys)
    }
    pub fn df(&self, t: f64) -> f64 {
        (1.0 + self.zero(t)).powf(-t)
    }
    pub fn pts(&self) -> Value {
        Value::Array(self.xs.iter().zip(&self.ys).map(|(t, z)| Value::Array(vec![n(*t), n(*z)])).collect())
    }
}

fn schedule(eff: &D, mat: &D, months: i64) -> Vec<(D, D)> {
    let mut r = Vec::new();
    let mut s = *eff;
    loop {
        let mut e = edate(&s, months);
        if e.n >= mat.n {
            e = *mat;
        }
        r.push((s, e));
        if e.n >= mat.n || r.len() > 100_000 {
            break;
        }
        s = e;
    }
    r
}

fn months_of(p: &Value, k: &str) -> R<i64> {
    let m = num(p, k);
    if !(m.is_finite()) || m.trunc() != m || m < 1.0 {
        return Err("지급주기(개월)가 올바르지 않습니다.".into());
    }
    Ok(m as i64)
}

fn date_of(p: &Value, k: &str) -> R<D> {
    parse(&s(p, k).ok_or_else(|| format!("날짜({})를 입력하세요.", k))?)
}

pub fn irs(p: &Value) -> R<Value> {
    let (eff, mat, val) = (date_of(p, "effective")?, date_of(p, "maturity")?, date_of(p, "valuation")?);
    let nn = num(p, "notional");
    let boot = bootstrap(&arr_num(p.get("par_rates")));
    let cv = Curve::new(&boot.curve);
    let fixed_rate = num(p, "fixed_rate");
    let spread = num(p, "float_spread");
    let fdc = s(p, "fixed_dc").unwrap_or_default();
    let ldc = s(p, "float_dc").unwrap_or_default();
    let mut fixed = Vec::new();
    let mut pv_fixed = 0.0;
    for (i, (st, e)) in schedule(&eff, &mat, months_of(p, "fixed_freq")?).iter().enumerate() {
        let dcf = yf(st, e, &fdc);
        let t = (e.n - val.n) as f64 / 365.0;
        let z = cv.zero(t);
        let df = (1.0 + z).powf(-t);
        let cf = if e.n <= val.n { 0.0 } else { nn * fixed_rate * dcf };
        pv_fixed += cf * df;
        fixed.push(o! {"no" => (i + 1) as i64, "start" => iso(st), "end" => iso(e), "days" => e.n - st.n, "dcf" => dcf, "t" => t, "zero" => z, "df" => df, "cf" => cf, "pv" => cf * df});
    }
    let mut floating = Vec::new();
    let mut pv_float = 0.0;
    for (i, (st, e)) in schedule(&eff, &mat, months_of(p, "float_freq")?).iter().enumerate() {
        let dcf = yf(st, e, &ldc);
        let ts = (st.n - val.n) as f64 / 365.0;
        let te = (e.n - val.n) as f64 / 365.0;
        let dfs = (1.0 + cv.zero(ts)).powf(-ts);
        let ze = cv.zero(te);
        let dfe = (1.0 + ze).powf(-te);
        let fwd = if e.n <= val.n || dcf == 0.0 || dcf.is_nan() { 0.0 } else { (dfs / dfe - 1.0) / dcf };
        let cf = if e.n <= val.n { 0.0 } else { nn * (fwd + spread) * dcf };
        pv_float += cf * dfe;
        floating.push(o! {"no" => (i + 1) as i64, "start" => iso(st), "end" => iso(e), "days" => e.n - st.n, "dcf" => dcf, "t" => te, "zero" => ze, "df" => dfe, "fwd" => fwd, "cf" => cf, "pv" => cf * dfe});
    }
    let npv = if s(p, "direction").as_deref() == Some("Receive Fixed") { pv_fixed - pv_float } else { pv_float - pv_fixed };
    let par_rate = if pv_fixed != 0.0 && !pv_fixed.is_nan() { fixed_rate * pv_float / pv_fixed } else { 0.0 };
    Ok(o! {"pv_fixed" => pv_fixed, "pv_float" => pv_float, "npv" => npv, "par_rate" => par_rate, "fixed" => fixed, "floating" => floating, "boot" => boot.json})
}

struct LegRow {
    end: String,
    dcf: f64,
    df: f64,
    cf: f64,
    pv: f64,
    json: Value,
}

#[allow(clippy::too_many_arguments)]
fn leg(eff: &D, mat: &D, val: &D, months: i64, dc: &str, nn: f64, kind: &str, fr: f64, sp: f64, cv: &Curve) -> Vec<LegRow> {
    schedule(eff, mat, months)
        .iter()
        .enumerate()
        .map(|(i, (st, e))| {
            let dcf = yf(st, e, dc);
            let ts = (st.n - val.n) as f64 / 365.0;
            let te = (e.n - val.n) as f64 / 365.0;
            let dfs = cv.df(ts);
            let dfe = cv.df(te);
            let alive = e.n > val.n;
            let fwd = if alive && dcf != 0.0 && !dcf.is_nan() { (dfs / dfe - 1.0) / dcf } else { 0.0 };
            let cf = if !alive {
                0.0
            } else if kind == "Fixed" {
                nn * fr * dcf
            } else {
                nn * (fwd + sp) * dcf
            };
            LegRow {
                end: iso(e),
                dcf,
                df: dfe,
                cf,
                pv: cf * dfe,
                json: o! {"no" => (i + 1) as i64, "start" => iso(st), "end" => iso(e), "days" => e.n - st.n, "dcf" => dcf, "t" => te, "zero" => cv.zero(te), "df" => dfe, "fwd" => fwd, "cf" => cf, "pv" => cf * dfe},
            }
        })
        .collect()
}

const KRW_PAY: &str = "KRW 지급 / USD 수취";

pub fn crs(p: &Value) -> R<Value> {
    let (eff, mat, val) = (date_of(p, "effective")?, date_of(p, "maturity")?, date_of(p, "valuation")?);
    let usd_n = num(p, "usd_notional");
    let fx_init = num(p, "fx_initial");
    let fx_val = num(p, "fx_valuation");
    let krw_n = usd_n * fx_init;
    let bk = bootstrap(&arr_num(p.get("krw_par_rates")));
    let bu = bootstrap(&arr_num(p.get("usd_par_rates")));
    let (ck, cu) = (Curve::new(&bk.curve), Curve::new(&bu.curve));
    let krw_type = s(p, "krw_type").unwrap_or_default();
    let usd_type = s(p, "usd_type").unwrap_or_default();
    let (kfr, ksp, ufr, usp) = (num(p, "krw_fixed_rate"), num(p, "krw_spread"), num(p, "usd_fixed_rate"), num(p, "usd_spread"));
    let krw = leg(&eff, &mat, &val, months_of(p, "krw_freq")?, &s(p, "krw_dc").unwrap_or_default(), krw_n, &krw_type, kfr, ksp, &ck);
    let usd = leg(&eff, &mat, &val, months_of(p, "usd_freq")?, &s(p, "usd_dc").unwrap_or_default(), usd_n, &usd_type, ufr, usp, &cu);
    let ex = s(p, "exchange").unwrap_or_default();
    let ix = if ex == "기초+만기 교환" { 1.0 } else { 0.0 };
    let mx = if ex == "교환 없음" { 0.0 } else { 1.0 };
    let t0 = (eff.n - val.n) as f64 / 365.0;
    let t1 = (mat.n - val.n) as f64 / 365.0;
    let (dk0, dk1, du0, du1) = (ck.df(t0), ck.df(t1), cu.df(t0), cu.df(t1));
    let kc: f64 = krw.iter().map(|r| r.pv).sum();
    let uc: f64 = usd.iter().map(|r| r.pv).sum();
    let kp = krw_n * (mx * dk1 - ix * dk0);
    let up = usd_n * (mx * du1 - ix * du0);
    let kt = kc + kp;
    let ut = uc + up;
    let utk = ut * fx_val;
    let dir = s(p, "direction").unwrap_or_default();
    let npv = if dir == KRW_PAY { utk - kt } else { kt - utk };
    let sk = if dir == KRW_PAY { -1.0 } else { 1.0 };
    let vs = iso(&val);
    let live = |r: &&LegRow| (r.cf != 0.0 && !r.cf.is_nan()) || r.end > vs;
    let ann_k: f64 = krw.iter().filter(live).map(|r| krw_n * r.dcf * r.df).sum();
    let ann_u: f64 = usd.iter().filter(live).map(|r| usd_n * r.dcf * r.df).sum::<f64>() * fx_val;
    let cur_k = if krw_type == "Fixed" { kfr } else { ksp };
    let cur_u = if usd_type == "Fixed" { ufr } else { usp };
    let tr = |x: f64| x != 0.0 && !x.is_nan();
    let fair = o! {
        "krw" => if tr(ann_k) { Some(cur_k - npv / (sk * ann_k)) } else { None },
        "usd" => if tr(ann_u) { Some(cur_u - npv / (-sk * ann_u)) } else { None },
        "krw_kind" => p.get("krw_type").cloned().unwrap_or(Value::Null),
        "usd_kind" => p.get("usd_type").cloned().unwrap_or(Value::Null),
    };
    Ok(o! {
        "fair" => fair, "krw_notional" => krw_n, "krw_coupon_pv" => kc, "usd_coupon_pv" => uc,
        "krw_principal_pv" => kp, "usd_principal_pv" => up, "krw_total" => kt, "usd_total" => ut, "usd_total_krw" => utk, "npv" => npv,
        "df" => o!{"krw_eff" => dk0, "krw_mat" => dk1, "usd_eff" => du0, "usd_mat" => du1},
        "krw" => krw.into_iter().map(|r| r.json).collect::<Vec<_>>(),
        "usd" => usd.into_iter().map(|r| r.json).collect::<Vec<_>>(),
        "boot_krw" => bk.json, "boot_usd" => bu.json,
    })
}

/// 통화선도: F = S × DF_USD/DF_KRW (또는 S + 시장 스왑포인트), 평가금액 = 부호 × N × (F − K) × DF_KRW
pub fn fx(p: &Value) -> R<Value> {
    let val = date_of(p, "valuation")?;
    let mat = date_of(p, "maturity")?;
    let days = mat.n - val.n;
    if days <= 0 {
        return Err("결제일이 평가일보다 뒤여야 합니다.".into());
    }
    let t = days as f64 / 365.0;
    let ck = Curve::new(&bootstrap(&arr_num(p.get("krw_par_rates"))).curve);
    let cu = Curve::new(&bootstrap(&arr_num(p.get("usd_par_rates"))).curve);
    let (sp_, k, nn) = (num(p, "spot"), num(p, "contract_rate"), num(p, "usd_notional"));
    let (dfk, dfu) = (ck.df(t), cu.df(t));
    let fcip = sp_ * dfu / dfk;
    let swp = p.get("swap_point");
    let mkt = matches!(swp, Some(v) if !v.is_null() && v.as_str() != Some(""));
    let f = if mkt { sp_ + tonum(swp.unwrap()) } else { fcip };
    let sg = if s(p, "direction").as_deref() == Some("USD 매입") { 1.0 } else { -1.0 };
    let value = sg * nn * (f - k) * dfk;
    let table: Vec<Value> = [1i64, 2, 3, 6, 9, 12, 24, 36]
        .iter()
        .map(|&m| {
            let d = edate(&val, m);
            let tt = (d.n - val.n) as f64 / 365.0;
            let fw = sp_ * cu.df(tt) / ck.df(tt);
            let label = if m < 12 { format!("{}M", m) } else { format!("{}Y", m / 12) };
            o! {"label" => label, "date" => iso(&d), "days" => d.n - val.n, "t" => tt, "krw_zero" => ck.zero(tt), "usd_zero" => cu.zero(tt), "fwd" => fw, "point" => fw - sp_}
        })
        .collect();
    Ok(o! {
        "value" => value, "npv" => value, "fwd" => f, "fwd_cip" => fcip, "use_mkt" => mkt, "point" => f - sp_, "point_cip" => fcip - sp_,
        "days" => days, "t" => t, "df_krw" => dfk, "df_usd" => dfu, "krw_zero" => ck.zero(t), "usd_zero" => cu.zero(t),
        "usd_leg" => sg * nn * f * dfk, "krw_leg" => sg * nn * k * dfk, "undisc" => sg * nn * (f - k), "delta" => sg * nn * (if mkt { dfk } else { dfu }),
        "sign" => sg, "maturity" => iso(&mat), "table" => table,
        // JS 쪽에서 _ck/_cu(zero·df 함수) 를 되살리는 데 쓰는 커브 점
        "_ck_pts" => ck.pts(), "_cu_pts" => cu.pts(),
    })
}

pub fn bootstrap_v(par: &Value) -> Value {
    bootstrap(&arr_num(Some(par))).json
}

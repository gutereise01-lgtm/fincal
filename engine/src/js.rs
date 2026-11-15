//! 기존 JS 엔진과 똑같이 동작하도록 JS의 값 변환 규칙(+x, ||, ??, Math.round 등)을 옮긴 도우미.
use serde_json::{Map, Value};

pub type R<T> = Result<T, String>;

/// JS 단항 `+v` (ToNumber)
pub fn tonum(v: &Value) -> f64 {
    match v {
        Value::Null => 0.0,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::String(s) => str_to_num(s),
        Value::Array(a) => match a.len() {
            0 => 0.0,
            1 => tonum(&a[0]),
            _ => f64::NAN,
        },
        Value::Object(o) => sentinel(o).unwrap_or(f64::NAN),
    }
}

fn sentinel(_o: &Map<String, Value>) -> Option<f64> {
    None
}

pub fn str_to_num(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        // 엔진 출력의 비유한값 표시(셈 결과를 다시 넣을 때)
        "__NaN__" => return f64::NAN,
        "__Inf__" => return f64::INFINITY,
        "__-Inf__" => return f64::NEG_INFINITY,
        _ => {}
    }
    let lower = t.to_ascii_lowercase();
    for (pre, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = lower.strip_prefix(pre) {
            return i64::from_str_radix(rest, radix).map(|v| v as f64).unwrap_or(f64::NAN);
        }
    }
    // JS 숫자 문법만 허용 (inf, nan 같은 Rust 표기는 NaN)
    if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// JS 참/거짓 판정
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            !(x == 0.0 || x.is_nan())
        }
        Value::String(s) => !s.is_empty() && s != "__NaN__",
        _ => true,
    }
}

pub fn get<'a>(p: &'a Value, k: &str) -> Option<&'a Value> {
    p.get(k)
}

/// `+p.k` (없으면 NaN, null이면 0)
pub fn num(p: &Value, k: &str) -> f64 {
    match p.get(k) {
        None => f64::NAN,
        Some(v) => tonum(v),
    }
}

/// `+(p.k || d)`
pub fn num_or(p: &Value, k: &str, d: f64) -> f64 {
    match p.get(k) {
        Some(v) if truthy(v) => tonum(v),
        _ => d,
    }
}

/// `+(p.k ?? d)`
pub fn num_nc(p: &Value, k: &str, d: f64) -> f64 {
    match p.get(k) {
        Some(v) if !v.is_null() => tonum(v),
        _ => d,
    }
}

/// `!!p.k`
pub fn flag(p: &Value, k: &str) -> bool {
    p.get(k).map(truthy).unwrap_or(false)
}

/// 문자열 값 (없거나 null이면 None). 숫자 등은 JS String()처럼 바꿈.
pub fn s(p: &Value, k: &str) -> Option<String> {
    match p.get(k) {
        None | Some(Value::Null) => None,
        Some(Value::String(x)) => Some(x.clone()),
        Some(v) => Some(js_string(v)),
    }
}

pub fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => fmt_num(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(x) => x.clone(),
        _ => String::new(),
    }
}

/// JS의 숫자→문자열 (정수는 소수점 없이)
pub fn fmt_num(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == x.trunc() && x.abs() < 1e21 {
        return format!("{}", x as i64);
    }
    format!("{}", x)
}

/// 값이 '빈칸'인지: null, undefined, '', 숫자가 아님
pub fn blank(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(x)) if x.is_empty() => true,
        Some(v) => tonum(v).is_nan(),
    }
}

/// JS Math.round (.5는 +∞ 쪽)
pub fn jround(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// JS Number.prototype.toFixed (자릿수 0 또는 양수, 동점은 큰 쪽)
pub fn to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() {
        return fmt_num(x);
    }
    let p = 10f64.powi(digits as i32);
    let v = x.abs() * p;
    let f = v.floor();
    let r = if v - f >= 0.5 { f + 1.0 } else { f };
    let neg = x < 0.0 && r != 0.0;
    let ip = (r / p).trunc();
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if digits == 0 {
        out.push_str(&format!("{}", r as i64));
    } else {
        let frac = (r - ip * p).round() as i64;
        out.push_str(&format!("{}.{:0width$}", ip as i64, frac, width = digits));
    }
    out
}

// ───────── 출력 값 만들기 ─────────
/// 숫자 → JSON. NaN·무한대는 표시 문자열로 보내고 JS 쪽에서 되살림.
pub fn n(x: f64) -> Value {
    if x.is_nan() {
        Value::String("__NaN__".into())
    } else if x.is_infinite() {
        Value::String(if x > 0.0 { "__Inf__".into() } else { "__-Inf__".into() })
    } else if x == 0.0 {
        Value::from(0.0f64) // -0도 0으로 (JSON에서 같음)
    } else {
        Value::from(x)
    }
}

pub trait ToV {
    fn to_v(self) -> Value;
}
impl ToV for f64 {
    fn to_v(self) -> Value {
        n(self)
    }
}
impl ToV for i64 {
    fn to_v(self) -> Value {
        Value::from(self)
    }
}
impl ToV for usize {
    fn to_v(self) -> Value {
        Value::from(self as i64)
    }
}
impl ToV for i32 {
    fn to_v(self) -> Value {
        Value::from(self as i64)
    }
}
impl ToV for bool {
    fn to_v(self) -> Value {
        Value::Bool(self)
    }
}
impl ToV for &str {
    fn to_v(self) -> Value {
        Value::String(self.to_string())
    }
}
impl ToV for String {
    fn to_v(self) -> Value {
        Value::String(self)
    }
}
impl ToV for Value {
    fn to_v(self) -> Value {
        self
    }
}
impl ToV for Option<f64> {
    fn to_v(self) -> Value {
        match self {
            Some(x) => n(x),
            None => Value::Null,
        }
    }
}
impl ToV for Vec<f64> {
    fn to_v(self) -> Value {
        Value::Array(self.into_iter().map(n).collect())
    }
}
impl ToV for &Vec<f64> {
    fn to_v(self) -> Value {
        Value::Array(self.iter().map(|x| n(*x)).collect())
    }
}
impl ToV for &[f64] {
    fn to_v(self) -> Value {
        Value::Array(self.iter().map(|x| n(*x)).collect())
    }
}
impl ToV for Vec<Value> {
    fn to_v(self) -> Value {
        Value::Array(self)
    }
}
impl ToV for Vec<Option<f64>> {
    fn to_v(self) -> Value {
        Value::Array(self.into_iter().map(|x| x.to_v()).collect())
    }
}

#[macro_export]
macro_rules! o {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m = serde_json::Map::new();
        $( m.insert($k.to_string(), $crate::js::ToV::to_v($v)); )*
        serde_json::Value::Object(m)
    }};
}

/// 숫자 배열 읽기 (빈칸은 None)
pub fn arr(v: Option<&Value>) -> Vec<Option<f64>> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| if blank(Some(x)) { None } else { Some(tonum(x)) })
            .collect(),
        _ => vec![],
    }
}

/// 숫자 배열 읽기 (JS의 +x 그대로)
pub fn arr_num(v: Option<&Value>) -> Vec<f64> {
    match v {
        Some(Value::Array(a)) => a.iter().map(tonum).collect(),
        _ => vec![],
    }
}

pub fn vnum(v: &Value) -> f64 {
    tonum(v)
}

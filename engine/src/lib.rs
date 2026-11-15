//! 재무계산기 계산 엔진 (Rust → WebAssembly)
//! 화면(JS)에서는 `call(이름, 인자 JSON 배열)` 하나로 모든 계산을 부른다.
//! 결과는 {"ok": 값} 또는 {"err": "메시지"} JSON 문자열.
pub mod cb;
pub mod curve;
pub mod date;
pub mod fin;
pub mod js;
pub mod option;
pub mod prs;
pub mod swap;

use js::*;
use serde_json::Value;
use wasm_bindgen::prelude::*;

const NULL: Value = Value::Null;

fn arg(a: &Value, i: usize) -> &Value {
    a.get(i).unwrap_or(&NULL)
}
/// 인자가 없거나 null이면 기본값
fn num_d(a: &Value, i: usize, d: f64) -> f64 {
    match a.get(i) {
        None | Some(Value::Null) => d,
        Some(v) => tonum(v),
    }
}
fn str_arg(a: &Value, i: usize) -> String {
    match a.get(i) {
        None | Some(Value::Null) => String::new(),
        Some(v) => js_string(v),
    }
}
fn bool_d(a: &Value, i: usize, d: bool) -> bool {
    match a.get(i) {
        None | Some(Value::Null) => d,
        Some(v) => truthy(v),
    }
}
fn flows_of(v: &Value) -> Vec<(f64, f64)> {
    v.as_array()
        .map(|a| a.iter().map(|x| (tonum(x.get(0).unwrap_or(&NULL)), tonum(x.get(1).unwrap_or(&NULL)))).collect())
        .unwrap_or_default()
}

pub fn dispatch(name: &str, a: &Value) -> R<Value> {
    Ok(match name {
        "version" => Value::String(env!("CARGO_PKG_VERSION").into()),
        // 금리스왑 · 통화스왑 · 통화선도
        "swap.irs" => swap::irs(arg(a, 0))?,
        "swap.crs" => swap::crs(arg(a, 0))?,
        "swap.fx" => swap::fx(arg(a, 0))?,
        "swap.bootstrap" => swap::bootstrap_v(arg(a, 0)),
        "swap.curve" => {
            // 제로커브 점 [[t,z]…]에서 t 목록의 zero·df
            let pts: Vec<(f64, f64)> = flows_of(arg(a, 0));
            let c = swap::Curve::new(&pts);
            let ts = arr_num(Some(arg(a, 1)));
            o! {"zero" => ts.iter().map(|&t| c.zero(t)).collect::<Vec<f64>>(), "df" => ts.iter().map(|&t| c.df(t)).collect::<Vec<f64>>()}
        }
        // 옵션
        "opt.value" => option::value(arg(a, 0), num_d(a, 1, 6.0) as i64)?,
        "opt.histVol" => option::hist_vol(arg(a, 0), &str_arg(a, 1), a.get(2).filter(|v| !v.is_null()).map(tonum)),
        "opt.contRate" => n(option::cont_rate(tonum(arg(a, 0)), &str_arg(a, 1))),
        // PRS
        "prs.value" => prs::value(arg(a, 0), bool_d(a, 1, true))?,
        "prs.bootstrap" => prs::bootstrap_v(arg(a, 0), num_d(a, 1, 2.0))?,
        // 내재파생 사채
        "cb.value" => cb::value(arg(a, 0), bool_d(a, 1, true))?,
        "cb.bootstrap" => cb::bootstrap_v(arg(a, 0), num_d(a, 1, 2.0))?,
        "cb.fill" => cb::fill(arg(a, 0))?.to_v(),
        "cb.accreted" => n(cb::accreted(tonum(arg(a, 0)), tonum(arg(a, 1)), tonum(arg(a, 2)), tonum(arg(a, 3)))),
        "cb.oas" => cb::oas(arg(a, 0), tonum(arg(a, 1)), num_d(a, 2, -0.05), num_d(a, 3, 0.6))?.to_v(),
        "cb.zspread" => cb::zspread(tonum(arg(a, 0)), &flows_of(arg(a, 1)), &arr_num(Some(arg(a, 2))), tonum(arg(a, 3))).to_v(),
        // 금리커브
        "curve.value" => curve::value(arg(a, 0))?,
        "curve.build" => {
            let mt = a.get(3).filter(|v| truthy(v)).map(tonum);
            curve::build(arg(a, 0), arg(a, 1), num_d(a, 2, 2.0), mt)?.json()
        }
        "curve.interp" => n(curve::interp(tonum(arg(a, 0)), &arr_num(Some(arg(a, 1))), &arr_num(Some(arg(a, 2))))),
        "curve.interpMany" => {
            let (xs, ys) = (arr_num(Some(arg(a, 1))), arr_num(Some(arg(a, 2))));
            arr_num(Some(arg(a, 0))).iter().map(|&x| curve::interp(x, &xs, &ys)).collect::<Vec<f64>>().to_v()
        }
        "curve.dfAt" => n(curve::df_at(&arr_num(Some(arg(a, 0))), &arr_num(Some(arg(a, 1))), tonum(arg(a, 2)))),
        "curve.fwdAt" => curve::fwd_at(&arr_num(Some(arg(a, 0))), &arr_num(Some(arg(a, 1))), tonum(arg(a, 2)), tonum(arg(a, 3))).to_v(),
        "curve.fwdMatrix" => curve::fwd_matrix(&arr_num(Some(arg(a, 0))), &arr_num(Some(arg(a, 1)))),
        "curve.ktbPoints" => curve::ktb_points(arg(a, 0), arg(a, 1)),
        "curve.corpPoints" => curve::corp_points(arg(a, 0), arg(a, 1), arg(a, 2), arg(a, 3)),
        // 재무계산기
        "fin.tvm" => n(fin::tvm(arg(a, 0), &str_arg(a, 1))?),
        "fin.rate" => n(fin::rate(arg(a, 0))),
        "fin.amort" => fin::amort(arg(a, 0), arg(a, 1), arg(a, 2))?,
        "fin.amortRows" => fin::amort_rows(arg(a, 0)),
        "fin.cf" => fin::cf(arg(a, 0), arg(a, 1), arg(a, 2))?,
        "fin.bond" => fin::bond(arg(a, 0), &str_arg(a, 1))?,
        "fin.deprSchedule" => fin::depr_schedule(arg(a, 0))?,
        "fin.depr" => fin::depr(arg(a, 0))?,
        "fin.date" => fin::date(arg(a, 0), &str_arg(a, 1))?,
        "fin.weekday" => Value::String(fin::weekday(arg(a, 0))),
        "fin.iconv" => n(fin::iconv(arg(a, 0), &str_arg(a, 1))),
        "fin.profit" => fin::profit(arg(a, 0), &str_arg(a, 1)),
        "fin.brkevn" => fin::brkevn(arg(a, 0), &str_arg(a, 1)),
        "fin.pct" => fin::pct(arg(a, 0), &str_arg(a, 1)),
        "fin.stat" => fin::stat(arg(a, 0), arg(a, 1))?,
        "fin.statPred" => n(fin::stat_pred(&str_arg(a, 0), tonum(arg(a, 1)), tonum(arg(a, 2)), tonum(arg(a, 3)), &str_arg(a, 4))),
        _ => return Err(format!("알 수 없는 계산: {}", name)),
    })
}

pub fn call_str(name: &str, args: &str) -> String {
    let res = match serde_json::from_str::<Value>(args) {
        Ok(a) => dispatch(name, &a),
        Err(e) => Err(format!("입력을 읽지 못했습니다: {}", e)),
    };
    match res {
        Ok(v) => format!("{{\"ok\":{}}}", v),
        Err(e) => format!("{{\"err\":{}}}", Value::String(e)),
    }
}

#[wasm_bindgen]
pub fn call(name: &str, args: &str) -> String {
    call_str(name, args)
}

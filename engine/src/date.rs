//! 날짜: JS 엔진의 {y,m,d,n} (n = 1970-01-01부터 일수, UTC) 그대로.
use crate::js::R;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct D {
    pub y: i64,
    pub m: i64,
    pub d: i64,
    pub n: i64,
}

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Date.UTC처럼 월 넘침을 정리
    let t = m - 1;
    let y = y + t.div_euclid(12);
    let m = t.rem_euclid(12) + 1;
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn mk(y: i64, m: i64, d: i64) -> D {
    let n = days_from_civil(y, m, d);
    let (yy, mm, dd) = civil_from_days(n);
    D { y: yy, m: mm, d: dd, n }
}

pub fn from_n(n: i64) -> D {
    let (y, m, d) = civil_from_days(n);
    D { y, m, d, n }
}

pub fn mdays(y: i64, m: i64) -> i64 {
    days_from_civil(y, m + 1, 1) - days_from_civil(y, m, 1)
}

/// "YYYY-MM-DD..." 앞 10자
pub fn parse(s: &str) -> R<D> {
    let head: String = s.chars().take(10).collect();
    let parts: Vec<&str> = head.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("날짜 형식이 올바르지 않습니다: {}", s));
    }
    let y: i64 = parts[0].trim().parse().map_err(|_| format!("날짜 형식이 올바르지 않습니다: {}", s))?;
    let m: i64 = parts[1].trim().parse().map_err(|_| format!("날짜 형식이 올바르지 않습니다: {}", s))?;
    let d: i64 = parts[2].trim().parse().map_err(|_| format!("날짜 형식이 올바르지 않습니다: {}", s))?;
    Ok(D { y, m, d, n: days_from_civil(y, m, d) })
}

pub fn iso(a: &D) -> String {
    format!("{}-{:02}-{:02}", a.y, a.m, a.d)
}

/// 엑셀 EDATE
pub fn edate(a: &D, k: i64) -> D {
    let t = a.m - 1 + k;
    let y = a.y + t.div_euclid(12);
    let m = t.rem_euclid(12) + 1;
    mk(y, m, a.d.min(mdays(y, m)))
}

/// 0=일 … 6=토
pub fn weekday(a: &D) -> i64 {
    (a.n + 4).rem_euclid(7)
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn roundtrip() {
        for n in -100000..100000 {
            let (y, m, d) = civil_from_days(n);
            assert_eq!(days_from_civil(y, m, d), n);
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(weekday(&mk(2026, 11, 15)), 0);
        assert_eq!(mdays(2024, 2), 29);
        assert_eq!(iso(&edate(&mk(2026, 1, 31), 1)), "2026-02-28");
    }
}

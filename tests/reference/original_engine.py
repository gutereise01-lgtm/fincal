"""IRS / CRS 밸류에이션 계산 엔진 (순수 파이썬, 표준 라이브러리만 사용)

업로드된 엑셀 모델(IRS 밸류에이션.xlsx, CRS 밸류에이션.xlsx)의 수식을 1:1로 옮긴 것:
  부트스트래핑(Par → Zero) → 제로커브 선형보간(FORECAST) → 현금흐름 스케줄(EDATE 체인)
  → Day Count(30/360, ACT/360, ACT/365) → 할인(연복리) → Leg PV / NPV
"""
from __future__ import annotations
import calendar
from datetime import date, datetime

# ─────────────────────────── 공통 유틸 ───────────────────────────
PAR_TENORS = [0.25, 0.5, 0.75, 1, 2, 3, 4, 5, 7, 10, 15, 20, 30]   # 1년 이하 마디: 3·6·9개월·1년 (모든 계산기 공통)
PAR_TENORS_V1 = [0.25, 0.5, 1, 2, 3, 4, 5, 7, 10, 15, 20, 30]   # 예전 12칸 입력(9개월 없음)도 그대로 계산
CURVE_TENORS = [0.25, 0.5, 0.75, 1, 2, 3, 5, 7, 10, 15, 20, 30]   # Inputs 시트 제로커브 테너(4Y 없음)


def to_date(x) -> date:
    if isinstance(x, datetime):
        return x.date()
    if isinstance(x, date):
        return x
    return datetime.strptime(str(x)[:10], "%Y-%m-%d").date()


def edate(d: date, months: int) -> date:
    """엑셀 EDATE: 월 단위 이동, 말일 초과 시 해당 월 말일로."""
    m = d.month - 1 + months
    y, m = d.year + m // 12, m % 12 + 1
    return date(y, m, min(d.day, calendar.monthrange(y, m)[1]))


def days360_us(d1: date, d2: date) -> int:
    """엑셀 DAYS360(start, end, FALSE) — 미국(NASD) 방식, 엑셀 실제 동작 기준.
    시작일: 31일 또는 2월 말일 → 30일 / 종료일: 31일이면 시작일<30 → 다음달 1일, 아니면 30일."""
    y1, m1, dd1 = d1.year, d1.month, d1.day
    y2, m2, dd2 = d2.year, d2.month, d2.day
    if dd1 == 31 or (m1 == 2 and dd1 == calendar.monthrange(y1, 2)[1]):
        dd1 = 30
    if dd2 == 31:
        if dd1 < 30:
            dd2, m2 = 1, m2 + 1
            if m2 > 12:
                m2, y2 = 1, y2 + 1
        else:
            dd2 = 30
    return (y2 - y1) * 360 + (m2 - m1) * 30 + (dd2 - dd1)


def year_frac(d1: date, d2: date, basis: str) -> float:
    if basis == "30/360":
        return days360_us(d1, d2) / 360
    if basis == "ACT/360":
        return (d2 - d1).days / 360
    return (d2 - d1).days / 365


def interp(x: float, xs: list[float], ys: list[float]) -> float:
    """엑셀 IF(<=첫점, 첫값, IF(>=끝점, 끝값, FORECAST(구간 선형))) 와 동일."""
    if x <= xs[0]:
        return ys[0]
    if x >= xs[-1]:
        return ys[-1]
    for i in range(len(xs) - 1):
        if xs[i] <= x < xs[i + 1]:
            x0, x1, y0, y1 = xs[i], xs[i + 1], ys[i], ys[i + 1]
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    return ys[-1]


# ─────────────────────────── 부트스트래핑 ───────────────────────────
def bootstrap(par_rates: list[float], par_tenors: list[float] = None) -> dict:
    """Par Swap Rate(연 1회 지급) → Zero Rate. 1~30년 연도별 선형보간 후 순차 부트스트랩."""
    if par_tenors is None:
        par_tenors = PAR_TENORS_V1 if len(par_rates) == len(PAR_TENORS_V1) else PAR_TENORS
    yearly = []
    cum_df = 0.0
    for n in range(1, 31):
        p = interp(n, par_tenors, par_rates)
        df = (1 - p * cum_df) / (1 + p)
        z = df ** (-1 / n) - 1
        yearly.append({"year": n, "par": p, "cum_df": cum_df, "df": df, "zero": z})
        cum_df += df
    zero_by_tenor = {}
    for t, p in zip(par_tenors, par_rates):
        zero_by_tenor[t] = p if t < 1 else yearly[int(t) - 1]["zero"]
    curve = [(t, zero_by_tenor[t]) for t in CURVE_TENORS if t in zero_by_tenor]
    return {"yearly": yearly, "zero_by_tenor": zero_by_tenor, "curve": curve}


class Curve:
    def __init__(self, points):
        self.t = [p[0] for p in points]
        self.z = [p[1] for p in points]

    def zero(self, t):
        return interp(t, self.t, self.z)

    def df(self, t):
        return (1 + self.zero(t)) ** (-t)


def schedule(eff: date, mat: date, months: int):
    """엑셀 Model 시트의 스케줄: 직전 종료일에서 EDATE 체인, 만기 초과 시 만기로 절단."""
    rows, start = [], eff
    while True:
        end = edate(start, months)
        if end >= mat:
            end = mat
        rows.append((start, end))
        if end >= mat:
            break
        start = end
    return rows


# ─────────────────────────── IRS ───────────────────────────
def value_irs(p: dict) -> dict:
    eff, mat, val = to_date(p["effective"]), to_date(p["maturity"]), to_date(p["valuation"])
    N = float(p["notional"])
    boot = bootstrap(p["par_rates"])
    cv = Curve(boot["curve"])

    fixed = []
    for i, (s, e) in enumerate(schedule(eff, mat, int(p["fixed_freq"])), 1):
        dcf = year_frac(s, e, p["fixed_dc"])
        T = (e - val).days / 365
        z = cv.zero(T)
        df = (1 + z) ** (-T)
        cf = 0.0 if e <= val else N * p["fixed_rate"] * dcf
        fixed.append(dict(no=i, start=s, end=e, days=(e - s).days, dcf=dcf, t=T, zero=z, df=df, cf=cf, pv=cf * df))

    flt = []
    for i, (s, e) in enumerate(schedule(eff, mat, int(p["float_freq"])), 1):
        dcf = year_frac(s, e, p["float_dc"])
        Ts, Te = (s - val).days / 365, (e - val).days / 365
        zs, ze = cv.zero(Ts), cv.zero(Te)
        dfs, dfe = (1 + zs) ** (-Ts), (1 + ze) ** (-Te)
        fwd = 0.0 if (e <= val or not dcf) else (dfs / dfe - 1) / dcf
        cf = 0.0 if e <= val else N * (fwd + p["float_spread"]) * dcf
        flt.append(dict(no=i, start=s, end=e, days=(e - s).days, dcf=dcf, t=Te, zero=ze, df=dfe, fwd=fwd, cf=cf, pv=cf * dfe))

    pv_fixed = sum(r["pv"] for r in fixed)
    pv_float = sum(r["pv"] for r in flt)
    npv = pv_fixed - pv_float if p["direction"] == "Receive Fixed" else pv_float - pv_fixed
    par = p["fixed_rate"] * (pv_float / pv_fixed) if pv_fixed else 0.0
    return dict(pv_fixed=pv_fixed, pv_float=pv_float, npv=npv, par_rate=par,
                fixed=fixed, floating=flt, boot=boot)


# ─────────────────────────── CRS ───────────────────────────
KRW_PAY = "KRW 지급 / USD 수취"


def _crs_leg(eff, mat, val, months, dc, notional, kind, fixed_rate, spread, cv):
    rows = []
    for i, (s, e) in enumerate(schedule(eff, mat, months), 1):
        dcf = year_frac(s, e, dc)
        Ts, Te = (s - val).days / 365, (e - val).days / 365
        dfs, dfe = cv.df(Ts), cv.df(Te)
        alive = e > val
        fwd = (dfs / dfe - 1) / dcf if (alive and dcf) else 0.0
        cf_fix = notional * fixed_rate * dcf if alive else 0.0
        cf_flt = notional * (fwd + spread) * dcf if alive else 0.0
        cf = cf_fix if kind == "Fixed" else cf_flt
        rows.append(dict(no=i, start=s, end=e, days=(e - s).days, dcf=dcf, t=Te,
                         zero=cv.zero(Te), df=dfe, fwd=fwd, cf=cf, pv=cf * dfe))
    return rows


def value_crs(p: dict) -> dict:
    eff, mat, val = to_date(p["effective"]), to_date(p["maturity"]), to_date(p["valuation"])
    usd_n = float(p["usd_notional"])
    krw_n = usd_n * p["fx_initial"]
    bk, bu = bootstrap(p["krw_par_rates"]), bootstrap(p["usd_par_rates"])
    ck, cu = Curve(bk["curve"]), Curve(bu["curve"])

    krw = _crs_leg(eff, mat, val, int(p["krw_freq"]), p["krw_dc"], krw_n, p["krw_type"],
                   p["krw_fixed_rate"], p["krw_spread"], ck)
    usd = _crs_leg(eff, mat, val, int(p["usd_freq"]), p["usd_dc"], usd_n, p["usd_type"],
                   p["usd_fixed_rate"], p["usd_spread"], cu)

    ex = p["exchange"]
    init_x = 1 if ex == "기초+만기 교환" else 0
    mat_x = 0 if ex == "교환 없음" else 1
    t_eff, t_mat = (eff - val).days / 365, (mat - val).days / 365
    dfk0, dfk1, dfu0, dfu1 = ck.df(t_eff), ck.df(t_mat), cu.df(t_eff), cu.df(t_mat)

    krw_cpn = sum(r["pv"] for r in krw)
    usd_cpn = sum(r["pv"] for r in usd)
    krw_prin = krw_n * (mat_x * dfk1 - init_x * dfk0)
    usd_prin = usd_n * (mat_x * dfu1 - init_x * dfu0)
    krw_total = krw_cpn + krw_prin
    usd_total = usd_cpn + usd_prin
    usd_total_krw = usd_total * p["fx_valuation"]
    npv = usd_total_krw - krw_total if p["direction"] == KRW_PAY else krw_total - usd_total_krw
    # 공정가치(NPV=0) 금리: NPV는 해당 Leg의 고정금리/스프레드에 대해 선형이므로 해석적으로 역산
    sign_k = -1 if p["direction"] == KRW_PAY else 1          # KRW Leg PV가 NPV에 들어가는 부호
    ann_k = sum(krw_n * r["dcf"] * r["df"] for r in krw if r["cf"] or r["end"] > val)
    ann_u = sum(usd_n * r["dcf"] * r["df"] for r in usd if r["cf"] or r["end"] > val) * p["fx_valuation"]
    cur_k = p["krw_fixed_rate"] if p["krw_type"] == "Fixed" else p["krw_spread"]
    cur_u = p["usd_fixed_rate"] if p["usd_type"] == "Fixed" else p["usd_spread"]
    fair = dict(krw=cur_k - npv / (sign_k * ann_k) if ann_k else None,
                usd=cur_u - npv / (-sign_k * ann_u) if ann_u else None,
                krw_kind=p["krw_type"], usd_kind=p["usd_type"])
    return dict(fair=fair, krw_notional=krw_n, krw_coupon_pv=krw_cpn, usd_coupon_pv=usd_cpn,
                krw_principal_pv=krw_prin, usd_principal_pv=usd_prin,
                krw_total=krw_total, usd_total=usd_total, usd_total_krw=usd_total_krw, npv=npv,
                df=dict(krw_eff=dfk0, krw_mat=dfk1, usd_eff=dfu0, usd_mat=dfu1),
                krw=krw, usd=usd, boot_krw=bk, boot_usd=bu)



# ─────────────────────────── 통화선도 (FX Forward) ───────────────────────────
FX_BUY, FX_SELL = "USD 매입", "USD 매도"
FX_TABLE_MONTHS = [1, 2, 3, 6, 9, 12, 24, 36]


def value_fx(p: dict) -> dict:
    """선도환율 F = S × DF_USD(T) / DF_KRW(T) (금리평형, 커브는 IRS·CRS와 같은 연복리 제로커브).
    시장 스왑포인트를 넣으면 F = S + 스왑포인트. 평가금액 = 부호 × USD 원금 × (F − 약정환율) × DF_KRW(T)."""
    val, mat = to_date(p["valuation"]), to_date(p["maturity"])
    days = (mat - val).days
    if days <= 0:
        raise ValueError("결제일이 평가일보다 뒤여야 합니다.")
    t = days / 365
    bk, bu = bootstrap(p["krw_par_rates"]), bootstrap(p["usd_par_rates"])
    ck, cu = Curve(bk["curve"]), Curve(bu["curve"])
    S, K, N = float(p["spot"]), float(p["contract_rate"]), float(p["usd_notional"])
    dfk, dfu = ck.df(t), cu.df(t)
    f_cip = S * dfu / dfk
    sp = p.get("swap_point")
    use_mkt = sp is not None and sp != ""
    F = S + float(sp) if use_mkt else f_cip
    sign = 1 if p["direction"] == FX_BUY else -1
    value = sign * N * (F - K) * dfk
    table = []
    for m in FX_TABLE_MONTHS:
        d = edate(val, m)
        tt = (d - val).days / 365
        f = S * cu.df(tt) / ck.df(tt)
        table.append(dict(label=f"{m}M" if m < 12 else f"{m // 12}Y", date=d.isoformat(), days=(d - val).days, t=tt,
                          krw_zero=ck.zero(tt), usd_zero=cu.zero(tt), fwd=f, point=f - S))
    return dict(value=value, npv=value, fwd=F, fwd_cip=f_cip, use_mkt=use_mkt, point=F - S, point_cip=f_cip - S,
                days=days, t=t, df_krw=dfk, df_usd=dfu, krw_zero=ck.zero(t), usd_zero=cu.zero(t),
                usd_leg=sign * N * F * dfk, krw_leg=sign * N * K * dfk, undisc=sign * N * (F - K),
                delta=sign * N * (dfk if use_mkt else dfu), sign=sign, maturity=mat.isoformat(), table=table)


# ─────────────────────────── 엑셀 기본값 ───────────────────────────
KRW_PAR = [0.03, 0.0305, 0.03075, 0.031, 0.032, 0.033, 0.034, 0.035, 0.0365, 0.038, 0.0395, 0.0405, 0.041]
USD_PAR = [0.043, 0.0415, 0.04025, 0.039, 0.0365, 0.0355, 0.035, 0.035, 0.0355, 0.0365, 0.0375, 0.038, 0.0375]

IRS_DEFAULT = dict(trade="2026-09-21", effective="2026-09-23", maturity="2031-09-23", valuation="2026-09-21",
                   notional=10_000_000_000, direction="Pay Fixed", fixed_rate=0.035, fixed_freq=3,
                   fixed_dc="ACT/360", float_freq=3, float_spread=0.0, float_dc="ACT/360",
                   par_rates=KRW_PAR)

CRS_DEFAULT = dict(trade="2026-09-21", effective="2026-09-23", maturity="2031-09-23", valuation="2026-09-21",
                   direction=KRW_PAY, fx_initial=1350, fx_valuation=1380, usd_notional=10_000_000,
                   exchange="기초+만기 교환",
                   krw_type="Fixed", krw_fixed_rate=0.032, krw_spread=0.0, krw_freq=3, krw_dc="ACT/365",
                   usd_type="Floating", usd_fixed_rate=0.038, usd_spread=-0.001, usd_freq=3, usd_dc="ACT/360",
                   krw_par_rates=KRW_PAR, usd_par_rates=USD_PAR)

FX_DEFAULT = dict(trade="2026-06-22", valuation="2026-09-21", maturity="2026-12-22", direction=FX_BUY,
                  usd_notional=10_000_000, contract_rate=1350.0, spot=1383.8, swap_point="")



"""
옵션 이항모형(Binomial, CRR) 계산 엔진 — 순수 파이썬, 표준 라이브러리만 사용

업로드된 엑셀 '옵션 이항모형 계산기'의 수식을 옮긴 것:
  · 무위험이자율: 고시금리(연 m회 복리) → 연속복리 r = m·ln(1+고시/m)
  · 변동성: 종가 로그수익률의 표본표준편차 × √(252|52|12)  (종가 2개 미만이면 20%)
  · T = (만기-평가일)/365, n = 일간: 경과일수 / 주간: ROUND(경과일수/7)  (1~1000)
  · u = e^(σ√dt), d = 1/u, p = (e^(r·dt) − d)/(u − d)
  · 행사가: 고정 K0 또는 이자율연동 K0·(1+IRR)^t
  · 유러피언: e^(−rT)·Σ C(n,j)p^j(1−p)^(n−j)·payoff(S0·u^j·d^(n−j), K_T)   (닫힌형)
  · 총액 = 1단위 이론가격 × 계약수량
  · 아메리칸: 이항트리 역산, 매 스텝 max(계속보유가치, 즉시행사가치)
    (엑셀은 트리를 500스텝까지만 그리지만, 이 엔진은 n=1000까지 계산)
"""

import math
from datetime import date, datetime

MAX_N = 1000
EXCEL_TREE_LIMIT = 500
COMPOUNDING = {"단리(연1회복리)": 1, "연 2회 복리": 2, "연 4회 복리": 4, "연 12회 복리(월복리)": 12}
ANNUALIZE = {"일별": 252, "주별": 52, "월별": 12}


def _d(x) -> date:
    if isinstance(x, datetime):
        return x.date()
    if isinstance(x, date):
        return x
    return datetime.strptime(str(x)[:10], "%Y-%m-%d").date()


def excel_round(x: float) -> int:
    """엑셀 ROUND(x,0): .5는 0에서 먼 쪽으로."""
    return int(math.floor(abs(x) + 0.5)) * (1 if x >= 0 else -1)


def cont_rate(quoted: float, method: str) -> float:
    m = COMPOUNDING.get(method, 2)
    return m * math.log(1 + quoted / m)


def hist_vol(prices, freq: str = "일별", n_obs: int | None = None):
    """종가 목록 → (관측주기 표준편차, 연율화 변동성, 사용한 수익률 개수). 수익률 2개 미만이면 표준편차 None."""
    px = [float(p) for p in prices if p not in (None, "")]
    if n_obs is not None:
        px = px[: max(2, min(int(n_obs), 500))]
    rets = [math.log(px[i] / px[i - 1]) for i in range(1, len(px)) if px[i] > 0 and px[i - 1] > 0]
    if len(rets) < 2:
        return None, 0.2, len(rets)
    mean = sum(rets) / len(rets)
    sd = math.sqrt(sum((r - mean) ** 2 for r in rets) / (len(rets) - 1))
    return sd, sd * math.sqrt(ANNUALIZE.get(freq, 252)), len(rets)


def _payoff(kind, s, k):
    return max(s - k, 0.0) if kind == "콜" else max(k - s, 0.0)


def value_option(p: dict, preview: int = 6) -> dict:
    val, mat = _d(p["valuation"]), _d(p["maturity"])
    days = (mat - val).days
    T = days / 365
    S0, K0 = float(p["spot"]), float(p["strike"])
    irr_mode = p.get("strike_mode") == "이자율연동(IRR)"
    irr = float(p.get("strike_irr", 0.0))
    kind, style = p.get("kind", "콜"), p.get("style", "유러피언")
    r = float(p["r"]) if p.get("r") is not None else cont_rate(float(p["rate_quoted"]), p.get("rate_comp", "연 2회 복리"))
    sigma = float(p["sigma"])
    n_raw = days if p.get("step") == "일간" else excel_round(days / 7)
    n = max(1, min(n_raw, MAX_N))
    dt = T / n
    u = math.exp(sigma * math.sqrt(dt)); d = 1 / u
    pu = (math.exp(r * dt) - d) / (u - d)
    disc = math.exp(-r * dt)
    K_T = K0 * (1 + irr) ** T if irr_mode else K0
    strike_at = (lambda t: K0 * (1 + irr) ** (t * dt)) if irr_mode else (lambda t: K0)

    # 유러피언 닫힌형 (로그 스케일로 안정적으로 계산)
    lp, lq = (math.log(pu) if pu > 0 else -math.inf), (math.log(1 - pu) if pu < 1 else -math.inf)
    lnf = [0.0] * (n + 1)
    for i in range(1, n + 1):
        lnf[i] = lnf[i - 1] + math.log(i)
    prob_sum = 0.0; wsum = 0.0
    for j in range(n + 1):
        lw = lnf[n] - lnf[j] - lnf[n - j] + j * lp + (n - j) * lq
        w = math.exp(lw) if lw > -745 else 0.0
        prob_sum += w
        wsum += w * _payoff(kind, S0 * u ** j * d ** (n - j), K_T)
    euro = wsum * math.exp(-r * T)

    # 이항트리 역산 (j = 하락 횟수, 엑셀 시트와 같은 배치)
    V = [_payoff(kind, S0 * u ** (n - j) * d ** j, strike_at(n)) for j in range(n + 1)]
    keep = {n: V[:preview + 1]} if n <= preview else {}
    early = 0
    for t in range(n - 1, -1, -1):
        Kt = strike_at(t)
        for j in range(t + 1):
            cont = disc * (pu * V[j] + (1 - pu) * V[j + 1])
            if style == "아메리칸":
                ex = _payoff(kind, S0 * u ** (t - j) * d ** j, Kt)
                if ex > cont:
                    early += 1
                    cont = ex
            V[j] = cont
        if t <= preview:
            keep[t] = V[: t + 1]
    tree_val = V[0]
    price = euro if style == "유러피언" else tree_val
    qty = float(p.get("quantity", 1) or 1)
    method = ("닫힌형(만기 분포 확률가중합)" if style == "유러피언"
              else "이항트리 역산(매 스텝 조기행사 판단)")
    pv_tree = [[S0 * u ** (t - j) * d ** j for j in range(t + 1)] for t in range(min(n, preview) + 1)]
    return dict(price=price, quantity=qty, total=price * qty, euro=euro, tree=tree_val, method=method, T=T, days=days, n_raw=n_raw, n=n,
                capped=n_raw > MAX_N, excel_tree_limit=(style == "아메리칸" and n > EXCEL_TREE_LIMIT),
                dt=dt, u=u, d=d, p=pu, r=r, sigma=sigma, K_T=K_T, prob_sum=prob_sum, early_nodes=early,
                intrinsic=_payoff(kind, S0, K0), preview_S=pv_tree,
                preview_V=[keep.get(t, []) for t in range(min(n, preview) + 1)])


OPT_DEFAULT = dict(valuation="2026-09-30", maturity="2029-10-20", spot=17000, strike=20000,
                   quantity=1, strike_mode="이자율연동(IRR)", strike_irr=0.03, kind="풋", style="아메리칸", step="주간",
                   rate_quoted=0.03, rate_comp="연 2회 복리",
                   vol_mode="종가", vol_freq="일별", sigma_direct=0.2,
                   prices=[15000, 15100, 17200, 17966.6666666667, 19066.6666666667, 20166.6666666667, 21266.6666666667,
                           22366.6666666667, 23466.6666666667, 24566.6666666668, 25666.6666666668, 26766.6666666668,
                           27866.6666666668, 28966.6666666668, 30066.6666666668, 31166.6666666668, 32266.6666666669,
                           33366.6666666669, 34466.6666666669, 35566.6666666669, 36666.6666666669, 37766.6666666669,
                           38866.6666666669, 39966.6666666669, 41066.6666666669, 42166.6666666669, 43266.6666666669,
                           44366.6666666669, 45466.6666666669, 46566.6666666669], vol_n=30)



"""
PRS(주가수익스왑) 공정가치 계산 엔진 — 순수 파이썬, 표준 라이브러리만 사용

업로드된 엑셀 'PRS 공정가치 평가 계산기'의 수식을 옮긴 것 (금액 단위: 백만원):
  · 금리커브: 국고채·AA- YTM(빈 만기는 직전 값) → 분기 그리드(0.25~10년)
      1년 이하: DF=(1+YTM)^-t / 1년 초과 이표일: DF=(1−c·ΣDF이전이표일)/(1+c), c=YTM/이표횟수
      그 외 시점: spot 선형보간. 임의 시점 spot = 분기 그리드 선형보간
  · 고정수익(A): 계약금액×Premium×일수/365 를 분기 선취 지급, 미지급분을 AA- spot으로 할인 (음수=지급)
  · 변동수익(B): CRR 이항트리, 만기 노드 = S·(1−매각비용) − 계약금액
      p = ((1+국고채 fwd)^dt − d)/(u − d)
      q = 만기에 양(+)의 정산이 될 위험중립 확률 (트리 역산)
      할인율 = q·국고채 fwd + (1−q)·AA- fwd   (정산 방향에 따라 신용위험 반영)
  · 양도차익 세금(선택): 만기 S·(1−비용) − K 가 양(+)이면 그 금액 × 세율. 세금 부담 = 변동수익 수령자면 B에서 차감
      (같은 트리·같은 할인율로 현재가치), 변동수익 지급자면 상대방이 부담하므로 평가금액 변화 없음
  · 지급일 조정(biz_roll, 기본 켜짐): Premium 지급일·만기 정산일이 토·일이면 다음 평일(월요일)에 지급한 것으로 할인
      (이자 계산 일수는 원래 기간 그대로, 공휴일은 미반영). 만기 정산일이 밀리면 트리도 그 날짜까지 계산
  · 공정가치 = A + B
  · 시뮬레이션: 금리 평행이동(bp)은 국고채·AA- spot에 동시에 더함
  · model="crr": 순수 CRR — p와 할인율 모두 국고채 선도금리, 고정수익도 국고채 spot 할인 (신용위험 미반영)
"""
import math
from datetime import date, datetime, timedelta

PRS_TENORS = [0.25, 0.5, 0.75, 1, 1.5, 2, 2.5, 3, 4, 5, 7, 10]
PRS_LABELS = ["3M", "6M", "9M", "1Y", "1.5Y", "2Y", "2.5Y", "3Y", "4Y", "5Y", "7Y", "10Y"]


def _pd(x) -> date:
    if isinstance(x, datetime):
        return x.date()
    if isinstance(x, date):
        return x
    return datetime.strptime(str(x)[:10], "%Y-%m-%d").date()


def _pedate(d: date, months: int) -> date:
    """엑셀 EDATE: 말일 초과 시 그 달 말일."""
    m = d.month - 1 + months
    y, m = d.year + m // 12, m % 12 + 1
    nxt = date(y + (m == 12), m % 12 + 1, 1)
    return date(y, m, min(d.day, (nxt - timedelta(days=1)).day))


def _ytm_fill(vals):
    """% 단위 YTM 목록 → 소수, 빈칸은 직전 만기 값."""
    out, last = [], None
    for v in vals:
        if v is None or v == "":
            out.append(last)
        else:
            last = float(v) / 100
            out.append(last)
    if out and out[0] is None:
        raise ValueError("3M YTM은 비워둘 수 없습니다.")
    return out


def prs_bootstrap(ytm_pct, freq: int = 2):
    """분기 그리드 40점의 spot (금리커브 시트 H/L열)."""
    y = _ytm_fill(ytm_pct)
    T = PRS_TENORS
    grid = [k / 4 for k in range(1, 41)]
    ytm_g, node, cpn = [], [], []
    sum_cpn = 0.0
    for t in grid:
        c_flag = abs(t * freq - round(t * freq)) < 1e-9
        if t >= T[-1]:
            yt = y[-1]
        else:
            D = max(i for i in range(len(T)) if T[i] <= t + 1e-12)
            yt = y[D] + (t - T[D]) * (y[D + 1] - y[D]) / (T[D + 1] - T[D])
        if t <= 1:
            df = (1 + yt) ** -t
        elif c_flag:
            c = yt / freq
            df = (1 - c * sum_cpn) / (1 + c)
        else:
            df = None
        if df is not None and c_flag:
            sum_cpn += df
        ytm_g.append(yt)
        cpn.append(c_flag)
        node.append(None if df is None else df ** (-1 / t) - 1)
    spot = []
    for i, t in enumerate(grid):
        if node[i] is not None:
            spot.append(node[i]); continue
        a = max(k for k in range(i) if node[k] is not None)
        b = min(k for k in range(i + 1, 40) if node[k] is not None)
        spot.append(node[a] + (t - grid[a]) * (node[b] - node[a]) / (grid[b] - grid[a]))
    return dict(t=grid, ytm=ytm_g, spot=spot)


def prs_spot_at(spot, t: float) -> float:
    """엑셀 INDEX(Spot, INT(t*4)) 선형보간 (0.25년 이하·10년 이상은 끝값)."""
    if t <= 0.25:
        return spot[0]
    if t >= 10:
        return spot[39]
    k = int(t * 4)
    return spot[k - 1] + (t * 4 - k) * (spot[k] - spot[k - 1])


def _roll(d: date) -> date:
    """토·일이면 다음 평일(월요일)로 (Following). 공휴일은 반영하지 않음."""
    wd = d.weekday()
    return d + timedelta(days=7 - wd) if wd >= 5 else d


def value_prs(p: dict, want_detail: bool = True) -> dict:
    val, start = _pd(p["valuation"]), _pd(p["start"])
    term = int(p.get("term_years", 4) or 4)
    # 계약만료일을 직접 주면 그 날짜를 쓰고, 없으면 개시일 + 계약기간(년) (엑셀과 같음)
    mat = _pd(p["maturity"]) if p.get("maturity") else _pedate(start, 12 * term)
    if mat <= start:
        raise ValueError("계약만료일이 계약개시일보다 뒤여야 합니다.")
    basis = float(p.get("day_basis", 365))
    K = float(p["notional"])
    prem = float(p["premium"])
    pm = int(p.get("pay_months", 3))
    bp = float(p.get("rate_shift_bp", 0.0)) / 10000
    S0 = float(p["usd_equity"]) * float(p["stake"]) * float(p["fx"]) / 1e6 * (1 + float(p.get("equity_chg", 0.0)))
    sigma = float(p["vol"])
    cost = float(p.get("cost_dispose", 0.01)) + float(p.get("cost_tax", 0.0035))
    tax_rate = max(0.0, float(p.get("gain_tax", 0.0) or 0.0))
    tax_bearer = "지급자" if p.get("tax_bearer") == "지급자" else "수령자"
    bear_recv = tax_bearer == "수령자"
    freq = int(p.get("coupon_freq", 2))
    gov = prs_bootstrap(p["gov_ytm"], freq)
    aa = prs_bootstrap(p["aa_ytm"], freq)
    G = lambda t: prs_spot_at(gov["spot"], t) + bp
    A_ = lambda t: prs_spot_at(aa["spot"], t) + bp
    # 평가 모형: "excel" = 엑셀 방식(고정수익 AA- 할인, 트리 q·국고채 + (1−q)·AA- 할인)
    #            "crr"   = 순수 CRR(위험중립확률과 할인 모두 국고채 무위험 금리, 고정수익도 국고채 할인)
    crr = str(p.get("model", "excel")).lower() == "crr"
    arrears = str(p.get("pay_timing", "선급")) in ("후급", "arrears")   # Premium 지급 시점
    FX_ = G if crr else A_
    roll = p.get("biz_roll", True) not in (False, 0, "0", "false", "False")   # 주말 지급일 → 다음 평일
    adj = _roll if roll else (lambda x: x)

    # ── 고정수익 (A)
    rows, A, paid_total = [], 0.0, 0.0
    b = 0
    while _pedate(start, b * pm) < mat:   # 개시일부터 지급주기마다, 만기 직전 회차는 만기일까지 (끝수 기간)
        pay, nxt = _pedate(start, b * pm), min(_pedate(start, (b + 1) * pm), mat)
        days = (nxt - pay).days
        amt = K * prem * days / basis
        pdate0 = nxt if arrears else pay                # 선급: 기간 시작일(첫 회는 계약개시일)에 지급 / 후급: 기간 종료일에 지급
        pdate = adj(pdate0)                             # 실제 지급일 (주말이면 다음 평일, 이자 일수는 그대로)
        live = pdate >= val
        t = (pdate - val).days / basis if live else 0.0
        s = FX_(t) if live else 0.0
        df = (1 + s) ** -t if live else 0.0
        pv = -amt * df if live else 0.0
        A += pv
        if live:
            paid_total += amt
        rows.append(dict(n=b, pay=pay.isoformat(), next=nxt.isoformat(), paydate=pdate.isoformat(), rolled=pdate != pdate0, days=days, amount=amt, live=live, t=t, spot=s, df=df, pv=pv))
        b += 1

    # ── 변동수익 (B) 이항트리
    settle = adj(mat)                                     # 만기 정산 지급일 (주말이면 다음 평일)
    days_left = (settle - val).days
    if days_left <= 0:
        raise ValueError("평가기준일이 만기 이후입니다.")
    gap_want, max_steps = int(p.get("gap_days", 5)), int(p.get("max_steps", 300))
    gap = max(gap_want, math.ceil(days_left / max_steps))
    dt = gap / basis
    N = days_left // gap
    if crr and N >= 1:   # 순수 CRR: 트리가 만기일에 정확히 끝나도록 스텝 길이를 남은 일수 ÷ 스텝 수로 맞춤
        N = max(1, round(days_left / gap))
        dt = days_left / N / basis
    if N < 1:
        raise ValueError("만기까지 남은 기간이 트리 간격보다 짧습니다.")
    u = math.exp(sigma * math.sqrt(dt)); d = 1 / u
    gs = [G(i * dt) for i in range(N + 1)]
    as_ = [A_(i * dt) for i in range(N + 1)]
    fg = [((1 + gs[i + 1]) ** ((i + 1) * dt) / (1 + gs[i]) ** (i * dt)) ** (1 / dt) - 1 for i in range(N)]
    fa = [((1 + as_[i + 1]) ** ((i + 1) * dt) / (1 + as_[i]) ** (i * dt)) ** (1 / dt) - 1 for i in range(N)]
    pp = [((1 + fg[i]) ** dt - d) / (u - d) for i in range(N)]
    # 순수 CRR: 트리에는 주식 매각대금 S·(1−비용)만 넣고(변동 레그), 계약금액 K는 고정 레그로 국고채 할인
    V = [S0 * u ** (N - 2 * j) * (1 - cost) - (0.0 if crr else K) for j in range(N + 1)]
    Q = [1.0 if S0 * u ** (N - 2 * j) * (1 - cost) - K > 0 else 0.0 for j in range(N + 1)]
    # 양도차익 세금: 만기 매각대금(비용 차감 후)이 계약금액 K를 넘는 금액 × 세율 — 같은 트리·같은 할인율로 현재가치
    TX = [tax_rate * max(S0 * u ** (N - 2 * j) * (1 - cost) - K, 0.0) for j in range(N + 1)]
    # 정산분을 양(+)·음(−)으로 나눠 같은 트리로 현재가치 (양수는 세금 차감 후 — 수령자 부담일 때)
    keep = 1.0 - (tax_rate if bear_recv else 0.0)
    XP = [keep * max(S0 * u ** (N - 2 * j) * (1 - cost) - K, 0.0) for j in range(N + 1)]
    XN = [min(S0 * u ** (N - 2 * j) * (1 - cost) - K, 0.0) for j in range(N + 1)]
    for i in range(N - 1, -1, -1):
        pi, g, a = pp[i], fg[i], fa[i]
        for j in range(i + 1):
            q = pi * Q[j] + (1 - pi) * Q[j + 1]
            rate = g if crr else q * g + (1 - q) * a
            disc = (1 + rate) ** dt
            V[j] = (pi * V[j] + (1 - pi) * V[j + 1]) / disc
            TX[j] = (pi * TX[j] + (1 - pi) * TX[j + 1]) / disc
            XP[j] = (pi * XP[j] + (1 - pi) * XP[j + 1]) / disc
            XN[j] = (pi * XN[j] + (1 - pi) * XN[j + 1]) / disc
            Q[j] = q
    tax_pv = TX[0]                                         # 세금 현재가치 (세율 기준, 부담자와 무관)
    tax_ded = tax_pv if bear_recv else 0.0                 # 우리(변동수익 수령자) 평가에서 빼는 금액
    V[0] -= tax_ded
    if crr:
        leg_float = V[0]                                   # 변동 레그: 주식 매각대금 현재가치 (= S0·(1−비용))
        k_pv = K * (1 + gs[N]) ** (-N * dt)                 # 계약금액 K의 현재가치 (국고채, 만기)
        B = leg_float - k_pv                               # 기존 분해의 정산분 B와 같은 값
    else:
        B = V[0]
        k_pv = K * (1 + gs[N]) ** (-N * dt)
        leg_float = None
    out = dict(A=A, B=B, fv=A + B, k_pv=k_pv, leg_float=leg_float, leg_fixed=(A - k_pv) if crr else None, S0=S0, K=K, q0=Q[0], tax_rate=tax_rate, tax_bearer=tax_bearer, tax_pv=tax_pv, tax_ded=tax_ded, B_gross=B + tax_ded, B_pos=XP[0], B_neg=XN[0], model="crr" if crr else "excel", pay_timing="후급" if arrears else "선급", maturity=mat.isoformat(), settle=settle.isoformat(), biz_roll=roll, term_days=(mat - start).days, n_pay=len(rows), N=N, gap=gap, gap_auto=gap > gap_want,
               dt=dt, u=u, d=d, p0=pp[0], fg0=fg[0], fa0=fa[0], days=days_left, cost=cost, sigma=sigma,
               premium_left=paid_total, n_live=sum(1 for r in rows if r["live"]))
    if want_detail:
        out.update(rows=rows, curve=dict(t=gov["t"], gov=gov["spot"], aa=aa["spot"], gov_ytm=gov["ytm"], aa_ytm=aa["ytm"]))
    return out


# 엑셀 시장데이터 시트 (금리DB·시장DB)
PRS_MARKET = {
    "2025-11-07": dict(gov=[2.455, 2.457, 2.513, 2.529, 2.702, 2.8, 2.845, 2.895, 2.995, 3.04, None, None],
                       aa=[2.769, 2.846, 2.915, 2.971, 3.042, 3.175, 3.201, 3.293, 3.36, 3.572, None, None],
                       usd_equity=200000000, fx=1400.0, vol=0.3),
    "2025-12-31": dict(gov=[2.41, 2.393, 2.471, 2.55, 2.745, 2.793, 2.982, 2.95, 3.154, None, None, None],
                       aa=[3.031, 3.073, 3.106, 3.142, 3.166, 3.292, 3.344, 3.459, 3.57, None, None, None],
                       usd_equity=200000000, fx=1400.0, vol=0.3),
}
PRS_REPORT = {}

_m0 = PRS_MARKET["2025-11-07"]
PRS_DEFAULT = dict(name="PRS", pay_timing="선급", model="excel", valuation="2025-11-07", start="2025-10-01", maturity="2029-10-01", term_years=4, notional=100000, premium=0.05,
                   pay_months=3, day_basis=365, usd_equity=_m0["usd_equity"], stake=0.4, fx=_m0["fx"], vol=_m0["vol"],
                   cost_dispose=0.01, cost_tax=0.0035, gain_tax=0.0, tax_bearer="수령자", biz_roll=True, gap_days=5, max_steps=300, coupon_freq=2,
                   gov_ytm=list(_m0["gov"]), aa_ytm=list(_m0["aa"]))



"""
내재파생상품 사채(CB·EB·BW·콜/풋 옵션부 사채) 공정가치 계산 엔진 — 순수 파이썬, 표준 라이브러리만 사용

구성 (권면 1만원 기준으로 계산 후 권면총액만큼 곱함)
  · 제로커브: 국고채 YTM → spot (PRS 계산기와 같은 분기 그리드 bootstrapping, 빈 만기는 앞뒤 만기 선형보간)
      무위험 할인 DF_rf(t) = (1+z(t))^-t, 위험 할인 DF_rk(t) = (1+z(t)+신용스프레드)^-t
  · 사채(주계약): BDT 금리 이항트리를 위험할인커브에 맞춰 보정(calibration)하고 역산
      - 풋(조기상환청구권, 투자자): 지정일마다 max(보유가치, 조기상환금액)
      - 콜(매도청구권, 발행자): 행사기간 중 min(보유가치, 콜 가격), 권면의 일부(비율)만 해당 → 비율로 가중평균
  · CB·EB: 주가 CRR 이항트리 + TF(Tsiveriotis–Fernandes) 분해
      주식 성분은 무위험 선도금리로, 현금(채권) 성분은 위험 선도금리로 할인
      각 노드: 콜(발행자) → 풋(투자자) → 전환(투자자) 순서로 판단
  · BW: 사채(금리트리, 풋·콜 포함) + 신주인수권(주가트리, 아메리칸 콜, 무위험 할인)
  · 리픽싱(선택): 노드 주가 기준 근사 — 전환가 = max(하한, min(최초 전환가, 그 노드 주가))
  · 만기보장수익률·조기상환수익률·콜 수익률: 이자지급주기 복리로 상환금액(원금+할증) 계산
  · Z-spread: 약정 현금흐름(이자+만기상환금)을 국고채 spot + s 로 할인한 값이 공정가치와 같아지는 s
"""
import math
from datetime import date, datetime, timedelta

CB_TENORS = [0.25, 0.5, 0.75, 1, 2, 3, 5, 10, 20, 30]  # 1년 이하 마디 3·6·9개월·1년 (빈칸은 앞뒤 선형보간)
CB_TENORS_V1 = [0.25, 1, 2, 3, 5, 10, 20, 30]  # 예전 8칸 입력(ECOS 고시 만기만)도 그대로 계산
CB_GRID = 120  # 분기 그리드 0.25~30년
CB_UNIT = 10000.0  # 권면 1만원 기준


def _cbd(x) -> date:
    if isinstance(x, datetime):
        return x.date()
    if isinstance(x, date):
        return x
    return datetime.strptime(str(x)[:10], "%Y-%m-%d").date()


def _cbedate(d: date, months: int) -> date:
    m = d.month - 1 + months
    y, m = d.year + m // 12, m % 12 + 1
    nxt = date(y + (m == 12), m % 12 + 1, 1)
    return date(y, m, min(d.day, (nxt - timedelta(days=1)).day))


def _rnd(x):
    return int(math.floor(x + 0.5))


def _months_between(a: date, b: date) -> float:
    return (b.year - a.year) * 12 + (b.month - a.month) + (b.day - a.day) / 30.0


def cb_fill(vals):
    """% 단위 YTM(빈칸 허용) → 소수. 빈 만기는 앞뒤 입력값으로 선형보간, 양 끝은 가장 가까운 값."""
    T = CB_TENORS_V1 if len(vals) == len(CB_TENORS_V1) else CB_TENORS
    pts = [(T[i], float(v) / 100) for i, v in enumerate(vals) if v not in (None, "") and i < len(T)]
    if not pts:
        raise ValueError("국고채 YTM을 하나 이상 입력하세요.")
    out = []
    for t in T:
        if t <= pts[0][0]:
            out.append(pts[0][1]); continue
        if t >= pts[-1][0]:
            out.append(pts[-1][1]); continue
        for (a, ya), (b, yb) in zip(pts, pts[1:]):
            if a <= t <= b:
                out.append(ya + (yb - ya) * (t - a) / (b - a)); break
    return out


def cb_bootstrap(ytm_pct, freq: int = 2):
    """분기 그리드 40점 spot (PRS 계산기 금리커브 시트와 같은 bootstrapping)."""
    y = cb_fill(ytm_pct)
    T = CB_TENORS_V1 if len(ytm_pct) == len(CB_TENORS_V1) else CB_TENORS
    grid = [k / 4 for k in range(1, CB_GRID + 1)]
    node, s_cpn = [], 0.0
    for t in grid:
        cf = abs(t * freq - round(t * freq)) < 1e-9
        if t >= T[-1]:
            yt = y[-1]
        else:
            D = max(i for i in range(len(T)) if T[i] <= t + 1e-12)
            yt = y[D] + (t - T[D]) * (y[D + 1] - y[D]) / (T[D + 1] - T[D])
        if t <= 1:
            df = (1 + yt) ** -t
        elif cf:
            c = yt / freq
            df = (1 - c * s_cpn) / (1 + c)
        else:
            df = None
        if df is not None and cf:
            s_cpn += df
        node.append(None if df is None else df ** (-1 / t) - 1)
    spot = []
    for i, t in enumerate(grid):
        if node[i] is not None:
            spot.append(node[i]); continue
        a = max(k for k in range(i) if node[k] is not None)
        b = min(k for k in range(i + 1, CB_GRID) if node[k] is not None)
        spot.append(node[a] + (t - grid[a]) * (node[b] - node[a]) / (grid[b] - grid[a]))
    return dict(t=grid, spot=spot, ytm=y)


def cb_spot_at(spot, t: float) -> float:
    if t <= 0.25:
        return spot[0]
    if t >= CB_GRID / 4:
        return spot[CB_GRID - 1]
    k = int(t * 4)
    return spot[k - 1] + (t * 4 - k) * (spot[k] - spot[k - 1])


def _accreted(F, c, pm, y, months):
    """발행일부터 months 개월 뒤 상환금액 (보장수익률 y, 이자지급주기 pm개월 복리, 이미 받은 표면이자 차감)."""
    if pm <= 0:
        return F * (1 + y) ** (months / 12)
    m = 12 / pm
    k = months / pm
    paid = int(math.floor(k + 1e-9))
    return F * (1 + y / m) ** k - (c * F / m) * sum((1 + y / m) ** (k - i) for i in range(1, paid + 1))


def _zspread(price, flows, spot, shift):
    """flows=[(t, cf)] — Σ cf/(1+z(t)+shift+s)^t = price 를 만족하는 s (이분법)."""
    f = lambda s: sum(cf * (1 + cb_spot_at(spot, t) + shift + s) ** -t for t, cf in flows) - price
    lo, hi = -0.5, 3.0
    if f(lo) < 0 or f(hi) > 0:
        return None
    for _ in range(100):
        mid = (lo + hi) / 2
        if f(mid) > 0:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


RV_EXP = [1 / 12, 0.25, 0.5, 1, 2, 3, 5, 7, 10]   # 금리 변동성 매트릭스 옵션만기 (년)
RV_TEN = [1, 2, 3, 5, 7, 10]                      # 스왑 기간 (년)


def _lin(xs, ys, x):
    if x <= xs[0]:
        return ys[0]
    if x >= xs[-1]:
        return ys[-1]
    for k in range(len(xs) - 1):
        if xs[k] <= x <= xs[k + 1]:
            return ys[k] + (ys[k + 1] - ys[k]) * (x - xs[k]) / (xs[k + 1] - xs[k])
    return ys[-1]


def cb_rate_sigmas(p: dict, N: int, dt: float, T: float):
    """스텝별 BDT 금리 변동성 σ_i.
    고정: 모든 스텝에 rate_vol.
    매트릭스(Black %, 옵션만기 × 스왑기간): 스텝 i 시점 t_i를 옵션만기, 만기까지 남은 기간 T − t_i를 스왑기간으로 보는
    co-terminal 칸을 이중 선형보간(범위 밖은 끝값)해 σ_i로 씀. BDT에서 t_i의 ln r 분산은 σ_i²·t_i 라서
    σ_i가 곧 '오늘~t_i 평균 변동성'(Black 호가와 같은 뜻)이 됨."""
    fixed = max(1e-6, float(p.get("rate_vol", 0.15)))
    m = p.get("rate_vol_matrix") if p.get("rate_vol_mode") == "matrix" else None
    if not m:
        return [fixed] * N, "fixed"
    ex = [float(x) for x in m.get("exp", RV_EXP)]
    tn = [float(x) for x in m.get("ten", RV_TEN)]
    V = [[float(v) / 100 for v in row] for row in m["vol"]]
    sig = []
    for i in range(N):
        t = i * dt
        e, ten = t, max(T - t, 1e-9)
        col = [_lin(tn, row, ten) for row in V]       # 각 옵션만기 행에서 스왑기간 방향 보간
        sig.append(max(1e-6, _lin(ex, col, e)))       # 옵션만기 방향 보간
    return sig, "matrix"


def value_cb(p: dict, detail: bool = True) -> dict:
    kind = p.get("kind", "CB")
    val, iss, mat = _cbd(p["valuation"]), _cbd(p["issue"]), _cbd(p["maturity"])
    if mat <= val:
        raise ValueError("만기일은 평가일보다 뒤여야 합니다.")
    if val < iss:
        raise ValueError("평가일은 발행일 이후여야 합니다.")
    F = CB_UNIT
    units = float(p.get("face_total", 1e10)) / F
    c = float(p.get("coupon", 0.0))
    pm = int(p.get("pay_months", 3) or 0)
    ytp = float(p.get("ytp", 0.0))
    bp = float(p.get("rate_shift_bp", 0.0)) / 1e4
    cs = float(p.get("credit_spread", 0.03)) + float(p.get("spread_shift_bp", 0.0)) / 1e4
    S0 = float(p.get("spot", 10000)) * (1 + float(p.get("spot_chg", 0.0)))
    sig = max(1e-6, float(p.get("vol", 0.3)) + float(p.get("vol_shift", 0.0)))
    q = float(p.get("div_yield", 0.0))
    gov = p.get("_gov") or cb_bootstrap(p["gov_ytm"], int(p.get("coupon_freq", 2)))
    Z = lambda t: cb_spot_at(gov["spot"], t) + bp
    DFrf = lambda t: (1 + Z(t)) ** -t
    DFrk = lambda t: (1 + Z(t) + cs) ** -t

    days = (mat - val).days
    T = days / 365
    step_days = max(1, int(p.get("step_days", 7)))
    N = max(2, min(int(p.get("max_steps", 300)), math.ceil(days / step_days)))
    dt = T / N
    stp = lambda d: max(0, min(N, _rnd(((d - val).days / 365) / dt)))
    tt = [i * dt for i in range(N + 1)]
    SR, rv_mode = cb_rate_sigmas(p, N, dt, T)          # 스텝별 금리 변동성

    # 표면이자 일정 (평가일 이후 지급분) — 스텝업 금리 선택 가능
    step_on = bool(p.get("step_on"))
    s_start, s_rate = float(p.get("step_start", 6)), float(p.get("step_rate", 0.08))
    s_inc, s_cap = float(p.get("step_inc", 0.01)), float(p.get("step_cap", 0.20))

    def rate_from(d0):
        """d0(이자계산 시작일)에 적용되는 연 표면이자율."""
        if step_on:
            ys = _months_between(iss, d0) / 12
            if ys >= s_start - 1e-9:
                return min(s_cap, s_rate + math.floor(ys - s_start + 1e-9) * s_inc)
        return c

    coupon_at = [0.0] * (N + 1)
    cpn_step = [False] * (N + 1)
    flows, acc = [], 0.0
    total_m = _months_between(iss, mat)
    if pm > 0:
        k, prev = 1, iss
        while True:
            d = _cbedate(iss, k * pm)
            if d > mat:
                break
            amt = rate_from(prev) * F * pm / 12
            if d > val:
                cpn_step[stp(d)] = True
                if amt > 0:
                    coupon_at[stp(d)] += amt
                    flows.append(((d - val).days / 365, amt))
                if prev <= val:
                    acc = amt * (val - prev).days / max(1, (d - prev).days)
            prev = d
            k += 1
    if step_on:
        redemption = F
    elif pm > 0:
        redemption = max(F, _accreted(F, c, pm, ytp, _rnd(total_m)))
    else:
        redemption = F * (1 + ytp) ** ((mat - iss).days / 365)
    flows.append((T, redemption))

    # 풋(조기상환청구) · 콜(매도청구·중도상환) 일정
    put_at = [None] * (N + 1)
    sched = []
    if p.get("put_on"):
        py = float(p.get("put_yield", ytp))
        every = max(1, int(p.get("put_every", 3)))
        d0 = _cbd(p["put_start"])
        j = 0
        while True:
            d = _cbedate(d0, j * every)
            if d >= mat:
                break
            if d >= val:
                amt = _accreted(F, c, pm, py, _rnd(_months_between(iss, d)))
                put_at[stp(d)] = amt
                sched.append(dict(type="풋(조기상환)", date=d.isoformat(), price=amt))
            j += 1
    call_at = [None] * (N + 1)
    alpha = 0.0
    if p.get("call_on"):
        alpha = min(1.0, max(0.0, float(p.get("call_ratio", 0.3))))
        cy = float(p.get("call_yield", ytp))
        par, cpn_only = bool(p.get("call_par")), bool(p.get("call_coupon_only"))
        a, b = _cbd(p["call_start"]), min(_cbd(p["call_end"]), mat)
        for i in range(N + 1):
            d = val + timedelta(days=_rnd(tt[i] * 365))
            if a <= d <= b and (not cpn_only or cpn_step[i]):
                call_at[i] = F if par else _accreted(F, c, pm, cy, _months_between(iss, d))
        sched.append(dict(type=f"콜({'중도상환' if par else '매도청구'} {alpha*100:.0f}%)", date=f"{a.isoformat()} ~ {b.isoformat()}",
                          price=F if par else _accreted(F, c, pm, cy, _months_between(iss, a))))

    drf = [DFrf(tt[i + 1]) / DFrf(tt[i]) for i in range(N)]
    drk = [DFrk(tt[i + 1]) / DFrk(tt[i]) for i in range(N)]

    # ── 일반사채(옵션 없음): 위험할인 DCF
    sb = sum(cf * DFrk(t) for t, cf in flows)

    # ── BDT 금리트리 (위험할인커브에 보정)
    sq = math.sqrt(dt)
    U = [0.0] * N
    Qa = [1.0]
    for i in range(N):
        P = DFrk(tt[i + 1])
        u = -math.log(drk[i]) / dt
        for _ in range(60):
            fs = fp = 0.0
            for j in range(i + 1):
                e = math.exp(SR[i] * (2 * j - i) * sq)
                dfac = math.exp(-u * e * dt)
                fs += Qa[j] * dfac
                fp += Qa[j] * dfac * (-e * dt)
            step_ = (fs - P) / fp
            u -= step_
            if abs(step_) < 1e-14:
                break
        U[i] = u
        Qn = [0.0] * (i + 2)
        for j in range(i + 1):
            dfac = math.exp(-u * math.exp(SR[i] * (2 * j - i) * sq) * dt)
            Qn[j] += 0.5 * Qa[j] * dfac
            Qn[j + 1] += 0.5 * Qa[j] * dfac
        Qa = Qn

    def rate_tree(use_put, use_call):
        V = [redemption + coupon_at[N]] * (N + 1)
        for i in range(N - 1, -1, -1):
            for j in range(i + 1):
                r = U[i] * math.exp(SR[i] * (2 * j - i) * sq)
                v = 0.5 * (V[j] + V[j + 1]) * math.exp(-r * dt)
                if use_call and call_at[i] is not None and v > call_at[i]:
                    v = call_at[i]
                if use_put and put_at[i] is not None and v < put_at[i]:
                    v = put_at[i]
                V[j] = v + (coupon_at[i] if i > 0 else 0.0)
        return V[0]

    host_none = rate_tree(False, False)
    host_put = rate_tree(True, False) if p.get("put_on") else host_none
    host_pc = host_put
    if alpha > 0:
        host_pc = alpha * rate_tree(True, True) + (1 - alpha) * host_put

    # ── 주가트리
    u_ = math.exp(sig * sq); d_ = 1 / u_
    pr = [(math.exp(-q * dt) / drf[i] - d_) / (u_ - d_) for i in range(N)]
    cp0 = float(p.get("conv_price", 10000))
    refix = bool(p.get("refix_on"))
    floor = float(p.get("refix_floor", 0.7)) * cp0
    k_ref = stp(_cbd(p["refix_start"])) if refix and p.get("refix_start") else 0
    k_cs = stp(_cbd(p.get("conv_start") or p["issue"]))
    k_ce = stp(_cbd(p.get("conv_end") or p["maturity"]))
    cs_ok = lambda i: k_cs <= i <= k_ce and _cbd(p.get("conv_end") or p["maturity"]) >= val
    ratio = float(p.get("bw_ratio", 1.0))

    def cp_at(i, S):
        if refix and i >= k_ref:
            return max(floor, min(cp0, S))
        return cp0

    cr = ratio if (kind == "BW" and p.get("bw_pay") == "대용") else 1.0

    def tf_tree(use_call):
        S = [S0 * u_ ** j * d_ ** (N - j) for j in range(N + 1)]
        E = [0.0] * (N + 1); D = [0.0] * (N + 1)
        for j in range(N + 1):
            bond = redemption + coupon_at[N]
            cv = F * cr / cp_at(N, S[j]) * S[j]
            if cs_ok(N) and cv > bond:
                E[j], D[j] = cv, 0.0
            else:
                E[j], D[j] = 0.0, bond
        for i in range(N - 1, -1, -1):
            p_, a_, b_ = pr[i], drf[i], drk[i]
            for j in range(i + 1):
                s = S0 * u_ ** j * d_ ** (i - j)
                e = a_ * (p_ * E[j + 1] + (1 - p_) * E[j])
                dd = b_ * (p_ * D[j + 1] + (1 - p_) * D[j])
                conv = cs_ok(i)
                cv = F * cr / cp_at(i, s) * s if conv else 0.0
                v = e + dd
                if use_call and call_at[i] is not None and v > call_at[i]:
                    if conv and cv >= call_at[i]:
                        e, dd = cv, 0.0
                    else:
                        e, dd = 0.0, call_at[i]
                if put_at[i] is not None and put_at[i] > e + dd:
                    e, dd = 0.0, put_at[i]
                if conv and cv > e + dd:
                    e, dd = cv, 0.0
                E[j] = e
                D[j] = dd + (coupon_at[i] if i > 0 else 0.0)
        return E[0] + D[0], E[0]

    def warrant_tree():
        V = [0.0] * (N + 1)
        for j in range(N + 1):
            s = S0 * u_ ** j * d_ ** (N - j)
            X = cp_at(N, s)
            V[j] = (F * ratio / X) * max(s - X, 0.0) if cs_ok(N) else 0.0
        for i in range(N - 1, -1, -1):
            p_, a_ = pr[i], drf[i]
            for j in range(i + 1):
                v = a_ * (p_ * V[j + 1] + (1 - p_) * V[j])
                if cs_ok(i):
                    s = S0 * u_ ** j * d_ ** (i - j)
                    X = cp_at(i, s)
                    v = max(v, (F * ratio / X) * max(s - X, 0.0))
                V[j] = v
        return V[0]

    eq_part = 0.0
    sub = kind == "BW" and p.get("bw_pay") == "대용"
    if kind in ("CB", "EB") or sub:
        nc, enc = tf_tree(False)
        if alpha > 0:
            wc, ewc = tf_tree(True)
            total = alpha * wc + (1 - alpha) * nc
            eq_part = alpha * ewc + (1 - alpha) * enc
        else:
            total, eq_part = nc, enc
        option = total - host_pc
    elif kind == "BW":
        option = warrant_tree()
        total = host_pc + option
    else:
        option = 0.0
        total = host_pc

    parity = (F / cp0) * S0 if kind != "SB" else 0.0
    z_total = _zspread(total, flows, gov["spot"], bp)
    z_host = _zspread(host_pc, flows, gov["spot"], bp)
    z_sb = _zspread(sb, flows, gov["spot"], bp)
    ytm_total = _irr(total, flows)
    out = dict(kind=kind, unit=F, units=units, total=total, total_amt=total * units, sb=sb, host_none=host_none,
               host_put=host_put, host_pc=host_pc, put_value=host_put - host_none, call_value=host_pc - host_put,
               option=option, eq_part=eq_part, accrued=acc, clean=total - acc, parity=parity,
               premium=(total / parity - 1) if parity > 0 else None, redemption=redemption,
               z_total=z_total, z_host=z_host, z_sb=z_sb, ytm=ytm_total, N=N, dt=dt, T=T, days=days, sigma=sig,
               S0=S0, cs=cs, p0=pr[0], calib_err=abs(host_none - sb),
               rate_vol_mode=rv_mode, rate_sig_min=min(SR[1:] or SR), rate_sig_max=max(SR[1:] or SR))
    if detail:
        out["flows"] = [dict(t=t, cf=cf) for t, cf in flows]
        out["sched"] = sched
        out["curve"] = dict(t=gov["t"], spot=gov["spot"], ytm=gov["ytm"])
        out["rate_sigs"] = [dict(t=tt[i], sigma=SR[i]) for i in range(N)]
    return out


def cb_oas(p: dict, price: float, lo: float = -0.05, hi: float = 0.6):
    """시장가격(권면 1만원당)을 맞추는 OAS — 사채(현금) 성분 할인에 쓰는 국고채 위 스프레드. 없으면 None."""
    q = dict(p); q["spread_shift_bp"] = 0.0
    q["_gov"] = q.get("_gov") or cb_bootstrap(q["gov_ytm"], int(q.get("coupon_freq", 2)))
    f = lambda s: value_cb(dict(q, credit_spread=s), False)["total"] - price
    fa, fb = f(lo), f(hi)
    if fa < 0 or fb > 0:
        return None
    a_, b_ = lo, hi
    for _ in range(60):  # Illinois(가위치법) — 보통 10회 안팎에 수렴
        m = b_ - fb * (b_ - a_) / (fb - fa)
        fm = f(m)
        if abs(fm) < 1e-7 or abs(b_ - a_) < 1e-10:
            return m
        if fm * fb < 0:
            a_, fa = b_, fb
        else:
            fa /= 2
        b_, fb = m, fm
    return b_


def _irr(price, flows):
    f = lambda y: sum(cf * (1 + y) ** -t for t, cf in flows) - price
    lo, hi = -0.9, 5.0
    if f(lo) < 0 or f(hi) > 0:
        return None
    for _ in range(100):
        m = (lo + hi) / 2
        if f(m) > 0:
            lo = m
        else:
            hi = m
    return (lo + hi) / 2


CB_DEFAULT = dict(name="", kind="CB", valuation="2026-10-08", issue="2026-03-31", maturity="2029-03-31",
                  face_total=10_000_000_000, coupon=0.01, pay_months=3, ytp=0.03,
                  put_on=True, put_start="2027-09-30", put_every=3, put_yield=0.03,
                  call_on=True, call_start="2027-03-31", call_end="2028-03-31", call_yield=0.04, call_ratio=0.3,
                  conv_start="2027-03-31", conv_end="2029-02-28", conv_price=12000, bw_ratio=1.0,
                  spot=10000, vol=0.40, div_yield=0.0,
                  refix_on=True, refix_floor=0.7, refix_start="2026-06-30",
                  credit_spread=0.03, rate_vol=0.15, coupon_freq=2, step_days=7, max_steps=300,
                  gov_ytm=[3.24, 3.741, 3.908, 3.983, 4.158, 4.395, 4.404, 4.448],
                  step_on=False, step_start=6, step_rate=0.08, step_inc=0.01, step_cap=0.20,
                  call_par=False, call_coupon_only=False, bw_pay="현금", market_price=0)

# 공시 사례 (DART 주요사항보고서 원문 기준, 시장 입력값은 가정 — 화면에서 수정 가능)
CB_PRESETS = {
    "롯데에너지머티리얼즈 제1회 BW (2025-02 발행, 스틱 사모)": dict(
        CB_DEFAULT, name="롯데에너지머티리얼즈 제1회 BW", kind="BW", valuation="2026-10-08", issue="2025-02-25", maturity="2055-02-25",
        face_total=150_000_000_000, coupon=0.0, pay_months=3, ytp=0.0,
        step_on=True, step_start=6, step_rate=0.08, step_inc=0.01, step_cap=0.20,
        put_on=False, put_start="2026-02-25",
        call_on=True, call_start="2031-02-25", call_end="2055-02-25", call_par=True, call_coupon_only=True, call_ratio=1.0, call_yield=0.0,
        conv_start="2026-02-25", conv_end="2055-02-25", conv_price=28612, bw_ratio=1.0, bw_pay="대용",
        spot=55200, vol=0.45, div_yield=0.0, refix_on=False, refix_floor=0.7, refix_start="2026-02-25",
        credit_spread=0.035, rate_vol=0.15, step_days=7,
        _src="DART 주요사항보고서(신주인수권부사채권발행결정) 2024-11-25, 접수번호 20241125000377",
        _assume="주가 55,200원(2026-10-08 종가, 재경일보 보도), 변동성 45%·신용스프레드 350bp는 가정값"),
}



"""공통 금리커브: 고시 수익률(= par 금리로 봄) → 선형보간 → 부트스트랩 → 제로금리·할인계수·선도금리.

  · 입력: 만기(년)별 연 % 수익률. 빈 만기는 앞뒤 고시값으로 선형보간, 양 끝 밖은 가장 가까운 값.
  · 분기(0.25년) 그리드. 1년 이하는 단기 금리로 보고 할인계수 = (1+y)^-t (PRS·사채 계산기와 같은 방식).
  · 1년 초과 이표일(freq: 국고채·미국채 연 2회, 회사채 연 4회)은 par 조건으로 순차 부트스트랩
      DF_n = (1 − c·ΣDF_이전이표일) / (1 + c),  c = par/freq
  · 이표일이 아닌 그리드 점의 제로금리는 앞뒤 이표일 제로금리를 선형보간.
  · 제로금리는 연복리(z: DF = (1+z)^-t)와 연속복리(ln(1+z)) 두 가지로 표시.
  · 선도금리 f(t1,t2) = (DF(t1)/DF(t2))^(1/(t2−t1)) − 1 (연복리).
  · 회사채 5년 초과: 한국신용평가 표가 5년까지라 '5년 신용스프레드 유지' — 국고채 par + (등급 5년 − 국고채 5년).
"""
import math

CV_STEP = 0.25
CV_FWD_START = [0, 0.25, 0.5, 0.75, 1, 2, 3, 5, 7, 10]
CV_FWD_TENOR = [0.25, 0.5, 0.75, 1, 2, 3, 5]
KIS_TENORS = [0.25, 0.5, 0.75, 1, 1.5, 2, 3, 5]
KTB_LONG = [10, 20, 30]
GRADES = ["AAA", "AA+", "AA", "AA-", "A+", "A", "A-", "BBB+", "BBB", "BBB-"]


def cv_interp(x, xs, ys):
    if x <= xs[0]:
        return ys[0]
    if x >= xs[-1]:
        return ys[-1]
    for i in range(len(xs) - 1):
        if xs[i] <= x <= xs[i + 1]:
            return ys[i] + (ys[i + 1] - ys[i]) * (x - xs[i]) / (xs[i + 1] - xs[i])
    return ys[-1]


def cv_points(tenors, rates_pct):
    pts = sorted((float(t), float(r) / 100) for t, r in zip(tenors, rates_pct) if r not in (None, ""))
    if not pts:
        raise ValueError("금리를 하나 이상 입력하세요.")
    return [p[0] for p in pts], [p[1] for p in pts]


def cv_build(tenors, rates_pct, freq=2, max_t=None):
    xs, ys = cv_points(tenors, rates_pct)
    max_t = max_t or xs[-1]
    n = max(1, int(round(math.ceil(max_t / CV_STEP - 1e-9))))
    grid = [round((k + 1) * CV_STEP, 10) for k in range(n)]
    par = [cv_interp(t, xs, ys) for t in grid]
    node, s_cpn = [], 0.0
    for t, y in zip(grid, par):
        cf = abs(t * freq - round(t * freq)) < 1e-9
        if t <= 1 + 1e-12:
            df = (1 + y) ** -t
        elif cf:
            c = y / freq
            df = (1 - c * s_cpn) / (1 + c)
        else:
            df = None
        if df is not None and cf:
            s_cpn += df
        node.append(None if df is None else df ** (-1 / t) - 1)
    zero = []
    for i, t in enumerate(grid):
        if node[i] is not None:
            zero.append(node[i]); continue
        a = max(k for k in range(i) if node[k] is not None)
        nb = [k for k in range(i + 1, n) if node[k] is not None]
        if not nb:
            zero.append(node[a]); continue
        b = nb[0]
        zero.append(node[a] + (t - grid[a]) * (node[b] - node[a]) / (grid[b] - grid[a]))
    df = [(1 + z) ** -t for t, z in zip(grid, zero)]
    fwd = [zero[0]] + [(df[i - 1] / df[i]) ** (1 / (grid[i] - grid[i - 1])) - 1 for i in range(1, n)]
    return dict(t=grid, par=par, zero=zero, zero_cc=[math.log(1 + z) for z in zero], df=df, fwd=fwd,
                freq=freq, src_t=xs, src_r=ys)


def cv_zero(c, t):
    """t년 제로금리(연복리). 그리드 사이는 선형보간, 3개월 미만은 첫 점, 끝 밖은 마지막 점."""
    return cv_interp(t, c["t"], c["zero"])


def cv_df(c, t):
    return 1.0 if t <= 0 else (1 + cv_zero(c, t)) ** -t


def cv_fwd(c, t1, t2):
    if t2 <= t1:
        return None
    return (cv_df(c, t1) / cv_df(c, t2)) ** (1 / (t2 - t1)) - 1


def cv_par(c, t):
    return cv_interp(t, c["t"], c["par"])


def cv_fwd_matrix(c):
    T = c["t"][-1]
    return [[(cv_fwd(c, s, s + m) if s + m <= T + 1e-9 else None) for m in CV_FWD_TENOR] for s in CV_FWD_START]


# ───────── 원자료 → 커브별 (만기, 금리) ─────────
def ktb_points(ecos, kis_gov=None):
    """국고채: 3M·6M·9M·1Y·1.5Y·2Y·3Y·5Y는 한국신용평가(KIS자산평가 민평), 10·20·30Y는 ECOS.
    KIS 자료가 없으면 ECOS 1·2·3·5Y를 쓴다. ecos = {만기: %}"""
    pts = {}
    if kis_gov:
        pts.update({t: v for t, v in zip(KIS_TENORS, kis_gov) if v is not None})
        pts.update({t: ecos[t] for t in KTB_LONG if ecos.get(t) is not None})
    else:
        pts.update({t: v for t, v in ecos.items() if v is not None})
    ts = sorted(pts)
    return ts, [pts[t] for t in ts]


def corp_points(grade_vals, kis_gov, ktb_t, ktb_r):
    """회사채 등급: 5Y 이하는 표 그대로, 5Y 초과(국고채 10·20·30Y 만기)는 5Y 신용스프레드를 국고채에 더함."""
    sp5 = grade_vals[-1] - kis_gov[-1]
    ts = list(KIS_TENORS) + [t for t in ktb_t if t > 5]
    rs = list(grade_vals) + [r + sp5 for t, r in zip(ktb_t, ktb_r) if t > 5]
    return ts, rs


def value_curve(p):
    """화면 계산용: p = dict(tenors, rates(%), freq, max_t)"""
    c = cv_build(p["tenors"], p["rates"], int(p.get("freq", 2)), p.get("max_t"))
    c["fwd_matrix"] = cv_fwd_matrix(c)
    c["fwd_start"], c["fwd_tenor"] = CV_FWD_START, CV_FWD_TENOR
    return c



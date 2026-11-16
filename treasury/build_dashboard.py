"""자금 지급일정 대시보드(엑셀) 생성 스크립트.

실행: python3 treasury/build_dashboard.py
결과: treasury/자금지급일정_대시보드.xlsx  (+ treasury/vba/*.bas, *.cls 를 가져와 .xlsm 으로 저장)

시트 구성
  대시보드   보고장: 요약 숫자, 월별 누적막대 차트, 월별 표, K-IFRS 1107 만기분석
  상세       차트 더블클릭(매크로) 또는 월·구분 선택으로 해당 항목을 FILTER 로 표시
  매입채무   입력: ERP 지급예정표 붙여넣기
  차입금     입력 + 적용금리·잔액·유동/비유동 자동 계산
  환율·금리  기준일, 서울외국환중개 매매기준율(버튼), 기준금리, 시나리오
  일정       계산용: 매입채무·차입이자·차입원금을 한 목록으로 펼침
  안내       매크로 설치 방법
"""
import datetime as dt
import math
import os

import xlsxwriter

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "자금지급일정_대시보드.xlsx")

BASE = dt.date(2026, 11, 16)
AP_MAX = 1000            # 매입채무 최대 건수 (시트 5~1004행)
LOAN_MAX = 30            # 차입금 최대 건수 (시트 6~35행)
K_MAX = 120              # 차입금 1건당 최대 이자 회차
AP_FIRST = 5
LOAN_FIRST = 6
LOAN_LAST = LOAN_FIRST + LOAN_MAX - 1

# 일정 시트 행 배치
S_AP0 = 2
S_INT0 = S_AP0 + AP_MAX                 # 1002
S_PRIN0 = S_INT0 + LOAN_MAX * K_MAX     # 4602
S_LAST = S_PRIN0 + LOAN_MAX * K_MAX - 1 # 8201

FX = "'환율·금리'"
SCH = "일정"
LN = "차입금"


def rng(col):
    return f"{SCH}!${col}${S_AP0}:${col}${S_LAST}"


# ---------------------------------------------------------------- 예시 자료
def add_months(d, n):
    m = d.month - 1 + n
    y, m = d.year + m // 12, m % 12 + 1
    last = (dt.date(y + m // 12, m % 12 + 1, 1) - dt.timedelta(days=1)).day
    return dt.date(y, m, min(d.day, last))


def sample_loans():
    D = dt.date
    return [
        ("L-001", "A은행", "운전자금", "KRW", 10e9, D(2026, 2, 15), D(2027, 2, 15), "만기일시", "고정", 4.15, None, None, 3, 365),
        ("L-002", "B은행", "운전자금", "KRW", 5e9, D(2025, 8, 28), D(2027, 8, 28), "만기일시", "변동", None, "CD91", 1.20, 3, 365),
        ("L-003", "C캐피탈", "시설자금", "KRW", 6e9, D(2024, 3, 20), D(2029, 3, 20), "분할", "변동", None, "CD91", 1.85, 3, 365),
        ("L-004", "A은행", "시설자금", "KRW", 12e9, D(2023, 6, 30), D(2028, 6, 30), "분할", "고정", 3.80, None, None, 3, 365),
        ("L-005", "D은행 뉴욕지점", "운전자금", "USD", 8e6, D(2025, 12, 10), D(2027, 6, 10), "만기일시", "변동", None, "SOFR3M", 1.10, 3, 360),
        ("L-006", "E은행 프랑크푸르트", "운전자금", "EUR", 3e6, D(2026, 5, 5), D(2027, 5, 5), "만기일시", "변동", None, "EURIBOR3M", 0.95, 3, 360),
        ("L-007", "제12회 공모사채", "회사채", "KRW", 20e9, D(2024, 11, 25), D(2027, 11, 25), "만기일시", "고정", 4.45, None, None, 3, 365),
        ("L-008", "B은행", "단기차입", "KRW", 3e9, D(2026, 9, 1), D(2026, 12, 31), "만기일시", "고정", 4.30, None, None, 1, 365),
    ]


def sample_ap():
    """웹 시안과 같은 난수(mulberry32)로 같은 예시 송장을 만든다."""
    M = 0xFFFFFFFF
    state = [20261116]

    def imul(a, b):
        return (a * b) & M

    def rnd():
        s = (state[0] + 0x6D2B79F5) & M
        state[0] = s
        t = imul(s ^ (s >> 15), 1 | s)
        t = ((t + imul(t ^ (t >> 7), 61 | t)) & M) ^ t
        return ((t ^ (t >> 14)) & M) / 4294967296

    V = [("㈜한빛소재", "KRW"), ("대성정밀", "KRW"), ("세원물류", "KRW"), ("동림화학", "KRW"), ("미래포장", "KRW"),
         ("청우전자", "KRW"), ("Pacific Resin Inc.", "USD"), ("Westline Components", "USD"),
         ("Kessler Maschinenbau GmbH", "EUR"), ("Sato Trading Co.", "JPY")]
    rngs = {"KRW": (30e6, 850e6), "USD": (25e3, 520e3), "EUR": (20e3, 260e3), "JPY": (3e6, 38e6)}
    out = []
    for i in range(72):
        vendor, ccy = V[math.floor(rnd() * len(V))]
        mo = min(5, math.floor(rnd() * rnd() * 7))
        first = add_months(dt.date(2026, 11, 1), mo)
        last = (add_months(first, 1) - dt.timedelta(days=1)).day
        day = [10, 25, last][math.floor(rnd() * 3)]
        due = dt.date(first.year, first.month, day)
        if due <= BASE:
            due = dt.date(2026, 11, 25)
        lo, hi = rngs[ccy]
        unit = 1e4 if ccy == "KRW" else 1
        amt = math.floor((lo + rnd() * (hi - lo)) / unit + 0.5) * unit
        out.append((vendor, f"INV-{first.year % 100:02d}{first.month:02d}-{i + 1:04d}", ccy, amt, due))
    return sorted(out, key=lambda r: r[4])


# ---------------------------------------------------------------- 서식
wb = xlsxwriter.Workbook(OUT, {"use_future_functions": True})
wb.set_properties({"title": "자금 지급일정 대시보드", "comments": "매입채무·차입금 지급 예정 대시보드"})
wb.set_calc_mode("auto")

FONT = "맑은 고딕"
INK, INK2, MUTED, LINE, CHIP = "#131820", "#465061", "#737D8C", "#DFE3EA", "#EEF1F5"
NAVY, NAVY_SOFT = "#1C4C8C", "#E5EDF8"
S1, S2, S3 = "#2A78D6", "#EB6834", "#1BAF7A"
INPUT_BG, CALC_BG = "#FFFFFF", "#F3F5F8"


def F(**kw):
    base = {"font_name": FONT, "font_size": 10, "font_color": INK, "valign": "vcenter"}
    base.update(kw)
    return wb.add_format(base)


f_title = F(font_size=18, bold=True)
f_eyebrow = F(font_size=9, font_color=MUTED, bold=True)
f_sub = F(font_color=MUTED)
f_note = F(font_size=9, font_color=MUTED, text_wrap=True, valign="top")
f_h2 = F(font_size=12, bold=True)
f_th = F(bold=True, font_color=INK2, bg_color=CHIP, bottom=1, bottom_color=LINE)
f_th_r = F(bold=True, font_color=INK2, bg_color=CHIP, bottom=1, bottom_color=LINE, align="right")
f_td = F(bottom=1, bottom_color=LINE)
f_td_date = F(bottom=1, bottom_color=LINE, num_format="yyyy-mm-dd", align="left")
f_td_num = F(bottom=1, bottom_color=LINE, num_format="#,##0")
f_td_num2 = F(bottom=1, bottom_color=LINE, num_format="#,##0.##")
f_td_mon = F(bottom=1, bottom_color=LINE, num_format='yyyy"년" m"월"', align="left")
f_tot = F(bold=True, bg_color=CHIP, top=1, top_color=LINE)
f_tot_num = F(bold=True, bg_color=CHIP, top=1, top_color=LINE, num_format="#,##0")
f_kpi_lab = F(indent=1, font_size=9, font_color=MUTED, bold=True, bg_color="#FFFFFF", left=1, right=1, top=1, border_color=LINE)
f_kpi_val = F(indent=1, font_size=18, bold=True, bg_color="#FFFFFF", left=1, right=1, border_color=LINE, num_format="#,##0", align="left")
f_kpi_mon = F(indent=1, font_size=18, bold=True, bg_color="#FFFFFF", left=1, right=1, border_color=LINE, num_format='yyyy"년" m"월"', align="left")
f_kpi_desc = F(indent=1, font_size=9, font_color=INK2, bg_color="#FFFFFF", left=1, right=1, bottom=1, border_color=LINE)
f_in = F(bg_color=INPUT_BG, border=1, border_color=LINE)
f_in_date = F(bg_color=INPUT_BG, border=1, border_color=LINE, num_format="yyyy-mm-dd", align="left")
f_in_num = F(bg_color=INPUT_BG, border=1, border_color=LINE, num_format="#,##0.##")
f_in_pct = F(bg_color=INPUT_BG, border=1, border_color=LINE, num_format="0.00")
f_calc = F(bg_color=CALC_BG, border=1, border_color=LINE, font_color=INK2)
f_calc_num = F(bg_color=CALC_BG, border=1, border_color=LINE, font_color=INK2, num_format="#,##0")
f_calc_num2 = F(bg_color=CALC_BG, border=1, border_color=LINE, font_color=INK2, num_format="#,##0.####")
f_calc_pct = F(bg_color=CALC_BG, border=1, border_color=LINE, font_color=INK2, num_format="0.00")
f_calc_date = F(bg_color=CALC_BG, border=1, border_color=LINE, font_color=INK2, num_format="yyyy-mm-dd", align="left")
f_calc_ts = F(bg_color=CALC_BG, border=1, border_color=LINE, font_color=INK2, num_format="yyyy-mm-dd hh:mm", align="left")
f_warn = F(font_color="#9A4A00", bold=True)
f_badge = F(font_size=9, font_color="#9A4A00", bg_color="#FDF1DF", bold=True, align="center")
f_big_in = F(bg_color=INPUT_BG, border=2, border_color=NAVY, num_format="yyyy-mm-dd", bold=True, font_size=12, align="left")
f_sel = F(bg_color=NAVY_SOFT, border=1, border_color=NAVY, bold=True, num_format='yyyy"년" m"월"', align="left")
f_sel_txt = F(bg_color=NAVY_SOFT, border=1, border_color=NAVY, bold=True, align="left")
f_s = {"매입채무": F(bottom=1, bottom_color=LINE, font_color=S1, bold=True),
       "차입원금": F(bottom=1, bottom_color=LINE, font_color=S2, bold=True),
       "차입이자": F(bottom=1, bottom_color=LINE, font_color=S3, bold=True)}

ws_dash = wb.add_worksheet("대시보드")
ws_det = wb.add_worksheet("상세")
ws_ap = wb.add_worksheet("매입채무")
ws_ln = wb.add_worksheet("차입금")
ws_fx = wb.add_worksheet("환율·금리")
ws_s = wb.add_worksheet("일정")
ws_help = wb.add_worksheet("안내")

for w in (ws_dash, ws_det, ws_ap, ws_ln, ws_fx, ws_s, ws_help):
    w.hide_gridlines(2)
    w.set_default_row(18)
ws_dash.set_tab_color(NAVY)
ws_det.set_tab_color(NAVY)
ws_s.set_tab_color("#B7BFCB")
ws_help.set_tab_color("#B7BFCB")

# ---------------------------------------------------------------- 환율·금리
w = ws_fx
w.set_column("A:A", 2)
w.set_column("B:B", 24)
w.set_column("C:C", 18)
w.set_column("D:D", 14)
w.set_column("E:E", 10)
w.set_column("F:F", 14)
w.set_column("G:G", 3)
w.set_column("H:K", 11)
w.write("B2", "환율·금리 설정", f_title)
w.write("B3", "보고서(대시보드)에 쓰는 기준일, 환율, 기준금리를 여기서 관리합니다.", f_sub)
w.write("B4", "보고 기준일", f_h2)
w.write_datetime("C4", dt.datetime.combine(BASE, dt.time()), f_big_in)
wb.define_name("기준일", f"={FX}!$C$4")

w.write("B6", "서울외국환중개 매매기준율", f_h2)
w.write("B7", "기준일(휴일이면 직전 영업일)에 고시된 매매기준율입니다. 오른쪽 버튼을 누르면 가져옵니다.", f_sub)
for c, h in zip("BCDEF", ["통화", "환산계수(원/1단위)", "매매기준율", "고시 단위", "고시일"]):
    w.write(f"{c}8", h, f_th_r if c in "CD" else f_th)
fx_rows = [("KRW", "=1", None, "–"), ("USD", "=D10", 1385.0, "1달러"),
           ("EUR", "=D11", 1505.0, "1유로"), ("JPY", "=D12/100", 920.0, "100엔")]
for i, (ccy, conv, rate, unit) in enumerate(fx_rows):
    r = 9 + i
    w.write(f"B{r}", ccy, f_calc)
    w.write_formula(f"C{r}", conv, f_calc_num2)
    if rate is None:
        w.write(f"D{r}", "–", f_calc)
        w.write(f"F{r}", "", f_calc)
    else:
        w.write_number(f"D{r}", rate, f_in_num)
        w.write(f"F{r}", "예시 값", f_calc_date)
    w.write(f"E{r}", unit, f_calc)
wb.define_name("환산표", f"={FX}!$B$9:$C$12")
w.write("B13", "가져온 시각", f_td)
w.write("C13", "", f_calc_ts)
w.write("B14", "가져오기 결과", f_td)
w.merge_range("C14:F14", "예시 값입니다. 버튼을 눌러 기준일 매매기준율로 바꾸세요.", f_calc)
w.write("H7", "버튼 자리 (매크로 설정 후 표시)", f_note)

w.write("B16", "기준금리 (%)", f_h2)
for c, h in zip("BCD", ["코드", "금리(%)", "설명"]):
    w.write(f"{c}17", h, f_th)
idx_rows = [("CD91", 3.10, "CD 91일물"), ("KORIBOR3M", 3.25, "KORIBOR 3개월"),
            ("SOFR3M", 4.05, "Term SOFR 3개월"), ("EURIBOR3M", 2.15, "EURIBOR 3개월")]
for i, (code, v, desc) in enumerate(idx_rows):
    r = 18 + i
    w.write(f"B{r}", code, f_calc)
    w.write_number(f"C{r}", v, f_in_pct)
    w.write(f"D{r}", desc, f_calc)
wb.define_name("기준금리표", f"={FX}!$B$18:$C$21")
w.write("B23", "변동금리 시나리오 (bp)", f_h2)
w.write_number("C23", 0, F(bg_color=INPUT_BG, border=2, border_color=NAVY, bold=True, num_format='+0"bp";-0"bp";0"bp"'))
w.data_validation("C23", {"validate": "integer", "criteria": "between", "minimum": -500, "maximum": 500,
                          "error_message": "-500 ~ 500 사이의 bp 값을 입력하세요."})
wb.define_name("시나리오bp", f"={FX}!$C$23")
w.merge_range("B24:F25", "변동금리 차입금의 이자에 일괄 가감합니다. 대시보드 차트·월별 표에는 반영되고, "
              "K-IFRS 1107 만기분석에는 반영하지 않습니다(기준일 금리 사용, 문단 B11D).", f_note)
w.freeze_panes(5, 0)

# ---------------------------------------------------------------- 매입채무
w = ws_ap
w.set_column("A:A", 28)
w.set_column("B:B", 18)
w.set_column("C:C", 8)
w.set_column("D:D", 16)
w.set_column("E:E", 12)
w.set_column("F:F", 16)
w.write("A1", "매입채무 지급 예정", f_title)
w.write("A2", f"ERP 지급예정표를 5행부터 붙여 넣으세요 (최대 {AP_MAX:,}건). 지금 들어 있는 72건은 예시입니다. F열은 자동 계산됩니다.", f_sub)
for c, h in zip("ABCDEF", ["거래처", "송장번호", "통화", "금액(원통화)", "지급기일", "원화환산(원)"]):
    w.write(f"{c}4", h, f_th_r if c in "DF" else f_th)
ap = sample_ap()
for i in range(AP_MAX):
    r = AP_FIRST + i
    if i < len(ap):
        v, inv, ccy, amt, due = ap[i]
        w.write(f"A{r}", v, f_in)
        w.write(f"B{r}", inv, f_in)
        w.write(f"C{r}", ccy, f_in)
        w.write_number(f"D{r}", amt, f_in_num)
        w.write_datetime(f"E{r}", dt.datetime.combine(due, dt.time()), f_in_date)
    w.write_formula(f"F{r}", f'=IF(D{r}="","",D{r}*IFERROR(VLOOKUP(C{r},환산표,2,0),0))', f_calc_num)
w.data_validation(f"C{AP_FIRST}:C{AP_FIRST + AP_MAX - 1}", {"validate": "list", "source": ["KRW", "USD", "EUR", "JPY"]})
w.data_validation(f"E{AP_FIRST}:E{AP_FIRST + AP_MAX - 1}", {"validate": "date", "criteria": ">", "minimum": dt.date(2000, 1, 1),
                                                          "error_message": "지급기일은 날짜로 입력하세요."})
w.freeze_panes(4, 1)
w.autofilter(f"A4:F{AP_FIRST + AP_MAX - 1}")

# ---------------------------------------------------------------- 차입금
w = ws_ln
cols = [("A", "차입번호", 10), ("B", "금융기관", 20), ("C", "구분", 10), ("D", "통화", 7), ("E", "원금(최초, 원통화)", 17),
        ("F", "차입일", 11), ("G", "만기일", 11), ("H", "상환방식", 9), ("I", "금리유형", 8), ("J", "고정금리(%)", 10),
        ("K", "기준금리", 11), ("L", "가산금리(%)", 10), ("M", "이자주기(개월)", 12), ("N", "일수기준", 8),
        ("O", "적용금리(%)", 10), ("P", "총 회차", 7), ("Q", "기준일 잔액(원통화)", 18), ("R", "기준일 잔액(원)", 17),
        ("S", "유동(원)", 16), ("T", "비유동(원)", 16), ("U", "점검", 22)]
for c, h, wd in cols:
    w.set_column(f"{c}:{c}", wd)
    w.write(f"{c}5", h, f_th_r if c in "EJLMOPQRST" else f_th)
w.write("A1", "차입금 명세", f_title)
w.write("A2", f"흰 칸(A~N열)을 입력하면 회색 칸(O~U열)과 이자·원금 일정이 자동 계산됩니다. 최대 {LOAN_MAX}건, 1건당 이자 {K_MAX}회차까지. 지금 자료는 예시입니다.", f_sub)
w.write("A3", "유동/비유동: 기준일로부터 12개월 안에 상환할 원금은 유동부채로 분류합니다 (K-IFRS 1001호 문단 69).", f_sub)
loans = sample_loans()
for j in range(LOAN_MAX):
    r = LOAN_FIRST + j
    if j < len(loans):
        L = loans[j]
        for c, v in zip("ABCDEFGHIJKLMN", L):
            if v is None:
                w.write_blank(f"{c}{r}", None, f_in)
            elif isinstance(v, dt.date):
                w.write_datetime(f"{c}{r}", dt.datetime.combine(v, dt.time()), f_in_date)
            elif isinstance(v, (int, float)):
                w.write_number(f"{c}{r}", v, f_in_pct if c in "JL" else f_in_num)
            else:
                w.write(f"{c}{r}", v, f_in)
    else:
        for c in "ABCDEFGHIJKLMN":
            fmt = f_in_date if c in "FG" else f_in_pct if c in "JL" else f_in_num if c in "EMN" else f_in
            w.write_blank(f"{c}{r}", None, fmt)
    j1 = j + 1
    w.write_formula(f"O{r}", f'=IF(A{r}="","",IF(I{r}="고정",J{r},IFERROR(VLOOKUP(K{r},기준금리표,2,0),NA())+L{r}))', f_calc_pct)
    q = f'MAX(1,ROUNDUP(DATEDIF(F{r},G{r},"m")/M{r},0))'
    w.write_formula(f"P{r}", f'=IF(OR(A{r}="",F{r}="",G{r}="",M{r}=""),"",{q}+IF(EDATE(F{r},{q}*M{r})<G{r},1,0))', f_calc_num)
    w.write_formula(f"Q{r}", f'=IF(A{r}="","",E{r}-SUMIFS({rng("Q")},{rng("L")},{j1},{rng("A")},"<="&기준일))', f_calc_num2)
    w.write_formula(f"R{r}", f'=IF(A{r}="","",Q{r}*IFERROR(VLOOKUP(D{r},환산표,2,0),0))', f_calc_num)
    w.write_formula(f"S{r}", f'=IF(A{r}="","",SUMIFS({rng("Q")},{rng("L")},{j1},{rng("A")},">"&기준일,{rng("A")},"<="&EDATE(기준일,12))'
                             f'*IFERROR(VLOOKUP(D{r},환산표,2,0),0))', f_calc_num)
    w.write_formula(f"T{r}", f'=IF(A{r}="","",R{r}-S{r})', f_calc_num)
    w.write_formula(f"U{r}", f'=IF(A{r}="","",IF(ISNA(O{r}),"기준금리 코드 확인",IF(P{r}>{K_MAX},"회차 {K_MAX} 초과: 일정 일부 누락",'
                             f'IF(G{r}<=F{r},"만기일이 차입일보다 빠름",""))))', F(bg_color=CALC_BG, border=1, border_color=LINE, font_color="#9A4A00"))
rr = f"{LOAN_FIRST}:{LOAN_LAST}"
w.data_validation(f"D{LOAN_FIRST}:D{LOAN_LAST}", {"validate": "list", "source": ["KRW", "USD", "EUR", "JPY"]})
w.data_validation(f"H{LOAN_FIRST}:H{LOAN_LAST}", {"validate": "list", "source": ["만기일시", "분할"]})
w.data_validation(f"I{LOAN_FIRST}:I{LOAN_LAST}", {"validate": "list", "source": ["고정", "변동"]})
w.data_validation(f"K{LOAN_FIRST}:K{LOAN_LAST}", {"validate": "list", "source": f"={FX}!$B$18:$B$21"})
w.data_validation(f"N{LOAN_FIRST}:N{LOAN_LAST}", {"validate": "list", "source": ["365", "360"]})
w.data_validation(f"M{LOAN_FIRST}:M{LOAN_LAST}", {"validate": "list", "source": ["1", "3", "6", "12"]})
w.write(f"A{LOAN_LAST + 1}", "합계(원)", f_tot)
for c in "BCDEFGHIJKLMNOPQ":
    w.write_blank(f"{c}{LOAN_LAST + 1}", None, f_tot)
for c in "RST":
    w.write_formula(f"{c}{LOAN_LAST + 1}", f"=SUM({c}{LOAN_FIRST}:{c}{LOAN_LAST})", f_tot_num)
w.write_blank(f"U{LOAN_LAST + 1}", None, f_tot)
w.freeze_panes(5, 2)

# ---------------------------------------------------------------- 일정 (계산용)
w = ws_s
heads = [("A", "지급일", 11), ("B", "구분", 9), ("C", "거래처·금융기관", 22), ("D", "내용", 44), ("E", "통화", 6),
         ("F", "원통화 금액", 16), ("G", "원화환산(원)", 16), ("H", "유효", 5), ("I", "월(말일)", 11), ("J", "참조번호", 14),
         ("K", "원화(기준금리)", 16), ("L", "차입#", 6), ("M", "회차", 5), ("N", "총회차", 6), ("O", "이전 지급일", 11),
         ("P", "기초잔액", 16), ("Q", "원금상환", 16), ("R", "적용금리(%)", 9), ("S", "이자(기준금리)", 14)]
for c, h, wd in heads:
    w.set_column(f"{c}:{c}", wd)
    w.write(f"{c}1", h, f_th)
date_c = F(num_format="yyyy-mm-dd", align="left")
num_c = F(num_format="#,##0.##")
plain = F()
fx_of = lambda cell: f"IFERROR(VLOOKUP({cell},환산표,2,0),0)"
valid_date = lambda r: f'IF(AND(ISNUMBER(A{r}),A{r}>=기준일),1,0)'

# 매입채무 블록
for i in range(AP_MAX):
    r, a = S_AP0 + i, AP_FIRST + i
    w.write_formula(f"A{r}", f'=IF(매입채무!$E{a}="","",매입채무!$E{a})', date_c)
    w.write(f"B{r}", "매입채무", plain)
    w.write_formula(f"C{r}", f"=매입채무!$A{a}&\"\"", plain)
    w.write_formula(f"D{r}", f'=IF(매입채무!$B{a}="","","송장 "&매입채무!$B{a})', plain)
    w.write_formula(f"E{r}", f"=매입채무!$C{a}&\"\"", plain)
    w.write_formula(f"F{r}", f"=N(매입채무!$D{a})", num_c)
    w.write_formula(f"G{r}", f"=F{r}*{fx_of(f'E{r}')}", num_c)
    w.write_formula(f"H{r}", f"={valid_date(r)}", plain)
    w.write_formula(f"I{r}", f'=IF(H{r}=1,EOMONTH(A{r},0),"")', date_c)
    w.write_formula(f"J{r}", f"=매입채무!$B{a}&\"\"", plain)
    w.write_formula(f"K{r}", f"=G{r}", num_c)

# 차입이자 블록 (차입 j, 회차 k) 과 차입원금 블록
for j in range(LOAN_MAX):
    lr = LOAN_FIRST + j
    X = lambda c: f"{LN}!${c}${lr}"
    for k in range(1, K_MAX + 1):
        r = S_INT0 + j * K_MAX + (k - 1)
        p = S_PRIN0 + j * K_MAX + (k - 1)
        ok = f'AND(ISNUMBER({X("P")}),{k}<={X("P")})'
        w.write_formula(f"A{r}", f'=IF({ok},IF({k}={X("P")},{X("G")},EDATE({X("F")},{k}*{X("M")})),"")', date_c)
        w.write(f"B{r}", "차입이자", plain)
        w.write_formula(f"C{r}", f'=IF(A{r}="","",{X("B")})', plain)
        w.write_formula(f"D{r}", f'=IF(A{r}="","",{X("A")}&" "&{k}&"/"&N{r}&"회차 이자 ("&TEXT(O{r},"yyyy-mm-dd")&"~"&TEXT(A{r},"yyyy-mm-dd")&", "&TEXT(R{r},"0.00")&"%)")', plain)
        w.write_formula(f"E{r}", f'=IF(A{r}="","",{X("D")})', plain)
        w.write_formula(f"F{r}", f'=IF(A{r}="",0,P{r}*R{r}/100*(A{r}-O{r})/{X("N")})', num_c)
        w.write_formula(f"G{r}", f"=F{r}*{fx_of(f'E{r}')}", num_c)
        w.write_formula(f"H{r}", f'=IF(AND(ISNUMBER(A{r}),A{r}>기준일),1,0)', plain)
        w.write_formula(f"I{r}", f'=IF(H{r}=1,EOMONTH(A{r},0),"")', date_c)
        w.write_formula(f"J{r}", f'=IF(A{r}="","",{X("A")})', plain)
        w.write_formula(f"K{r}", f"=S{r}*{fx_of(f'E{r}')}", num_c)
        w.write_number(f"L{r}", j + 1, plain)
        w.write_number(f"M{r}", k, plain)
        w.write_formula(f"N{r}", f'={X("P")}', plain)
        w.write_formula(f"O{r}", f'=IF(A{r}="","",IF({k}=1,{X("F")},EDATE({X("F")},{k - 1}*{X("M")})))', date_c)
        w.write_formula(f"P{r}", f'=IF(A{r}="",0,IF({X("H")}="분할",{X("E")}-{X("E")}/N{r}*{k - 1},{X("E")}))', num_c)
        w.write_formula(f"Q{r}", f'=IF(A{r}="",0,IF({X("H")}="분할",{X("E")}/N{r},IF({k}=N{r},{X("E")},0)))', num_c)
        w.write_formula(f"R{r}", f'=IF(A{r}="","",{X("O")}+IF({X("I")}="변동",시나리오bp/100,0))', num_c)
        w.write_formula(f"S{r}", f'=IF(A{r}="",0,P{r}*{X("O")}/100*(A{r}-O{r})/{X("N")})', num_c)
        # 원금 블록: 같은 (j, k) 의 이자 행을 참조
        w.write_formula(f"A{p}", f"=A{r}", date_c)
        w.write(f"B{p}", "차입원금", plain)
        w.write_formula(f"C{p}", f"=C{r}", plain)
        w.write_formula(f"D{p}", f'=IF(A{p}="","",{X("A")}&IF({X("H")}="분할"," 분할상환 "&{k}&"/"&N{r}&"회"," 만기 일시상환"))', plain)
        w.write_formula(f"E{p}", f"=E{r}", plain)
        w.write_formula(f"F{p}", f"=Q{r}", num_c)
        w.write_formula(f"G{p}", f"=F{p}*{fx_of(f'E{p}')}", num_c)
        w.write_formula(f"H{p}", f'=IF(AND(ISNUMBER(A{p}),A{p}>기준일,F{p}>0),1,0)', plain)
        w.write_formula(f"I{p}", f'=IF(H{p}=1,EOMONTH(A{p},0),"")', date_c)
        w.write_formula(f"J{p}", f"=J{r}", plain)
        w.write_formula(f"K{p}", f"=G{p}", num_c)
w.freeze_panes(1, 1)
w.write("U1", "계산용 시트입니다. 직접 고치지 마세요. 2~1001행 매입채무, 1002~4601행 차입이자, 4602~8201행 차입원금.", f_note)

# ---------------------------------------------------------------- 대시보드
w = ws_dash
w.set_column("A:A", 2)
w.set_column("B:M", 11.5)
w.set_column("N:N", 2)
w.set_row(1, 30)
w.write("B2", "자금 지급일정 대시보드", f_title)
w.write_formula("B3", '="기준일 "&TEXT(기준일,"yyyy-mm-dd")&"  ·  단위 백만원(원화 환산)  ·  환율 고시일 "&IFERROR(TEXT(\'환율·금리\'!F10,"yyyy-mm-dd"),\'환율·금리\'!F10)', f_sub)
w.merge_range("K2:M2", "예시 자료", f_badge)

MR0, MR1 = 32, 43   # 월별 표 행
cat_rng = lambda c: f"${c}${MR0}:${c}${MR1}"
kpis = [
    ("B", "D", "향후 12개월 지급 예정 (백만원)", f"=F{MR1 + 1}", f_kpi_val,
     f'=TEXT(G{MR1 + 1},"#,##0")&"건 · 외화 비중 "&TEXT(IFERROR(SUMIFS({rng("G")},{rng("H")},1,{rng("I")},"<="&B{MR1},{rng("E")},"<>KRW")/1000000/F{MR1 + 1},0),"0.0%")'),
    ("E", "G", "지급이 가장 많은 달", f"=INDEX(B{MR0}:B{MR1},MATCH(MAX(F{MR0}:F{MR1}),F{MR0}:F{MR1},0))", f_kpi_mon,
     f'=TEXT(MAX(F{MR0}:F{MR1}),"#,##0")&"백만원"'),
    ("H", "J", "12개월 차입이자 (백만원)", f"=E{MR1 + 1}", f_kpi_val,
     f'=IF(시나리오bp=0,"기준금리 현재 수준 가정","시나리오 "&TEXT(시나리오bp,"+0;-0")&"bp · 기준 대비 "&TEXT(E{MR1 + 1}-SUMIFS({rng("K")},{rng("B")},"차입이자",{rng("H")},1,{rng("I")},"<="&B{MR1})/1000000,"+#,##0;-#,##0;0")&"백만원")'),
    ("K", "M", "차입금 잔액 (백만원)", f"=SUM({LN}!R{LOAN_FIRST}:R{LOAN_LAST})/1000000", f_kpi_val,
     f'="변동금리 "&TEXT(IFERROR(SUMIFS({LN}!R{LOAN_FIRST}:R{LOAN_LAST},{LN}!I{LOAN_FIRST}:I{LOAN_LAST},"변동")/SUM({LN}!R{LOAN_FIRST}:R{LOAN_LAST}),0),"0.0%")&" · "&COUNTA({LN}!A{LOAN_FIRST}:A{LOAN_LAST})&"건"'),
]
w.set_row(5, 30)
for c0, c1, lab, val, vf, desc in kpis:
    w.merge_range(f"{c0}5:{c1}5", lab, f_kpi_lab)
    w.merge_range(f"{c0}6:{c1}6", "", vf)
    w.write_formula(f"{c0}6", val, vf)
    w.merge_range(f"{c0}7:{c1}7", "", f_kpi_desc)
    w.write_formula(f"{c0}7", desc, f_kpi_desc)

w.write("B9", "월별 지급 예정액", f_h2)
w.write("B10", "막대에 커서를 올리면 금액이 보이고, 막대를 더블클릭하면 「상세」 시트에 그달·그 구분의 항목이 표시됩니다.", f_sub)

w.write(f"B{MR0 - 1}", "월", f_th)
for c, h in zip("CDEFG", ["매입채무", "차입원금", "차입이자", "합계", "건수"]):
    w.write(f"{c}{MR0 - 1}", h, f_th_r)
w.write(f"B{MR0 - 2}", "월별 표", f_h2)
for i in range(12):
    r = MR0 + i
    w.write_formula(f"B{r}", f"=EOMONTH(기준일,{i})", F(bottom=1, bottom_color=LINE, num_format='yyyy"년" m"월"', align="left"))
    for c, cat in zip("CDE", ["매입채무", "차입원금", "차입이자"]):
        w.write_formula(f"{c}{r}", f'=SUMIFS({rng("G")},{rng("B")},"{cat}",{rng("I")},$B{r},{rng("H")},1)/1000000', f_td_num)
    w.write_formula(f"F{r}", f"=SUM(C{r}:E{r})", F(bottom=1, bottom_color=LINE, num_format="#,##0", bold=True))
    w.write_formula(f"G{r}", f'=COUNTIFS({rng("I")},$B{r},{rng("H")},1)', f_td_num)
w.write(f"B{MR1 + 1}", "합계", f_tot)
for c in "CDEFG":
    w.write_formula(f"{c}{MR1 + 1}", f"=SUM({c}{MR0}:{c}{MR1})", f_tot_num)
w.write(f"B{MR1 + 2}", "첫 달은 기준일 이후 지급분만 집계합니다. 차입이자는 환율·금리 시트의 시나리오(bp)를 반영합니다.", f_note)


chart = wb.add_chart({"type": "column", "subtype": "stacked"})
for col, color in zip("CDE", [S1, S2, S3]):
    chart.add_series({
        "name": f"=대시보드!${col}${MR0 - 1}",
        "categories": f"=대시보드!$B${MR0}:$B${MR1}",
        "values": f"=대시보드!${col}${MR0}:${col}${MR1}",
        "fill": {"color": color},
        "border": {"color": "#FFFFFF", "width": 1},
        "gap": 70,
        "overlap": 100,
    })
chart.set_x_axis({"text_axis": True, "num_format": "yy.mm", "num_font": {"name": FONT, "size": 9, "color": MUTED}, "line": {"color": "#B7BFCB"},
                  "major_tick_mark": "none"})
chart.set_y_axis({"num_format": "#,##0", "num_font": {"name": FONT, "size": 9, "color": MUTED},
                  "major_gridlines": {"visible": True, "line": {"color": "#E9ECF1"}}, "line": {"none": True},
                  "name": "백만원", "name_font": {"name": FONT, "size": 9, "color": MUTED, "bold": False}})
chart.set_legend({"position": "top", "font": {"name": FONT, "size": 9, "color": INK2}})
chart.set_chartarea({"border": {"none": True}, "fill": {"color": "#FFFFFF"}})
chart.set_plotarea({"fill": {"none": True}})
chart.set_title({"none": True})
chart.set_size({"width": 1010, "height": 340})
w.insert_chart("B11", chart, {"x_offset": 0, "y_offset": 4, "object_position": 1})

# K-IFRS 1107 만기분석
T0 = MR1 + 4   # 47
w.write(f"B{T0}", "금융부채 계약상 만기분석 (K-IFRS 1107호 문단 39(1), B11)", f_h2)
heads = ["구분", "", "장부금액", "1년 이내", "1~2년", "2~5년", "5년 초과", "계약상 현금흐름"]
for c, h in zip("BCDEFGHI", heads):
    w.write(f"{c}{T0 + 1}", h, f_th if c in "BC" else f_th_r)
y = lambda n: f"EDATE(기준일,{n})"
bk = [f'{rng("A")},"<="&{y(12)}', f'{rng("A")},">"&{y(12)},{rng("A")},"<="&{y(24)}',
      f'{rng("A")},">"&{y(24)},{rng("A")},"<="&{y(60)}', f'{rng("A")},">"&{y(60)}']
rows1107 = [("매입채무", "매입채무", f'=SUMIFS({rng("G")},{rng("B")},"매입채무",{rng("H")},1)/1000000'),
            ("차입금 원금", "차입원금", f"=SUM({LN}!R{LOAN_FIRST}:R{LOAN_LAST})/1000000"),
            ("차입금 이자", "차입이자", None)]
for i, (lab, cat, book) in enumerate(rows1107):
    r = T0 + 2 + i
    w.merge_range(f"B{r}:C{r}", lab, f_s[cat])
    if book:
        w.write_formula(f"D{r}", book, f_td_num)
    else:
        w.write(f"D{r}", "–", F(bottom=1, bottom_color=LINE, align="right"))
    for c, b in zip("EFGH", bk):
        w.write_formula(f"{c}{r}", f'=SUMIFS({rng("K")},{rng("B")},"{cat}",{rng("H")},1,{b})/1000000', f_td_num)
    w.write_formula(f"I{r}", f"=SUM(E{r}:H{r})", F(bottom=1, bottom_color=LINE, num_format="#,##0", bold=True))
rt = T0 + 5
w.merge_range(f"B{rt}:C{rt}", "합계", f_tot)
for c in "DEFGHI":
    w.write_formula(f"{c}{rt}", f"=SUM({c}{T0 + 2}:{c}{T0 + 4})", f_tot_num)
w.merge_range(f"B{rt + 1}:M{rt + 2}", "할인하지 않은 계약상 현금흐름(원금+이자)입니다. 변동금리 이자는 기준일 현재 기준금리로 계산하며(문단 B11D) "
              "시나리오 bp는 반영하지 않습니다. 외화는 기준일 매매기준율로 환산합니다.", f_note)
w.set_landscape()
w.fit_to_pages(1, 0)
w.print_area(f"A1:N{rt + 2}")
w.activate()
w.set_first_sheet()

# ---------------------------------------------------------------- 상세
w = ws_det
w.set_column("A:A", 2)
w.set_column("B:B", 12)
w.set_column("C:C", 10)
w.set_column("D:D", 24)
w.set_column("E:E", 46)
w.set_column("F:F", 7)
w.set_column("G:H", 17)
w.write("B2", "상세 내역", f_title)
w.write("B3", "대시보드 차트의 막대를 더블클릭하면 아래 월·구분이 바뀝니다. 직접 골라도 됩니다.", f_sub)
w.write("B4", "월", f_h2)
w.merge_range("C4:D4", "", f_sel)
w.write_datetime("C4", dt.datetime(2026, 11, 30), f_sel)
w.data_validation("C4", {"validate": "list", "source": f"=대시보드!$B${MR0}:$B${MR1}"})
w.write("B5", "구분", f_h2)
w.merge_range("C5:D5", "전체", f_sel_txt)
w.data_validation("C5", {"validate": "list", "source": ["전체", "매입채무", "차입원금", "차입이자"]})
w.write("B6", "합계(원)", f_h2)
w.merge_range("C6:D6", "", F(bold=True, num_format="#,##0", align="left"))
w.write_formula("C6", f'=SUMIFS({rng("G")},{rng("H")},1,{rng("I")},C4,{rng("B")},IF(C5="전체","*",C5))', F(bold=True, num_format="#,##0", align="left"))
w.write_formula("E6", f'=COUNTIFS({rng("H")},1,{rng("I")},C4,{rng("B")},IF(C5="전체","*",C5))&"건"', F(bold=True))
for c, h in zip("BCDEFGH", ["지급일", "구분", "거래처·금융기관", "내용", "통화", "원통화 금액", "원화환산(원)"]):
    w.write(f"{c}8", h, f_th_r if c in "GH" else f_th)
w.set_column("B:B", 12, F(num_format="yyyy-mm-dd", align="left"))
w.set_column("G:G", 17, F(num_format="#,##0.##"))
w.set_column("H:H", 17, F(num_format="#,##0"))
src = f"{SCH}!$A${S_AP0}:$G${S_LAST}"
w.write_dynamic_array_formula(
    "B9:B9",
    f'=IFERROR(SORT(FILTER({src},({rng("H")}=1)*({rng("I")}=$C$4)*(($C$5="전체")+({rng("B")}=$C$5))),1,1),"해당 기간에 항목이 없습니다.")',
    F(num_format="yyyy-mm-dd", align="left"))
w.freeze_panes(8, 0)

# ---------------------------------------------------------------- 안내
w = ws_help
w.set_column("A:A", 2)
w.set_column("B:B", 110)
lines = [
    ("자금 지급일정 대시보드 사용 안내", f_title),
    ("", None),
    ("1. 매크로 넣기 (처음 한 번)", f_h2),
    ("① 이 파일을 연 상태에서 Alt+F11 → [파일] → [파일 가져오기]로 vba 폴더의 modTreasury.bas, clsChartEvents.cls 두 파일을 가져옵니다.", None),
    ("② [파일] → [다른 이름으로 저장] → 파일 형식 'Excel 매크로 사용 통합 문서(*.xlsm)'로 저장합니다.", None),
    ("③ Alt+F8 → Setup 실행. 환율·금리 시트에 '서울외국환중개 매매기준율 가져오기' 버튼이 생기고, 차트 더블클릭이 연결됩니다.", None),
    ("   이후에는 파일을 열 때마다 차트 더블클릭이 자동으로 연결됩니다(매크로 사용 허용 필요).", None),
    ("", None),
    ("2. 매달 쓰는 순서", f_h2),
    ("① 환율·금리 시트: 보고 기준일(C4)을 바꾸고 버튼을 눌러 매매기준율을 가져옵니다. 기준금리(CD91 등)를 갱신합니다.", None),
    ("② 매입채무 시트: ERP 지급예정표를 5행부터 붙여 넣습니다 (거래처, 송장번호, 통화, 금액, 지급기일).", None),
    ("③ 차입금 시트: 신규 차입·상환을 반영합니다. U열 '점검'에 문구가 있으면 그 행을 확인합니다.", None),
    ("④ 대시보드에서 막대를 더블클릭하면 상세 시트로 이동합니다.", None),
    ("", None),
    ("3. 계산 기준", f_h2),
    ("· 이자 = 기초잔액 × 적용금리 × 실제일수 / 일수기준(365 또는 360). 지급일은 차입일로부터 이자주기(개월)마다, 마지막 회차는 만기일.", None),
    ("· 분할상환은 매 이자지급일에 원금을 균등하게 상환하는 것으로 계산합니다. 거치기간·불균등 상환은 아직 지원하지 않습니다.", None),
    ("· 변동금리 = 기준금리 + 가산금리. 앞으로의 기준금리는 지금 값이 유지된다고 가정하고, 시나리오 bp를 일괄 가감합니다.", None),
    ("· 영업일 조정(휴일이면 익영업일 지급)은 하지 않습니다.", None),
    ("", None),
    ("4. 환율 가져오기가 안 될 때", f_h2),
    ("· 회사 방화벽이 www.smbs.biz 접속을 막으면 실패합니다. 이때는 환율·금리 시트 D10:D12에 매매기준율을 직접 입력하면 됩니다.", None),
    ("· 매크로는 Windows용 Excel 기준입니다 (Mac Excel에서는 환율 가져오기가 동작하지 않습니다).", None),
]
for i, (t, f) in enumerate(lines):
    if t:
        w.write(i + 1, 1, t, f or F(text_wrap=True))

wb.close()
print("written", OUT)

// Python 원본 엔진과 대조할 입력 + Rust 결과를 파일로 저장
const D = require('./reference/defaults.js'); const wasm = require('./pkg/fincalc_engine.js'); const rust = require('../docs/engine.js')(wasm.call);
let seed = 7; const rnd = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296; };
const U = (a, b) => a + (b - a) * rnd(), I = (a, b) => Math.floor(U(a, b + 1)), pick = a => a[I(0, a.length - 1)], clone = o => JSON.parse(JSON.stringify(o));
const iso = n => new Date(n * 86400000).toISOString().slice(0, 10), dn = s => Date.parse(s) / 86400000, sh = (s, lo, hi) => iso(dn(s) + I(lo, hi));
const parR = b => b.map(v => Math.max(0.0005, v + U(-0.01, 0.01))), DC = ['ACT/360', 'ACT/365', '30/360'], FREQ = [1, 3, 6, 12];
const cases = []; const add = (kind, p, f) => { let r, e; try { r = f(clone(p)); } catch (x) { e = x.message; } cases.push({ kind, p, r, e }); };
const N = +(process.argv[2] || 60);
add('irs', { ...D.irs, par_rates: D.curve.krw }, rust.SwapJS.irs); add('crs', { ...D.crs, krw_par_rates: D.curve.krw, usd_par_rates: D.curve.usd }, rust.SwapJS.crs); add('fx', D.fx, rust.SwapJS.fx);
add('prs', D.prs, rust.PrsJS.value); add('cb', D.cb, rust.CbJS.value); add('cb', Object.values(D.cb_presets)[0], rust.CbJS.value);
for (const k of Object.keys(D.prs_market)) { const m = D.prs_market[k]; add('prs', { ...D.prs, valuation: k, gov_ytm: m.gov, aa_ytm: m.aa, usd_equity: m.usd_equity, fx: m.fx, vol: m.vol }, rust.PrsJS.value); }
add('opt', { ...D.opt, sigma: rust.OptionJS.histVol(D.opt.prices, '일별', 30).sigma }, rust.OptionJS.value);
for (let i = 0; i < N; i++) {
  { const p = clone(D.irs); p.par_rates = parR(D.curve.krw); p.effective = sh(p.effective, -900, 400); p.maturity = sh(p.effective, 30, 4000); p.valuation = sh(p.effective, -300, 1500);
    p.notional = U(1e6, 1e11); p.direction = pick(['Pay Fixed', 'Receive Fixed']); p.fixed_rate = U(0.001, 0.08); p.fixed_freq = pick(FREQ); p.float_freq = pick(FREQ); p.fixed_dc = pick(DC); p.float_dc = pick(DC); p.float_spread = U(-0.01, 0.01); add('irs', p, rust.SwapJS.irs); }
  { const p = clone(D.crs); p.krw_par_rates = parR(D.curve.krw); p.usd_par_rates = parR(D.curve.usd); p.effective = sh(p.effective, -900, 400); p.maturity = sh(p.effective, 30, 4000); p.valuation = sh(p.effective, -300, 1500);
    p.direction = pick(['KRW 지급 / USD 수취', 'USD 지급 / KRW 수취']); p.exchange = pick(['기초+만기 교환', '만기 교환', '교환 없음']); p.krw_type = pick(['Fixed', 'Floating']); p.usd_type = pick(['Fixed', 'Floating']);
    p.krw_freq = pick(FREQ); p.usd_freq = pick(FREQ); p.krw_dc = pick(DC); p.usd_dc = pick(DC); p.fx_initial = U(1000, 1500); p.fx_valuation = U(1000, 1500); p.usd_notional = U(1e5, 1e9); p.krw_fixed_rate = U(0, 0.07); p.usd_fixed_rate = U(0, 0.07); p.krw_spread = U(-0.01, 0.01); p.usd_spread = U(-0.01, 0.01); add('crs', p, rust.SwapJS.crs); }
  { const p = clone(D.fx); p.krw_par_rates = parR(D.curve.krw); p.usd_par_rates = parR(D.curve.usd); p.valuation = sh(p.valuation, -500, 500); p.maturity = sh(p.valuation, 1, 1500); p.direction = pick(['USD 매입', 'USD 매도']);
    p.spot = U(900, 1600); p.contract_rate = U(900, 1600); p.usd_notional = U(1e4, 1e9); p.swap_point = pick(['', null, U(-50, 50)]); add('fx', p, rust.SwapJS.fx); }
  { const p = clone(D.opt); delete p.prices; p.valuation = sh(p.valuation, -400, 400); p.maturity = sh(p.valuation, 8, 2500); p.spot = U(100, 1e5); p.strike = p.spot * U(0.5, 1.6); p.quantity = pick([1, 250, 12.5]);
    p.strike_mode = pick(['이자율연동(IRR)', '고정']); p.strike_irr = U(0, 0.08); p.kind = pick(['콜', '풋']); p.style = pick(['유러피언', '아메리칸']); p.step = pick(['주간', '일간']); p.rate_quoted = U(0, 0.07);
    p.rate_comp = pick(['단리(연1회복리)', '연 2회 복리', '연 4회 복리', '연 12회 복리(월복리)']); p.sigma = U(0.05, 0.9); add('opt', p, rust.OptionJS.value); }
  { const p = clone(D.prs); const m = D.prs_market[pick(Object.keys(D.prs_market))]; p.gov_ytm = m.gov.map(v => v == null ? null : v + U(-0.5, 0.5)); p.aa_ytm = m.aa.map(v => v == null ? null : v + U(-0.5, 0.5));
    p.start = sh(p.start, -600, 300); p.maturity = sh(p.start, 200, 2500); p.valuation = sh(p.start, -30, 180); p.pay_timing = pick(['선급', '후급']); p.model = pick(['excel', 'crr']); p.premium = U(0.01, 0.09); p.pay_months = pick([1, 3, 6, 12]);
    p.day_basis = pick([365, 360]); p.vol = U(0.1, 0.8); p.gain_tax = pick([0, 0.22]); p.tax_bearer = pick(['수령자', '지급자']); p.biz_roll = pick([true, false]); p.gap_days = pick([5, 7, 10]); p.max_steps = pick([300, 100]); p.coupon_freq = pick([2, 4]);
    p.fx = U(1100, 1500); p.stake = U(0.1, 1); p.rate_shift_bp = pick([0, 50, -100]); p.equity_chg = pick([0, 0.1, -0.3]); p.notional = U(5e4, 2e5); add('prs', p, rust.PrsJS.value); }
  { const p = clone(rnd() < 0.25 ? Object.values(D.cb_presets)[0] : D.cb); p.kind = pick(['CB', 'EB', 'BW', 'SB', 'CB']); p.bw_pay = pick(['현금', '대용']);
    if (rnd() < 0.7) { p.issue = sh(p.issue, -400, 200); p.maturity = sh(p.issue, 400, 3000); p.valuation = sh(p.issue, 0, 390); p.put_start = sh(p.issue, 100, 900); p.call_start = sh(p.issue, 100, 900); p.call_end = sh(p.call_start, 30, 2000); p.conv_start = sh(p.issue, 30, 500); p.conv_end = sh(p.maturity, -60, 0); p.refix_start = sh(p.issue, 30, 400); }
    p.coupon = pick([0, 0.01, 0.025, 0.05]); p.pay_months = pick([3, 6, 12, 0, 1]); p.ytp = U(0, 0.06); p.put_on = rnd() < 0.6; p.put_every = pick([3, 6, 12]); p.put_yield = U(0, 0.06); p.call_on = rnd() < 0.6; p.call_ratio = pick([0.3, 1, 0.5]); p.call_yield = U(0, 0.06);
    p.call_par = rnd() < 0.3 && !p.refix_on; p.call_coupon_only = rnd() < 0.3; p.conv_price = U(5000, 60000); p.spot = U(3000, 80000); p.vol = U(0.1, 0.8); p.div_yield = pick([0, 0.01, 0.03]); p.refix_on = rnd() < 0.5 && !p.call_par; p.refix_floor = pick([0.7, 0.8]);
    p.credit_spread = U(0.005, 0.09); p.rate_vol = pick([0.15, 0.05, 0.3]); p.coupon_freq = pick([2, 4]); p.step_days = pick([7, 14, 30]); p.max_steps = pick([120, 60]); p.step_on = rnd() < 0.25; p.bw_ratio = pick([1, 0.5]); p.gov_ytm = p.gov_ytm.map((v, j) => j && rnd() < 0.2 ? null : v + U(-0.5, 0.5));
    if (rnd() < 0.3) { p.rate_vol_mode = 'matrix'; p.rate_vol_matrix = { exp: [1 / 12, 0.25, 0.5, 1, 2, 3, 5, 7, 10], ten: [1, 2, 3, 5, 7, 10], vol: Array.from({ length: 9 }, () => Array.from({ length: 6 }, () => U(10, 40))) }; }
    add('cb', p, rust.CbJS.value); }
  { const ten = pick([[0.25, 0.5, 0.75, 1, 1.5, 2, 3, 5, 10, 20, 30], [0.25, 1, 2, 3, 5, 10]]); add('curve', { tenors: ten, rates: ten.map((t, j) => j && rnd() < 0.15 ? null : 2.5 + 0.05 * t + U(-0.3, 0.3)), freq: pick([2, 4, 1]), max_t: pick([30, 10]) }, rust.CurveJS.value); }
}
// 무작위 대조에서 갈린 칼날 사례 (콜 가격 = 전환가치가 소수 끝자리까지 같은 경우)
add('cb', {"name":"","kind":"CB","valuation":"2028-05-23","issue":"2025-12-29","maturity":"2031-05-03","face_total":10000000000,"coupon":0.05,"pay_months":1,"ytp":0.047927044336684044,"put_on":true,"put_start":"2026-10-02","put_every":12,"put_yield":0.03697538025677204,"call_on":true,"call_start":"2026-09-05","call_end":"2031-08-11","call_yield":0.04622421150561422,"call_ratio":0.5,"conv_start":null,"conv_end":"","conv_price":46702.421254012734,"bw_ratio":1,"spot":11010.203672805801,"vol":0.431324645318091,"div_yield":0.03,"refix_on":true,"refix_floor":0.7,"refix_start":"","credit_spread":0.04337804222013801,"rate_vol":0.3,"coupon_freq":2,"step_days":1,"max_steps":120,"gov_ytm":[null,4.194248441917821,null,3.983,4.158,4.395,4.048784179748967,4.420039069747553],"step_on":false,"call_par":true,"call_coupon_only":true,"bw_pay":"현금"}, rust.CbJS.value);
require('fs').writeFileSync('py_cases.json', JSON.stringify(cases, (k, v) => typeof v === 'number' && !isFinite(v) ? null : (k.startsWith('_c') ? undefined : v)));
console.log('cases', cases.length);

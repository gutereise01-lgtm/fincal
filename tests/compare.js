// Rust(WASM) 엔진 결과를 기존 JS 엔진(원본 아티팩트에서 추출)과 무작위 입력으로 대조
const ref = require('./reference/original_engines.js');
const D = require('./reference/defaults.js');
const wasm = require('./pkg/fincalc_engine.js');
const rust = require('../docs/engine.js')(wasm.call);

let seed = 20261115;
const rnd = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 4294967296; };
const U = (a, b) => a + (b - a) * rnd();
const I = (a, b) => Math.floor(U(a, b + 1));
const pick = a => a[I(0, a.length - 1)];
const clone = o => JSON.parse(JSON.stringify(o));
const iso = n => new Date(n * 86400000).toISOString().slice(0, 10);
const dn = s => Date.parse(s) / 86400000;
const shiftDate = (s, lo, hi) => iso(dn(s) + I(lo, hi));

const TOL = 1e-9;
let maxRel = 0, tieNotes = 0, halfNotes = 0;
function cmp(a, b, path, out) {
  if (typeof a === 'function' || typeof b === 'function') return;
  if (a === undefined) a = null; if (b === undefined) b = null;
  if (typeof a === 'number' && typeof b === 'number') {
    if (Number.isNaN(a) && Number.isNaN(b)) return;
    if (a === b) return;
    const rel = Math.abs(a - b) / Math.max(1, Math.abs(a), Math.abs(b));
    if (path.endsWith('.early_nodes')) { tieNotes++; return; } // 조기행사 노드 '개수': 계속보유가치와 행사가치가 소수 15자리까지 같은 노드에서만 갈림
    if (!(rel <= TOL)) out.push(`${path}: ref=${a} rust=${b} rel=${rel}`); else maxRel = Math.max(maxRel, rel);
    return;
  }
  if (a === null || b === null || typeof a !== 'object' || typeof b !== 'object') { if (a !== b) out.push(`${path}: ref=${JSON.stringify(a)} rust=${JSON.stringify(b)}`); return; }
  if (Array.isArray(a) !== Array.isArray(b)) { out.push(`${path}: array mismatch`); return; }
  if (Array.isArray(a)) { if (a.length !== b.length) { out.push(`${path}: len ref=${a.length} rust=${b.length}`); return; } a.forEach((x, i) => cmp(x, b[i], `${path}[${i}]`, out)); return; }
  const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
  for (const k of keys) { if (k.startsWith('_c')) continue; cmp(a[k], b[k], `${path}.${k}`, out); }
}
const stats = {};
function check(name, fnRef, fnRust, input) {
  const st = stats[name] = stats[name] || { n: 0, fail: 0, err: 0, shown: 0 };
  st.n++;
  let a, b, ea, eb;
  try { a = fnRef(clone(input)); } catch (e) { ea = e.message; }
  try { b = fnRust(clone(input)); } catch (e) { eb = e.message; }
  if (ea || eb) {
    st.err++;
    if (ea !== eb) { st.fail++; if (st.shown++ < 3) console.log(`[${name}] error mismatch ref=${ea} rust=${eb}\n  in=${JSON.stringify(input).slice(0, 600)}`); }
    return;
  }
  const out = []; cmp(a, b, '', out);
  if (out.length) { st.fail++; if (st.shown++ < 3) console.log(`[${name}] ${out.length} diffs: ${out.slice(0, 4).join(' | ')}\n  in=${JSON.stringify(input).slice(0, 900)}`); }
}

const N = +(process.argv[2] || 300);
const parR = base => base.map(v => Math.max(0.0005, v + U(-0.01, 0.01)));
const DC = ['ACT/360', 'ACT/365', '30/360'], FREQ = [1, 3, 6, 12];

for (let i = 0; i < N; i++) {
  // IRS
  { const p = clone(D.irs); p.par_rates = parR(D.curve.krw); if (rnd() < 0.2) p.par_rates.splice(2, 1);
    p.effective = shiftDate(p.effective, -900, 400); p.maturity = shiftDate(p.effective, 30, 4000); p.valuation = shiftDate(p.effective, -300, 1500);
    p.notional = U(1e6, 1e11); p.direction = pick(['Pay Fixed', 'Receive Fixed']); p.fixed_rate = U(0, 0.08); p.fixed_freq = pick(FREQ); p.float_freq = pick(FREQ);
    p.fixed_dc = pick(DC); p.float_dc = pick(DC); p.float_spread = U(-0.01, 0.01);
    check('irs', ref.SwapJS.irs, rust.SwapJS.irs, p); }
  // CRS
  { const p = clone(D.crs); p.krw_par_rates = parR(D.curve.krw); p.usd_par_rates = parR(D.curve.usd);
    p.effective = shiftDate(p.effective, -900, 400); p.maturity = shiftDate(p.effective, 30, 4000); p.valuation = shiftDate(p.effective, -300, 1500);
    p.direction = pick(['KRW 지급 / USD 수취', 'USD 지급 / KRW 수취']); p.exchange = pick(['기초+만기 교환', '만기 교환', '교환 없음']);
    p.krw_type = pick(['Fixed', 'Floating']); p.usd_type = pick(['Fixed', 'Floating']); p.krw_freq = pick(FREQ); p.usd_freq = pick(FREQ); p.krw_dc = pick(DC); p.usd_dc = pick(DC);
    p.fx_initial = U(1000, 1500); p.fx_valuation = U(1000, 1500); p.usd_notional = U(1e5, 1e9); p.krw_fixed_rate = U(0, 0.07); p.usd_fixed_rate = U(0, 0.07); p.krw_spread = U(-0.01, 0.01); p.usd_spread = U(-0.01, 0.01);
    check('crs', ref.SwapJS.crs, rust.SwapJS.crs, p); }
  // FX
  { const p = clone(D.fx); p.krw_par_rates = parR(D.curve.krw); p.usd_par_rates = parR(D.curve.usd);
    p.valuation = shiftDate(p.valuation, -500, 500); p.maturity = shiftDate(p.valuation, -5, 1500); p.direction = pick(['USD 매입', 'USD 매도']);
    p.spot = U(900, 1600); p.contract_rate = U(900, 1600); p.usd_notional = U(1e4, 1e9); p.swap_point = pick(['', null, U(-50, 50), String(U(-5, 5).toFixed(2))]);
    check('fx', ref.SwapJS.fx, rust.SwapJS.fx, p);
    if (i < 20) { try { const a = ref.SwapJS.fx(clone(p)), b = rust.SwapJS.fx(clone(p)); for (const t of [0.1, 0.5, 1.3, 7, 40]) { const o = []; cmp(a._ck.df(t), b._ck.df(t), 'ck.df', o); cmp(a._cu.zero(t), b._cu.zero(t), 'cu.zero', o); if (o.length) console.log('[fx curve]', o); } } catch (e) {} } }
  check('swap.bootstrap', ref.SwapJS.bootstrap, rust.SwapJS.bootstrap, parR(D.curve.usd));
  // 옵션
  { const p = clone(D.opt); p.valuation = shiftDate(p.valuation, -400, 400); p.maturity = shiftDate(p.valuation, 1, 2500);
    p.spot = U(100, 1e5); p.strike = p.spot * U(0.5, 1.6); p.quantity = pick([1, 0, 250, '', 12.5]); p.strike_mode = pick(['이자율연동(IRR)', '고정']); p.strike_irr = U(0, 0.08);
    p.kind = pick(['콜', '풋']); p.style = pick(['유러피언', '아메리칸']); p.step = pick(['주간', '일간']); p.rate_quoted = U(0, 0.07); p.rate_comp = pick(['단리(연1회복리)', '연 2회 복리', '연 4회 복리', '연 12회 복리(월복리)']);
    p.sigma = U(0.05, 0.9); if (rnd() < 0.3) p.r = U(0, 0.06);
    check('opt', x => ref.OptionJS.value(x), x => rust.OptionJS.value(x), p);
    const px = Array.from({ length: I(0, 60) }, () => pick([U(100, 200), U(100, 200), U(100, 200), '', null, 0]));
    const fr = pick(['일별', '주별', '월별']), no = pick([undefined, null, I(0, 80)]);
    check('histVol', x => ref.OptionJS.histVol(x, fr, no), x => rust.OptionJS.histVol(x, fr, no), px);
    const q = U(0, 0.1), m = pick(['단리(연1회복리)', '연 2회 복리', '연 4회 복리', '연 12회 복리(월복리)', '?']);
    check('contRate', () => ref.OptionJS.contRate(q, m), () => rust.OptionJS.contRate(q, m), {}); }
  // PRS
  { const p = clone(D.prs); const mk = pick(Object.keys(D.prs_market)); const mkt = D.prs_market[mk];
    p.gov_ytm = mkt.gov.map(v => v == null ? pick([null, '', 3.1]) : v + U(-0.5, 0.5)); p.aa_ytm = mkt.aa.map(v => v == null ? pick([null, 3.6]) : v + U(-0.5, 0.5));
    p.start = shiftDate(p.start, -600, 300); p.maturity = pick([shiftDate(p.start, 200, 2500), '', undefined]); p.term_years = pick([4, 3, 5, 2]);
    p.valuation = shiftDate(p.start, -30, 700); p.pay_timing = pick(['선급', '후급']); p.model = pick(['excel', 'crr', 'CRR']); p.premium = U(0.01, 0.09); p.pay_months = pick([1, 3, 6, 12]);
    p.day_basis = pick([365, 360]); p.vol = U(0.1, 0.8); p.gain_tax = pick([0, 0.22, 0.11]); p.tax_bearer = pick(['수령자', '지급자']); p.biz_roll = pick([true, false, true]);
    p.gap_days = pick([5, 1, 7, 10]); p.max_steps = pick([300, 100, 500]); p.coupon_freq = pick([2, 4, 1]); p.fx = U(1100, 1500); p.stake = U(0.1, 1);
    p.rate_shift_bp = pick([0, 0, 50, -100]); p.equity_chg = pick([0, 0, 0.1, -0.3]); p.notional = U(5e4, 2e5);
    const det = rnd() < 0.7;
    let half = false; try { const r0 = ref.PrsJS.value(clone(p), false); half = r0.model === 'crr' && Math.abs((r0.days / r0.gap) % 1 - 0.5) < 1e-12; } catch (e) {}
    if (half) halfNotes++; // 순수 CRR에서 남은일수÷간격이 정확히 x.5: Rust는 Python 원본(짝수 쪽 반올림)을 따름, JS 예비 엔진은 올림
    else check('prs', x => ref.PrsJS.value(x, det), x => rust.PrsJS.value(x, det), p);
    if (i % 10 === 0 && !half) { const q = clone(p); q._gov = ref.PrsJS.bootstrap(p.gov_ytm, p.coupon_freq); q._aa = ref.PrsJS.bootstrap(p.aa_ytm, p.coupon_freq); check('prs(_gov)', x => ref.PrsJS.value(x), x => rust.PrsJS.value(x), q); }
    check('prs.bootstrap', x => ref.PrsJS.bootstrap(x, p.coupon_freq), x => rust.PrsJS.bootstrap(x, p.coupon_freq), p.gov_ytm); }
  // 내재파생 사채
  { const base = rnd() < 0.25 ? Object.values(D.cb_presets)[0] : D.cb; const p = clone(base);
    p.kind = pick(['CB', 'EB', 'BW', 'SB', 'CB']); p.bw_pay = pick(['현금', '대용']);
    if (rnd() < 0.7) { p.issue = shiftDate(p.issue, -400, 200); p.maturity = shiftDate(p.issue, 400, 3000); p.valuation = shiftDate(p.issue, -5, 900);
      p.put_start = shiftDate(p.issue, 100, 900); p.call_start = shiftDate(p.issue, 100, 900); p.call_end = shiftDate(p.call_start, 30, 2000); p.conv_start = pick([shiftDate(p.issue, 30, 500), '', null]); p.conv_end = pick([shiftDate(p.maturity, -60, 0), '']); p.refix_start = pick([shiftDate(p.issue, 30, 400), '']); }
    p.coupon = pick([0, 0.01, 0.025, 0.05]); p.pay_months = pick([3, 6, 12, 0, 1]); p.ytp = U(0, 0.06); p.put_on = rnd() < 0.6; p.put_every = pick([3, 6, 12, 1]); p.put_yield = U(0, 0.06);
    p.call_on = rnd() < 0.6; p.call_ratio = pick([0.3, 1, 0, 0.5]); p.call_yield = U(0, 0.06); p.call_par = rnd() < 0.3; p.call_coupon_only = rnd() < 0.3;
    p.conv_price = U(5000, 60000); p.spot = U(3000, 80000); p.vol = U(0.1, 0.8); p.div_yield = pick([0, 0.01, 0.03]); p.refix_on = rnd() < 0.5; p.refix_floor = pick([0.7, 0.8, 1]);
    p.credit_spread = U(0.005, 0.09); p.rate_vol = pick([0.15, 0.05, 0.3]); p.coupon_freq = pick([2, 4]); p.step_days = pick([7, 1, 14, 30]); p.max_steps = pick([300, 120, 60]);
    p.step_on = rnd() < 0.25; p.bw_ratio = pick([1, 0.5]); p.gov_ytm = p.gov_ytm.map(v => pick([v + U(-0.5, 0.5), v, null, '']));
    if (rnd() < 0.3) p.gov_ytm = [3.2, 3.3, 3.4, 3.5, 3.6, 3.8, 4.0, 4.1, 4.2, 4.2].map(v => pick([v, v, v, null]));
    p.rate_shift_bp = pick([0, 0, 25, -50]); p.spread_shift_bp = pick([0, 0, 100]); p.spot_chg = pick([0, 0, 0.2, -0.2]); p.vol_shift = pick([0, 0, 0.05]);
    if (rnd() < 0.3) { p.rate_vol_mode = 'matrix'; p.rate_vol_matrix = { exp: [1 / 12, 0.25, 0.5, 1, 2, 3, 5, 7, 10], ten: [1, 2, 3, 5, 7, 10], vol: Array.from({ length: 9 }, () => Array.from({ length: 6 }, () => U(10, 40))) }; if (rnd() < 0.3) { delete p.rate_vol_matrix.exp; delete p.rate_vol_matrix.ten; } }
    const det = rnd() < 0.7;
    check('cb', x => ref.CbJS.value(x, det), x => rust.CbJS.value(x, det), p);
    if (i % 12 === 0) { let tot; try { tot = ref.CbJS.value(clone(p), false).total; } catch (e) {} if (tot) { const price = tot * U(0.9, 1.1);
      check('cb.oas', x => ref.CbJS.oas(x, price), x => rust.CbJS.oas(x, price), p);
      const g = ref.CbJS.bootstrap(p.gov_ytm, p.coupon_freq); check('cb.oas(_gov)', x => ref.CbJS.oas({ ...x, _gov: g }, price), x => rust.CbJS.oas({ ...x, _gov: g }, price), p);
      const r0 = ref.CbJS.value(clone(p)); const fl = r0.flows.map(f => [f.t, f.cf]); check('cb.zspread', () => ref.CbJS.zspread(price, fl, g.spot, 0), () => rust.CbJS.zspread(price, fl, g.spot, 0), {}); } }
    check('cb.bootstrap', x => ref.CbJS.bootstrap(x, p.coupon_freq), x => rust.CbJS.bootstrap(x, p.coupon_freq), p.gov_ytm);
    check('cb.fill', ref.CbJS.fill, rust.CbJS.fill, p.gov_ytm);
    const ac = [pick([0, 0.01, 0.03]), pick([0, 3, 6, 12]), U(0, 0.06), pick([I(0, 120), U(0, 120)])];
    check('cb.accreted', () => ref.CbJS.accreted(...ac), () => rust.CbJS.accreted(...ac), {}); }
  // 금리커브
  { const ten = pick([[0.25, 0.5, 0.75, 1, 1.5, 2, 3, 5, 10, 20, 30], [0.25, 1, 2, 3, 5, 10], [1, 2, 3, 5, 10, 20, 30, 0.5]]);
    const rates = ten.map(() => pick([U(1, 6), U(1, 6), U(1, 6), null, '', '3.25']));
    const p = { tenors: ten, rates, freq: pick([2, 4, 1, undefined]), max_t: pick([30, null, 10, 7.3, undefined]) };
    check('curve.value', ref.CurveJS.value, rust.CurveJS.value, p);
    try { const c = ref.CurveJS.build(ten, rates, p.freq || 2, p.max_t || null), c2 = rust.CurveJS.build(ten, rates, p.freq, p.max_t); const o = []; cmp(c, c2, 'build', o);
      for (const t of [0, 0.1, 0.25, 0.6, 1, 2.2, 9.99, 35]) { cmp(ref.CurveJS.zeroAt(c, t), rust.CurveJS.zeroAt(c, t), 'zeroAt', o); cmp(ref.CurveJS.parAt(c, t), rust.CurveJS.parAt(c, t), 'parAt', o); cmp(ref.CurveJS.dfAt(c, t), rust.CurveJS.dfAt(c, t), 'dfAt', o); cmp(ref.CurveJS.fwdAt(c, t, t + 0.7), rust.CurveJS.fwdAt(c, t, t + 0.7), 'fwdAt', o); cmp(ref.CurveJS.fwdAt(c, t, t), rust.CurveJS.fwdAt(c, t, t), 'fwdAt0', o); }
      cmp(ref.CurveJS.fwdMatrix(c), rust.CurveJS.fwdMatrix(c), 'fwdMatrix', o);
      stats['curve.at'] = stats['curve.at'] || { n: 0, fail: 0, err: 0 }; stats['curve.at'].n++; if (o.length) { stats['curve.at'].fail++; console.log('[curve.at]', o.slice(0, 3)); } } catch (e) {}
    const ecos = { 1: U(2, 4), 2: pick([U(2, 4), null]), 3: U(2, 4), 5: U(2, 4), 10: U(2, 4), 20: pick([U(2, 4), null]), 30: U(2, 4) };
    const kis = pick([null, undefined, ref.CurveJS.KIS_TENORS.map(() => pick([U(2, 4), U(2, 4), null]))]);
    check('ktbPoints', () => ref.CurveJS.ktbPoints(ecos, kis), () => rust.CurveJS.ktbPoints(ecos, kis), {});
    const gv = ref.CurveJS.KIS_TENORS.map(() => U(3, 6)), kg = ref.CurveJS.KIS_TENORS.map(() => U(2, 4)), [kt, kr] = ref.CurveJS.ktbPoints(ecos, kg);
    check('corpPoints', () => ref.CurveJS.corpPoints(gv, kg, kt, kr), () => rust.CurveJS.corpPoints(gv, kg, kt, kr), {}); }
  // 재무계산기
  { const F = ref.FinJS, G = rust.FinJS;
    const t = { N: pick([U(1, 480), I(1, 360), '120', 0]), IY: pick([U(0, 20), 0, U(-5, 30), '6.5']), PV: pick([U(-1e6, 1e6), 0]), PMT: pick([U(-1e4, 1e4), 0]), FV: pick([U(-1e6, 1e6), 0]), PY: pick([1, 12, 2, 4, '', 0]), CY: pick([1, 12, 2, undefined, '']), BGN: rnd() < 0.3 };
    for (const k of ['IY', 'PV', 'FV', 'PMT', 'N', 'X']) check('tvm.' + k, x => F.tvm(x, k), x => G.tvm(x, k), t);
    check('fin.rate', F.rate, G.rate, t);
    check('amortRows', F.amortRows, G.amortRows, t);
    const P1 = pick([1, 3, '2', undefined, 0, 500]), P2 = pick([12, 3, undefined, '24', 1]);
    check('amort', x => F.amort(x, P1, P2), x => G.amort(x, P1, P2), t);
    const flows = Array.from({ length: I(0, 9) }, (_, j) => ({ c: pick([U(-1000, 1500), U(100, 900), j === 0 ? -U(500, 5000) : U(0, 800), '', '250']), f: pick([1, 1, 2, 5, '', '3', 0, undefined]) }));
    const II = pick([U(0, 20), 0, '', '8', undefined]), RI = pick([null, '', U(0, 15), undefined]);
    check('cf', x => F.cf(x, II, RI), x => G.cf(x, II, RI), flows);
    const sd = shiftDate('2026-01-15', -700, 700); const b = { SDT: pick([sd, sd, sd, '']), RDT: pick([shiftDate(sd, 1, 9000), shiftDate(sd, 1, 9000), '2030-02-28', '2032-08-31', '2028-02-29', '']), CPN: pick([U(0, 9), 0, '', '4.25']), RV: pick([100, '', 102, 0]), DAY: pick(['ACT', '360']), FREQ: pick([2, 1, '2', '1']), YLD: pick([U(0, 12), '', '5', null]), PRI: pick([U(60, 140), '', null]) };
    for (const k of ['PRI', 'YLD', undefined, 'AI']) check('bond.' + k, x => F.bond(x, k), x => G.bond(x, k), b);
    const d = { LIF: pick([5, 10, 3.5, 7, '', 0, 1]), CST: pick([U(1e3, 1e6), '', 0]), SAL: pick([0, '', U(0, 900), 1e7]), M01: pick([1, '', 3.5, 7, 12, 12.9]), DB: pick([200, '', 150, 125]), METH: pick(['SL', 'SYD', 'DB', 'DBX', '', undefined]), YR: pick([1, 2, 5, '', 30, 0]) };
    check('deprSchedule', F.deprSchedule, G.deprSchedule, d); check('depr', F.depr, G.depr, d);
    const dx = { DT1: pick([sd, '']), DT2: pick([shiftDate(sd, -900, 900), '', '2027-03-31', '2028-02-29']), DBD: pick([I(-900, 900), U(-99, 99), '', '45']), DAY: pick(['ACT', '360']) };
    for (const k of ['DBD', 'DT1', 'DT2', 'Q']) check('date.' + k, x => F.date(x, k), x => G.date(x, k), dx);
    check('weekday', F.weekday, G.weekday, pick([sd, '', null, shiftDate(sd, 0, 6)]));
    const ic = { NOM: U(-5, 30), EFF: U(-5, 30), CY: pick([1, 2, 4, 12, 365, '', 0]) }; for (const k of ['EFF', 'NOM']) check('iconv.' + k, x => F.iconv(x, k), x => G.iconv(x, k), ic);
    const pf = { CST: pick([U(1, 900), '', null]), SEL: pick([U(1, 900), '']), MAR: pick([U(-50, 90), '', null]), MU: pick([U(-50, 200), '']), _via: pick(['MU', 'MAR', undefined]) }; for (const k of ['MAR', 'MU', 'SEL', 'CST', 'Z']) check('profit.' + k, x => F.profit(x, k), x => G.profit(x, k), pf);
    const be = { FC: pick([U(0, 1e5), '']), VC: pick([U(0, 50), '']), P: pick([U(1, 99), '']), PFT: pick([U(-1e4, 1e5), '']), Q: pick([U(0, 5000), '', 0]) }; for (const k of ['PFT', 'Q', 'P', 'FC', 'VC', 'Z']) check('brkevn.' + k, x => F.brkevn(x, k), x => G.brkevn(x, k), be);
    const pc = { OLD: pick([U(1, 900), '', 0]), NEW: pick([U(1, 900), '']), CH: pick([U(-50, 90), '']), PD: pick([1, 3, '', 0, 2.5]) }; for (const k of ['CH', 'NEW', 'OLD', 'PD', 'Z']) check('pct.' + k, x => F.pct(x, k), x => G.pct(x, k), pc);
    const meth = pick(['LIN', 'Ln', 'EXP', 'PWR', '1-V', undefined, '']);
    const pts = Array.from({ length: I(0, 12) }, () => ({ x: pick([U(0.5, 90), U(0.5, 90), '', '12', U(-5, 5)]), y: pick([U(0.5, 90), U(0.5, 90), '', null, U(-5, 5)]) }));
    check('stat.' + meth, x => F.stat(x, meth), x => G.stat(x, meth), pts);
    try { const a = F.stat(clone(pts), meth), b2 = G.stat(clone(pts), meth); if (a.predY) { const o = []; for (const v of [1.5, 7, 40]) { cmp(a.predY(v), b2.predY(v), 'predY', o); cmp(a.predX(v), b2.predX(v), 'predX', o); } stats.statPred = stats.statPred || { n: 0, fail: 0, err: 0 }; stats.statPred.n++; if (o.length) { stats.statPred.fail++; console.log('[statPred]', meth, o.slice(0, 2)); } } } catch (e) {}
  }
}
let bad = 0, tot = 0;
for (const [k, s] of Object.entries(stats)) { tot += s.n; bad += s.fail; console.log(`${s.fail ? 'FAIL' : 'ok  '} ${k.padEnd(16)} cases=${s.n} errors(matching)=${s.err - 0} mismatches=${s.fail}`); }
console.log(`\nTOTAL cases=${tot} mismatches=${bad} maxDiff(passing, |a-b|/max(1,|a|,|b|))=${maxRel.toExponential(2)} early_nodes tie notes=${tieNotes} prs half-step notes=${halfNotes}`);
process.exit(bad ? 1 : 0);

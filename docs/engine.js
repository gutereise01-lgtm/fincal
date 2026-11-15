/* 재무계산기 계산 엔진 연결부.
   모든 계산은 Rust(WebAssembly) 엔진의 call(이름, 인자) 로 넘기고, 화면 코드가 쓰던 이름(SwapJS·OptionJS·PrsJS·CbJS·CurveJS·FinJS)
   그대로 결과를 돌려준다. 이 파일에는 계산식이 없다. */
function makeEngines(rawCall){
  const enc = (k, v) => (typeof v === 'number' && !isFinite(v)) ? (v !== v ? '__NaN__' : v > 0 ? '__Inf__' : '__-Inf__') : (typeof v === 'function' ? undefined : v);
  const dec = (k, v) => v === '__NaN__' ? NaN : v === '__Inf__' ? Infinity : v === '__-Inf__' ? -Infinity : v;
  function call(name, ...args){
    const r = JSON.parse(rawCall(name, JSON.stringify(args, enc)), dec);
    if (r.err !== undefined) throw new Error(r.err);
    return r.ok;
  }
  const curveFns = pts => ({ zero: t => call('swap.curve', pts, [t]).zero[0], df: t => call('swap.curve', pts, [t]).df[0] });
  const SwapJS = {
    irs: p => call('swap.irs', p), crs: p => call('swap.crs', p),
    fx: p => { const r = call('swap.fx', p); r._ck = curveFns(r._ck_pts); r._cu = curveFns(r._cu_pts); return r; },
    bootstrap: par => call('swap.bootstrap', par),
  };
  const OptionJS = {
    value: (p, preview) => call('opt.value', p, preview),
    histVol: (prices, freq, nObs) => call('opt.histVol', prices, freq, nObs),
    contRate: (q, m) => call('opt.contRate', q, m),
  };
  const PrsJS = {
    value: (p, detail) => call('prs.value', p, detail),
    bootstrap: (ytm, freq) => call('prs.bootstrap', ytm, freq),
    TEN: [0.25,0.5,0.75,1,1.5,2,2.5,3,4,5,7,10],
  };
  const CbJS = {
    value: (p, detail) => call('cb.value', p, detail),
    bootstrap: (ytm, freq) => call('cb.bootstrap', ytm, freq),
    fill: v => call('cb.fill', v),
    accreted: (c, pm, y, months) => call('cb.accreted', c, pm, y, months),
    oas: (p, price, lo, hi) => call('cb.oas', p, price, lo, hi),
    zspread: (price, flows, spot, shift) => call('cb.zspread', price, flows, spot, shift),
    TEN: [0.25,0.5,0.75,1,2,3,5,10,20,30],
  };
  const CurveJS = {
    build: (tenors, rates, freq, maxT) => call('curve.build', tenors, rates, freq, maxT),
    value: p => call('curve.value', p),
    zeroAt: (c, t) => call('curve.interp', t, c.t, c.zero),
    dfAt: (c, t) => call('curve.dfAt', c.t, c.zero, t),
    fwdAt: (c, t1, t2) => call('curve.fwdAt', c.t, c.zero, t1, t2),
    parAt: (c, t) => call('curve.interp', t, c.t, c.par),
    fwdMatrix: c => call('curve.fwdMatrix', c.t, c.zero),
    ktbPoints: (ecos, kisGov) => call('curve.ktbPoints', ecos, kisGov),
    corpPoints: (gv, kisGov, kt, kr) => call('curve.corpPoints', gv, kisGov, kt, kr),
    interp: (x, xs, ys) => call('curve.interp', x, xs, ys),
    GRADES: ['AAA','AA+','AA','AA-','A+','A','A-','BBB+','BBB','BBB-'],
    KIS_TENORS: [0.25,0.5,0.75,1,1.5,2,3,5],
    FWD_START: [0,0.25,0.5,0.75,1,2,3,5,7,10], FWD_TENOR: [0.25,0.5,0.75,1,2,3,5],
  };
  const FinJS = {
    tvm: (t, key) => call('fin.tvm', t, key), rate: t => call('fin.rate', t),
    amort: (t, P1, P2) => call('fin.amort', t, P1, P2), amortRows: t => call('fin.amortRows', t),
    cf: (flows, I, RI) => call('fin.cf', flows, I, RI),
    bond: (b, key) => call('fin.bond', b, key),
    deprSchedule: d => call('fin.deprSchedule', d), depr: d => call('fin.depr', d),
    date: (x, key) => { const r = call('fin.date', x, key); return r === null ? undefined : r; },
    weekday: s => call('fin.weekday', s),
    iconv: (x, key) => call('fin.iconv', x, key),
    profit: (x, key) => { const r = call('fin.profit', x, key); return r === null ? undefined : r; },
    brkevn: (x, key) => { const r = call('fin.brkevn', x, key); return r === null ? undefined : r; },
    pct: (x, key) => { const r = call('fin.pct', x, key); return r === null ? undefined : r; },
    stat: (pts, meth) => { const o = call('fin.stat', pts, meth);
      if (o.b !== undefined){ o.predY = x => call('fin.statPred', o.meth, o.a, o.b, x, 'Y'); o.predX = y => call('fin.statPred', o.meth, o.a, o.b, y, 'X'); }
      return o; },
  };
  return { SwapJS, OptionJS, PrsJS, CbJS, CurveJS, FinJS, version: () => call('version') };
}
if (typeof module !== 'undefined') module.exports = makeEngines;

/* 시작 순서: Rust 엔진(WebAssembly) 준비 → 화면 코드(app.js) 실행 → 오프라인용 서비스 워커 등록 */
(async () => {
  const fail = msg => { const d = document.createElement('div'); d.className = 'boot-err'; d.textContent = msg; document.body.prepend(d); const t = document.getElementById('engineText'); if (t) t.textContent = '엔진을 불러오지 못했습니다'; };
  try {
    if (typeof WebAssembly !== 'object') throw new Error('이 브라우저는 WebAssembly를 지원하지 않습니다.');
    await wasm_bindgen({ module_or_path: 'pkg/fincalc_engine_bg.wasm' });
    const E = makeEngines(wasm_bindgen.call);
    Object.assign(window, { SwapJS: E.SwapJS, OptionJS: E.OptionJS, PrsJS: E.PrsJS, CbJS: E.CbJS, CurveJS: E.CurveJS, FinJS: E.FinJS, ENGINE_VERSION: 'v' + E.version() });
    await new Promise((ok, no) => { const s = document.createElement('script'); s.src = 'app.js'; s.onload = ok; s.onerror = () => no(new Error('화면 코드(app.js)를 불러오지 못했습니다.')); document.body.appendChild(s); });
  } catch (e) {
    console.error(e);
    fail('계산 엔진을 시작하지 못했습니다. 인터넷에 연결한 뒤 다시 열어 주세요. (' + ((e && e.message) || e) + ')');
  }
  if ('serviceWorker' in navigator && (location.protocol === 'https:' || location.hostname === 'localhost' || location.hostname === '127.0.0.1')) {
    try {
      const reg = await navigator.serviceWorker.register('sw.js');
      const offer = w => {
        if (!w || !navigator.serviceWorker.controller || document.getElementById('upd-bar')) return;
        const bar = document.createElement('div'); bar.className = 'upd-bar'; bar.id = 'upd-bar';
        bar.innerHTML = '<span>새 버전이 준비되었습니다.</span><button type="button">지금 적용</button>';
        bar.querySelector('button').addEventListener('click', () => w.postMessage('skipWaiting'));
        document.body.appendChild(bar);
      };
      if (reg.waiting) offer(reg.waiting);
      reg.addEventListener('updatefound', () => { const w = reg.installing; if (w) w.addEventListener('statechange', () => { if (w.state === 'installed') offer(w); }); });
      let reloaded = false;
      navigator.serviceWorker.addEventListener('controllerchange', () => { if (reloaded) return; reloaded = true; location.reload(); });
      document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') reg.update().catch(() => {}); });
    } catch (e) { console.log('service worker', e); }
  }
})();

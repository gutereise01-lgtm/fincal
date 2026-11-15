/* 아이폰 홈 화면 앱용 연결부.
   원본은 Claude 아티팩트의 저장소·내려받기 기능(window.claude)을 썼다. 여기서는
   · 평가 저장: 이 기기 저장소(localStorage) — 앱 코드에 원래 있던 '이 기기에만 저장' 방식 그대로
   · 파일 내려받기(엑셀·평가 파일): 아이폰 공유 시트(파일에 저장 등) 또는 일반 내려받기 */
(() => {
  const MIME = { xlsx: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet', json: 'application/json', csv: 'text/csv' };
  async function save({ filename, data }) {
    const ext = (filename.split('.').pop() || '').toLowerCase();
    const blob = data instanceof Blob ? (data.type ? data : new Blob([data], { type: MIME[ext] || 'application/octet-stream' })) : new Blob([data], { type: MIME[ext] || 'application/octet-stream' });
    // 아이폰: 공유 시트로 넘기면 '파일에 저장'·메일·메신저로 바로 보낼 수 있다
    const touch = navigator.maxTouchPoints > 0 && /iPhone|iPad|iPod|Macintosh|Android/.test(navigator.userAgent);
    if (touch && navigator.canShare) {
      try {
        const file = new File([blob], filename, { type: blob.type });
        if (navigator.canShare({ files: [file] })) { await navigator.share({ files: [file] }); return; }
      } catch (e) {
        if (e && e.name === 'AbortError') { const err = new Error('declined'); err.code = 'declined'; throw err; }
        // NotAllowedError 등: 아래 일반 내려받기로 넘어감
      }
    }
    const url = URL.createObjectURL(blob), a = document.createElement('a');
    a.href = url; a.download = filename; a.rel = 'noopener'; a.style.display = 'none';
    document.body.appendChild(a); a.click();
    setTimeout(() => { a.remove(); URL.revokeObjectURL(url); }, 60000);
  }
  window.claude = { use: async cap => cap === 'downloads' ? { save } : null };
  // 저장한 평가가 iOS 저장공간 정리로 지워지지 않도록 요청 (지원하는 경우)
  try { if (navigator.storage && navigator.storage.persist) navigator.storage.persist().catch(() => {}); } catch (e) {}
})();

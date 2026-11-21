/* CW570 계산기 오프라인 실행용 서비스 워커 */
const VERSION='59e56483c863';
const CACHE='cw570-'+VERSION;
const SHELL=['./','index.html','manifest.webmanifest','icon-192.png','icon-512.png','icon-180.png'];
self.addEventListener('install',e=>{e.waitUntil(caches.open(CACHE).then(c=>c.addAll(SHELL.map(u=>new Request(u,{cache:'reload'})))).then(()=>self.skipWaiting()));});
self.addEventListener('activate',e=>{e.waitUntil(caches.keys().then(ks=>Promise.all(ks.filter(k=>k.startsWith('cw570-')&&k!==CACHE).map(k=>caches.delete(k)))).then(()=>self.clients.claim()));});
self.addEventListener('fetch',e=>{
  const req=e.request;if(req.method!=='GET')return;
  const url=new URL(req.url);if(url.origin!==location.origin)return;
  e.respondWith((async()=>{
    const c=await caches.open(CACHE);
    // 네트워크 우선(새 버전 바로 반영), 오프라인이면 캐시
    try{const res=await fetch(req);if(res.ok)c.put(req,res.clone());return res;}
    catch(err){return (await c.match(req,{ignoreSearch:true}))||(req.mode==='navigate'?await c.match('index.html'):null)||Response.error();}
  })());
});

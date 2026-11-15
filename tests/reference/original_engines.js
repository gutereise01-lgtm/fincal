// swap_engine.py 의 1:1 JS 사본 (Pyodide를 쓸 수 없는 환경용 예비 엔진)
const SwapJS = (() => {
  const PAR_TENORS = [0.25,0.5,0.75,1,2,3,4,5,7,10,15,20,30], PAR_TENORS_V1 = [0.25,0.5,1,2,3,4,5,7,10,15,20,30];
  const CURVE_TENORS = [0.25,0.5,0.75,1,2,3,5,7,10,15,20,30];
  const DAY = 86400000;
  const parse = s => { const [y,m,d] = s.slice(0,10).split('-').map(Number); return {y,m,d,n:Date.UTC(y,m-1,d)/DAY}; };
  const mk = (y,m,d) => ({y,m,d,n:Date.UTC(y,m-1,d)/DAY});
  const iso = a => `${a.y}-${String(a.m).padStart(2,'0')}-${String(a.d).padStart(2,'0')}`;
  const mdays = (y,m) => new Date(Date.UTC(y,m,0)).getUTCDate();
  const edate = (a,k) => { let t = a.m-1+k; const y = a.y+Math.floor(t/12), m = ((t%12)+12)%12+1; return mk(y,m,Math.min(a.d,mdays(y,m))); };
  function days360(a,b){
    let d1=a.d, y2=b.y, m2=b.m, d2=b.d;
    if (d1===31 || (a.m===2 && d1===mdays(a.y,2))) d1=30;
    if (d2===31){ if(d1<30){ d2=1; m2+=1; if(m2>12){m2=1;y2+=1;} } else d2=30; }
    return (y2-a.y)*360+(m2-a.m)*30+(d2-d1);
  }
  const yf = (a,b,dc) => dc==='30/360' ? days360(a,b)/360 : dc==='ACT/360' ? (b.n-a.n)/360 : (b.n-a.n)/365;
  function interp(x,xs,ys){
    if (x<=xs[0]) return ys[0];
    if (x>=xs[xs.length-1]) return ys[ys.length-1];
    for (let i=0;i<xs.length-1;i++) if (xs[i]<=x && x<xs[i+1]) return ys[i]+(ys[i+1]-ys[i])*(x-xs[i])/(xs[i+1]-xs[i]);
    return ys[ys.length-1];
  }
  function bootstrap(par){
    const PT = par.length===PAR_TENORS_V1.length ? PAR_TENORS_V1 : PAR_TENORS;
    const yearly=[]; let cum=0;
    for (let n=1;n<=30;n++){ const p=interp(n,PT,par); const df=(1-p*cum)/(1+p); const z=Math.pow(df,-1/n)-1;
      yearly.push({year:n,par:p,cum_df:cum,df,zero:z}); cum+=df; }
    const zbt={}; PT.forEach((t,i)=>{ zbt[t]= t<1 ? par[i] : yearly[t-1].zero; });
    return {yearly, zero_by_tenor:zbt, curve:CURVE_TENORS.filter(t=>t in zbt).map(t=>[t,zbt[t]])};
  }
  const curve = c => { const xs=c.map(p=>p[0]), ys=c.map(p=>p[1]); const zero=t=>interp(t,xs,ys); return {zero, df:t=>Math.pow(1+zero(t),-t)}; };
  function schedule(eff,mat,months){ const r=[]; let s=eff; for(;;){ let e=edate(s,months); if(e.n>=mat.n) e=mat; r.push([s,e]); if(e.n>=mat.n) break; s=e; } return r; }

  function irs(p){
    const eff=parse(p.effective), mat=parse(p.maturity), val=parse(p.valuation), N=+p.notional;
    const boot=bootstrap(p.par_rates), cv=curve(boot.curve);
    const fixed=schedule(eff,mat,+p.fixed_freq).map(([s,e],i)=>{ const dcf=yf(s,e,p.fixed_dc), T=(e.n-val.n)/365, z=cv.zero(T), df=Math.pow(1+z,-T);
      const cf = e.n<=val.n?0:N*p.fixed_rate*dcf; return {no:i+1,start:iso(s),end:iso(e),days:e.n-s.n,dcf,t:T,zero:z,df,cf,pv:cf*df}; });
    const floating=schedule(eff,mat,+p.float_freq).map(([s,e],i)=>{ const dcf=yf(s,e,p.float_dc), Ts=(s.n-val.n)/365, Te=(e.n-val.n)/365;
      const dfs=Math.pow(1+cv.zero(Ts),-Ts), ze=cv.zero(Te), dfe=Math.pow(1+ze,-Te);
      const fwd=(e.n<=val.n||!dcf)?0:(dfs/dfe-1)/dcf, cf=e.n<=val.n?0:N*(fwd+p.float_spread)*dcf;
      return {no:i+1,start:iso(s),end:iso(e),days:e.n-s.n,dcf,t:Te,zero:ze,df:dfe,fwd,cf,pv:cf*dfe}; });
    const pv_fixed=fixed.reduce((a,r)=>a+r.pv,0), pv_float=floating.reduce((a,r)=>a+r.pv,0);
    const npv = p.direction==='Receive Fixed' ? pv_fixed-pv_float : pv_float-pv_fixed;
    return {pv_fixed,pv_float,npv,par_rate:pv_fixed?p.fixed_rate*pv_float/pv_fixed:0,fixed,floating,boot};
  }
  function leg(eff,mat,val,months,dc,N,kind,fr,sp,cv){
    return schedule(eff,mat,months).map(([s,e],i)=>{ const dcf=yf(s,e,dc), Ts=(s.n-val.n)/365, Te=(e.n-val.n)/365;
      const dfs=cv.df(Ts), dfe=cv.df(Te), alive=e.n>val.n, fwd=(alive&&dcf)?(dfs/dfe-1)/dcf:0;
      const cf = !alive?0 : kind==='Fixed' ? N*fr*dcf : N*(fwd+sp)*dcf;
      return {no:i+1,start:iso(s),end:iso(e),days:e.n-s.n,dcf,t:Te,zero:cv.zero(Te),df:dfe,fwd,cf,pv:cf*dfe}; });
  }
  function crs(p){
    const eff=parse(p.effective), mat=parse(p.maturity), val=parse(p.valuation);
    const usdN=+p.usd_notional, krwN=usdN*p.fx_initial;
    const bk=bootstrap(p.krw_par_rates), bu=bootstrap(p.usd_par_rates), ck=curve(bk.curve), cu=curve(bu.curve);
    const krw=leg(eff,mat,val,+p.krw_freq,p.krw_dc,krwN,p.krw_type,p.krw_fixed_rate,p.krw_spread,ck);
    const usd=leg(eff,mat,val,+p.usd_freq,p.usd_dc,usdN,p.usd_type,p.usd_fixed_rate,p.usd_spread,cu);
    const ix=p.exchange==='기초+만기 교환'?1:0, mx=p.exchange==='교환 없음'?0:1;
    const t0=(eff.n-val.n)/365, t1=(mat.n-val.n)/365;
    const df={krw_eff:ck.df(t0),krw_mat:ck.df(t1),usd_eff:cu.df(t0),usd_mat:cu.df(t1)};
    const kc=krw.reduce((a,r)=>a+r.pv,0), uc=usd.reduce((a,r)=>a+r.pv,0);
    const kp=krwN*(mx*df.krw_mat-ix*df.krw_eff), up=usdN*(mx*df.usd_mat-ix*df.usd_eff);
    const kt=kc+kp, ut=uc+up, utk=ut*p.fx_valuation;
    const npv = p.direction==='KRW 지급 / USD 수취' ? utk-kt : kt-utk;
    const sk = p.direction==='KRW 지급 / USD 수취' ? -1 : 1;
    const annK = krw.filter(r=>r.cf||r.end>iso(val)).reduce((a,r)=>a+krwN*r.dcf*r.df,0);
    const annU = usd.filter(r=>r.cf||r.end>iso(val)).reduce((a,r)=>a+usdN*r.dcf*r.df,0)*p.fx_valuation;
    const curK = p.krw_type==='Fixed'?p.krw_fixed_rate:p.krw_spread, curU = p.usd_type==='Fixed'?p.usd_fixed_rate:p.usd_spread;
    const fair = {krw: annK? curK-npv/(sk*annK):null, usd: annU? curU-npv/(-sk*annU):null, krw_kind:p.krw_type, usd_kind:p.usd_type};
    return {fair,krw_notional:krwN,krw_coupon_pv:kc,usd_coupon_pv:uc,krw_principal_pv:kp,usd_principal_pv:up,
      krw_total:kt,usd_total:ut,usd_total_krw:utk,npv,df,krw,usd,boot_krw:bk,boot_usd:bu};
  }
  // 통화선도: F = S × DF_USD/DF_KRW (또는 S + 시장 스왑포인트), 평가금액 = 부호 × N × (F − K) × DF_KRW
  function fx(p){
    const val=parse(p.valuation), mat=parse(p.maturity), days=mat.n-val.n;
    if(!(days>0)) throw new Error('결제일이 평가일보다 뒤여야 합니다.');
    const t=days/365, ck=curve(bootstrap(p.krw_par_rates).curve), cu=curve(bootstrap(p.usd_par_rates).curve);
    const S=+p.spot, K=+p.contract_rate, N=+p.usd_notional, dfk=ck.df(t), dfu=cu.df(t), fcip=S*dfu/dfk;
    const sp=p.swap_point, mkt=sp!=null&&sp!=='', F=mkt?S+(+sp):fcip, sg=p.direction==='USD 매입'?1:-1, value=sg*N*(F-K)*dfk;
    const table=[1,2,3,6,9,12,24,36].map(m=>{ const d=edate(val,m), tt=(d.n-val.n)/365, f=S*cu.df(tt)/ck.df(tt);
      return {label:m<12?m+'M':(m/12)+'Y',date:iso(d),days:d.n-val.n,t:tt,krw_zero:ck.zero(tt),usd_zero:cu.zero(tt),fwd:f,point:f-S}; });
    return {value,npv:value,fwd:F,fwd_cip:fcip,use_mkt:mkt,point:F-S,point_cip:fcip-S,days,t,df_krw:dfk,df_usd:dfu,krw_zero:ck.zero(t),usd_zero:cu.zero(t),
      usd_leg:sg*N*F*dfk,krw_leg:sg*N*K*dfk,undisc:sg*N*(F-K),delta:sg*N*(mkt?dfk:dfu),sign:sg,maturity:iso(mat),table,_ck:ck,_cu:cu};
  }
  return {irs,crs,fx,bootstrap};
})();
if (typeof module!=="undefined") module.exports=SwapJS;

// option_engine.py 의 1:1 JS 사본 (옵션 이항모형)
const OptionJS = (() => {
  const MAX_N=1000, COMP={'단리(연1회복리)':1,'연 2회 복리':2,'연 4회 복리':4,'연 12회 복리(월복리)':12}, ANN={'일별':252,'주별':52,'월별':12};
  const DAY=86400000, dn=s=>{const [y,m,d]=s.slice(0,10).split('-').map(Number); return Date.UTC(y,m-1,d)/DAY;};
  const xround=x=>Math.floor(Math.abs(x)+0.5)*(x>=0?1:-1);
  const contRate=(q,m)=>{ const k=COMP[m]||2; return k*Math.log(1+q/k); };
  function histVol(prices,freq,nObs){
    let px=prices.filter(v=>v!==null&&v!==''&&isFinite(+v)).map(Number);
    if(nObs!=null) px=px.slice(0,Math.max(2,Math.min(+nObs,500)));
    const rets=[]; for(let i=1;i<px.length;i++) if(px[i]>0&&px[i-1]>0) rets.push(Math.log(px[i]/px[i-1]));
    if(rets.length<2) return {sd:null,sigma:0.2,count:rets.length};
    const m=rets.reduce((a,b)=>a+b,0)/rets.length, sd=Math.sqrt(rets.reduce((a,b)=>a+(b-m)*(b-m),0)/(rets.length-1));
    return {sd,sigma:sd*Math.sqrt(ANN[freq]||252),count:rets.length};
  }
  const pay=(kind,s,k)=>kind==='콜'?Math.max(s-k,0):Math.max(k-s,0);
  function value(p,preview=6){
    const days=dn(p.maturity)-dn(p.valuation), T=days/365, S0=+p.spot, K0=+p.strike;
    const irrMode=p.strike_mode==='이자율연동(IRR)', irr=+(p.strike_irr||0), kind=p.kind||'콜', style=p.style||'유러피언';
    const r = p.r!=null ? +p.r : contRate(+p.rate_quoted, p.rate_comp||'연 2회 복리');
    const sigma=+p.sigma, nRaw = p.step==='일간' ? days : xround(days/7), n=Math.max(1,Math.min(nRaw,MAX_N));
    const dt=T/n, u=Math.exp(sigma*Math.sqrt(dt)), d=1/u, pu=(Math.exp(r*dt)-d)/(u-d), disc=Math.exp(-r*dt);
    const KT = irrMode ? K0*Math.pow(1+irr,T) : K0, Kat = t => irrMode ? K0*Math.pow(1+irr,t*dt) : K0;
    const lp=pu>0?Math.log(pu):-Infinity, lq=pu<1?Math.log(1-pu):-Infinity, lnf=new Float64Array(n+1);
    for(let i=1;i<=n;i++) lnf[i]=lnf[i-1]+Math.log(i);
    let probSum=0, wsum=0;
    for(let j=0;j<=n;j++){ const lw=lnf[n]-lnf[j]-lnf[n-j]+j*lp+(n-j)*lq, w=lw>-745?Math.exp(lw):0; probSum+=w; wsum+=w*pay(kind,S0*Math.pow(u,j)*Math.pow(d,n-j),KT); }
    const euro=wsum*Math.exp(-r*T);
    const V=new Float64Array(n+1); for(let j=0;j<=n;j++) V[j]=pay(kind,S0*Math.pow(u,n-j)*Math.pow(d,j),Kat(n));
    const keep={}; if(n<=preview) keep[n]=Array.from(V.slice(0,n+1));
    let early=0;
    for(let t=n-1;t>=0;t--){ const Kt=Kat(t);
      for(let j=0;j<=t;j++){ let c=disc*(pu*V[j]+(1-pu)*V[j+1]);
        if(style==='아메리칸'){ const ex=pay(kind,S0*Math.pow(u,t-j)*Math.pow(d,j),Kt); if(ex>c){ early++; c=ex; } }
        V[j]=c; }
      if(t<=preview) keep[t]=Array.from(V.slice(0,t+1)); }
    const tree=V[0], price= style==='유러피언'?euro:tree, pv=Math.min(n,preview);
    const preview_S=[], preview_V=[]; for(let t=0;t<=pv;t++){ const row=[]; for(let j=0;j<=t;j++) row.push(S0*Math.pow(u,t-j)*Math.pow(d,j)); preview_S.push(row); preview_V.push(keep[t]||[]); }
    const qty=+(p.quantity||1)||1;
    return {price,quantity:qty,total:price*qty,euro,tree,method: style==='유러피언'?'닫힌형(만기 분포 확률가중합)':'이항트리 역산(매 스텝 조기행사 판단)',T,days,n_raw:nRaw,n,capped:nRaw>MAX_N,
      excel_tree_limit: style==='아메리칸'&&n>500, dt,u,d,p:pu,r,sigma,K_T:KT,prob_sum:probSum,early_nodes:early,intrinsic:pay(kind,S0,K0),preview_S,preview_V};
  }
  return {value,histVol,contRate};
})();
if (typeof module!=='undefined') module.exports.OptionJS=OptionJS;

// prs_engine.py 의 1:1 JS 사본 (PRS 주가수익스왑)
const PrsJS = (() => {
  const TEN=[0.25,0.5,0.75,1,1.5,2,2.5,3,4,5,7,10], DAY=86400000;
  const parse=s=>{ const [y,m,d]=String(s).slice(0,10).split('-').map(Number); return {y,m,d,n:Date.UTC(y,m-1,d)/DAY}; };
  const mk=(y,m,d)=>({y,m,d,n:Date.UTC(y,m-1,d)/DAY});
  const iso=a=>`${a.y}-${String(a.m).padStart(2,'0')}-${String(a.d).padStart(2,'0')}`;
  const mdays=(y,m)=>new Date(Date.UTC(y,m,0)).getUTCDate();
  const edate=(a,k)=>{ const t=a.m-1+k, y=a.y+Math.floor(t/12), m=((t%12)+12)%12+1; return mk(y,m,Math.min(a.d,mdays(y,m))); };
  function fill(v){ const out=[]; let last=null; for(const x of v){ if(x===null||x===''||x===undefined||isNaN(+x)) out.push(last); else { last=+x/100; out.push(last); } }
    if(out[0]==null) throw new Error('3M YTM은 비워둘 수 없습니다.'); return out; }
  function bootstrap(ytm,freq=2){
    const y=fill(ytm), grid=[], yg=[], node=[]; let sum=0;
    for(let k=1;k<=40;k++){ const t=k/4; grid.push(t);
      const cf=Math.abs(t*freq-Math.round(t*freq))<1e-9; let yt;
      if(t>=TEN[11]) yt=y[11]; else { let D=0; for(let i=0;i<12;i++) if(TEN[i]<=t+1e-12) D=i; yt=y[D]+(t-TEN[D])*(y[D+1]-y[D])/(TEN[D+1]-TEN[D]); }
      let df=null; if(t<=1) df=Math.pow(1+yt,-t); else if(cf){ const c=yt/freq; df=(1-c*sum)/(1+c); }
      if(df!=null&&cf) sum+=df;
      yg.push(yt); node.push(df==null?null:Math.pow(df,-1/t)-1); }
    const spot=node.map((v,i)=>{ if(v!=null) return v; let a=i-1; while(node[a]==null) a--; let b=i+1; while(node[b]==null) b++;
      return node[a]+(grid[i]-grid[a])*(node[b]-node[a])/(grid[b]-grid[a]); });
    return {t:grid,ytm:yg,spot};
  }
  const fromN=n=>{ const t=new Date(n*DAY); return mk(t.getUTCFullYear(),t.getUTCMonth()+1,t.getUTCDate()); };
  const roll=a=>{ const wd=new Date(a.n*DAY).getUTCDay(); return wd===6?fromN(a.n+2):wd===0?fromN(a.n+1):a; }; // 토·일 → 다음 평일
  const at=(s,t)=>{ if(t<=0.25) return s[0]; if(t>=10) return s[39]; const k=Math.floor(t*4); return s[k-1]+(t*4-k)*(s[k]-s[k-1]); };
  function value(p,detail=true){
    const val=parse(p.valuation), start=parse(p.start), term=+(p.term_years||4), mat=p.maturity?parse(p.maturity):edate(start,12*term);
    if(mat.n<=start.n) throw new Error('계약만료일이 계약개시일보다 뒤여야 합니다.');
    const basis=+(p.day_basis||365), K=+p.notional, prem=+p.premium, pm=+(p.pay_months||3), bp=(+(p.rate_shift_bp||0))/10000;
    const S0=(+p.usd_equity)*(+p.stake)*(+p.fx)/1e6*(1+(+(p.equity_chg||0))), sigma=+p.vol;
    const cost=(+(p.cost_dispose??0.01))+(+(p.cost_tax??0.0035)), freq=+(p.coupon_freq||2);
    const gov=p._gov||bootstrap(p.gov_ytm,freq), aa=p._aa||bootstrap(p.aa_ytm,freq);
    const G=t=>at(gov.spot,t)+bp, AA=t=>at(aa.spot,t)+bp, crr=String(p.model||'excel').toLowerCase()==='crr', FXD=crr?G:AA; const arrears=['후급','arrears'].includes(String(p.pay_timing||'선급')); const bizRoll=!(p.biz_roll===false||p.biz_roll===0||p.biz_roll==='0'||p.biz_roll==='false'), adj=bizRoll?roll:(a=>a);
    const rows=[]; let A=0, left=0, npay=0;
    for(let b=0;edate(start,b*pm).n<mat.n;b++){ npay++;
      const pay=edate(start,b*pm), n1=edate(start,(b+1)*pm), nxt=n1.n<mat.n?n1:mat, days=nxt.n-pay.n, amt=K*prem*days/basis, pd0=arrears?nxt:pay, pd=adj(pd0), live=pd.n>=val.n;
      const t=live?(pd.n-val.n)/basis:0, s=live?FXD(t):0, df=live?Math.pow(1+s,-t):0, pv=live?-amt*df:0;
      A+=pv; if(live) left+=amt;
      if(detail) rows.push({n:b,pay:iso(pay),next:iso(nxt),paydate:iso(pd),rolled:pd.n!==pd0.n,days,amount:amt,live,t,spot:s,df,pv});
    }
    const settle=adj(mat), daysLeft=settle.n-val.n; if(daysLeft<=0) throw new Error('평가기준일이 만기 이후입니다.');
    const gw=+(p.gap_days||5), ms=+(p.max_steps||300), gap=Math.max(gw,Math.ceil(daysLeft/ms)); let dt=gap/basis, N=Math.floor(daysLeft/gap);
    if(crr&&N>=1){ N=Math.max(1,Math.round(daysLeft/gap)); dt=daysLeft/N/basis; } // 순수 CRR: 트리가 만기일에 정확히 끝남
    if(N<1) throw new Error('만기까지 남은 기간이 트리 간격보다 짧습니다.');
    const u=Math.exp(sigma*Math.sqrt(dt)), d=1/u;
    const gs=[], as=[]; for(let i=0;i<=N;i++){ gs.push(G(i*dt)); as.push(AA(i*dt)); }
    const fg=[], fa=[], pp=[];
    for(let i=0;i<N;i++){ fg.push(Math.pow(Math.pow(1+gs[i+1],(i+1)*dt)/Math.pow(1+gs[i],i*dt),1/dt)-1);
      fa.push(Math.pow(Math.pow(1+as[i+1],(i+1)*dt)/Math.pow(1+as[i],i*dt),1/dt)-1); pp.push((Math.pow(1+fg[i],dt)-d)/(u-d)); }
    const V=new Float64Array(N+1), Q=new Float64Array(N+1);
    const taxRate=Math.max(0,+(p.gain_tax||0)), taxBearer=p.tax_bearer==='지급자'?'지급자':'수령자', TX=new Float64Array(N+1), XP=new Float64Array(N+1), XN=new Float64Array(N+1), keep=1-(taxBearer==='수령자'?taxRate:0);
    for(let j=0;j<=N;j++){ const sv=S0*Math.pow(u,N-2*j)*(1-cost); V[j]=sv-(crr?0:K); Q[j]=sv-K>0?1:0; TX[j]=taxRate*Math.max(sv-K,0); XP[j]=keep*Math.max(sv-K,0); XN[j]=Math.min(sv-K,0); } // 순수 CRR: 트리에는 주식 매각대금만, K는 고정 레그
    for(let i=N-1;i>=0;i--){ const pi=pp[i], g=fg[i], a=fa[i];
      for(let j=0;j<=i;j++){ const q=pi*Q[j]+(1-pi)*Q[j+1], disc=Math.pow(1+(crr?g:q*g+(1-q)*a),dt); V[j]=(pi*V[j]+(1-pi)*V[j+1])/disc; TX[j]=(pi*TX[j]+(1-pi)*TX[j+1])/disc; XP[j]=(pi*XP[j]+(1-pi)*XP[j+1])/disc; XN[j]=(pi*XN[j]+(1-pi)*XN[j+1])/disc; Q[j]=q; } }
    const taxPv=TX[0], taxDed=taxBearer==='수령자'?taxPv:0; V[0]-=taxDed;
    const kpv=K*Math.pow(1+gs[N],-N*dt), B=crr?V[0]-kpv:V[0];
    const out={A,B,fv:A+B,k_pv:kpv,leg_float:crr?V[0]:null,leg_fixed:crr?A-kpv:null,S0,K,q0:Q[0],tax_rate:taxRate,tax_bearer:taxBearer,tax_pv:taxPv,tax_ded:taxDed,B_gross:B+taxDed,B_pos:XP[0],B_neg:XN[0],model:crr?'crr':'excel',pay_timing:arrears?'후급':'선급',maturity:iso(mat),settle:iso(settle),biz_roll:bizRoll,term_days:mat.n-start.n,n_pay:npay,N,gap,gap_auto:gap>gw,dt,u,d,p0:pp[0],fg0:fg[0],fa0:fa[0],days:daysLeft,cost,sigma,premium_left:left,n_live:rows.filter(r=>r.live).length};
    if(detail){ out.rows=rows; out.curve={t:gov.t,gov:gov.spot,aa:aa.spot,gov_ytm:gov.ytm,aa_ytm:aa.ytm}; }
    return out;
  }
  return {value,bootstrap,TEN};
})();
if (typeof module!=='undefined') module.exports.PrsJS=PrsJS;

// cb_engine.py 의 1:1 JS 사본 (내재파생 사채: CB·EB·BW·콜/풋 옵션부 사채)
const CbJS = (() => {
  const TEN2=[0.25,0.5,0.75,1,2,3,5,10,20,30], TEN1=[0.25,1,2,3,5,10,20,30], tenOf=v=>v&&v.length===TEN1.length?TEN1:TEN2, GRID=120, DAY=86400000, F=10000;
  const parse=s=>{ const [y,m,d]=String(s).slice(0,10).split('-').map(Number); return {y,m,d,n:Date.UTC(y,m-1,d)/DAY}; };
  const mk=(y,m,d)=>({y,m,d,n:Date.UTC(y,m-1,d)/DAY});
  const fromN=n=>{ const dt=new Date(n*DAY); return mk(dt.getUTCFullYear(),dt.getUTCMonth()+1,dt.getUTCDate()); };
  const iso=a=>`${a.y}-${String(a.m).padStart(2,'0')}-${String(a.d).padStart(2,'0')}`;
  const mdays=(y,m)=>new Date(Date.UTC(y,m,0)).getUTCDate();
  const edate=(a,k)=>{ const t=a.m-1+k, y=a.y+Math.floor(t/12), m=((t%12)+12)%12+1; return mk(y,m,Math.min(a.d,mdays(y,m))); };
  const mb=(a,b)=>(b.y-a.y)*12+(b.m-a.m)+(b.d-a.d)/30;
  const rnd=x=>Math.floor(x+0.5);
  const blank=v=>v===null||v===undefined||v===''||isNaN(+v);
  function fill(vals){
    const TEN=tenOf(vals); const pts=[]; TEN.forEach((t,i)=>{ if(!blank(vals[i])) pts.push([t,+vals[i]/100]); });
    if(!pts.length) throw new Error('국고채 YTM을 하나 이상 입력하세요.');
    return TEN.map(t=>{ if(t<=pts[0][0]) return pts[0][1]; if(t>=pts[pts.length-1][0]) return pts[pts.length-1][1];
      for(let k=0;k<pts.length-1;k++){ const [a,ya]=pts[k],[b,yb]=pts[k+1]; if(a<=t&&t<=b) return ya+(yb-ya)*(t-a)/(b-a); } });
  }
  function bootstrap(ytm,freq=2){
    const TEN=tenOf(ytm), NT=TEN.length, y=fill(ytm), grid=[], node=[]; let sum=0;
    for(let k=1;k<=GRID;k++){ const t=k/4; grid.push(t);
      const cf=Math.abs(t*freq-Math.round(t*freq))<1e-9; let yt;
      if(t>=TEN[NT-1]) yt=y[NT-1]; else { let D=0; for(let i=0;i<NT;i++) if(TEN[i]<=t+1e-12) D=i; yt=y[D]+(t-TEN[D])*(y[D+1]-y[D])/(TEN[D+1]-TEN[D]); }
      let df=null; if(t<=1) df=Math.pow(1+yt,-t); else if(cf){ const c=yt/freq; df=(1-c*sum)/(1+c); }
      if(df!=null&&cf) sum+=df;
      node.push(df==null?null:Math.pow(df,-1/t)-1); }
    const spot=node.map((v,i)=>{ if(v!=null) return v; let a=i-1; while(node[a]==null) a--; let b=i+1; while(node[b]==null) b++;
      return node[a]+(grid[i]-grid[a])*(node[b]-node[a])/(grid[b]-grid[a]); });
    return {t:grid,spot,ytm:y};
  }
  const at=(s,t)=>{ if(t<=0.25) return s[0]; if(t>=GRID/4) return s[GRID-1]; const k=Math.floor(t*4); return s[k-1]+(t*4-k)*(s[k]-s[k-1]); };
  function accreted(c,pm,y,months){
    if(pm<=0) return F*Math.pow(1+y,months/12);
    const m=12/pm, k=months/pm, paid=Math.floor(k+1e-9); let s=0;
    for(let i=1;i<=paid;i++) s+=Math.pow(1+y/m,k-i);
    return F*Math.pow(1+y/m,k)-(c*F/m)*s;
  }
  function solve(f,lo,hi){ if(f(lo)<0||f(hi)>0) return null; for(let i=0;i<100;i++){ const m=(lo+hi)/2; if(f(m)>0) lo=m; else hi=m; } return (lo+hi)/2; }
  const zspread=(price,flows,spot,shift)=>solve(s=>flows.reduce((a,[t,cf])=>a+cf*Math.pow(1+at(spot,t)+shift+s,-t),0)-price,-0.5,3.0);
  const irr=(price,flows)=>solve(y=>flows.reduce((a,[t,cf])=>a+cf*Math.pow(1+y,-t),0)-price,-0.9,5.0);

  const RV_EXP=[1/12,0.25,0.5,1,2,3,5,7,10], RV_TEN=[1,2,3,5,7,10];
  const lin=(xs,ys,x)=>{ if(x<=xs[0]) return ys[0]; if(x>=xs[xs.length-1]) return ys[ys.length-1]; for(let k=0;k<xs.length-1;k++) if(xs[k]<=x&&x<=xs[k+1]) return ys[k]+(ys[k+1]-ys[k])*(x-xs[k])/(xs[k+1]-xs[k]); return ys[ys.length-1]; };
  // 스텝별 BDT 금리 변동성: 고정값, 또는 Black 매트릭스에서 (옵션만기 t_i, 스왑기간 T−t_i) co-terminal 칸 이중 선형보간
  function rateSigmas(p,N,dt,T){ const fixed=Math.max(1e-6,+(p.rate_vol??0.15)), m=p.rate_vol_mode==='matrix'?p.rate_vol_matrix:null;
    if(!m) return {sig:new Array(N).fill(fixed),mode:'fixed'};
    const ex=(m.exp||RV_EXP).map(Number), tn=(m.ten||RV_TEN).map(Number), V=m.vol.map(r=>r.map(v=>(+v)/100)), sig=[];
    for(let i=0;i<N;i++){ const t=i*dt, ten=Math.max(T-t,1e-9), col=V.map(r=>lin(tn,r,ten)); sig.push(Math.max(1e-6,lin(ex,col,t))); }
    return {sig,mode:'matrix'}; }
  function value(p,detail=true){
    const kind=p.kind||'CB', val=parse(p.valuation), iss=parse(p.issue), mat=parse(p.maturity);
    if(mat.n<=val.n) throw new Error('만기일은 평가일보다 뒤여야 합니다.');
    if(val.n<iss.n) throw new Error('평가일은 발행일 이후여야 합니다.');
    const units=(+(p.face_total??1e10))/F, c=+(p.coupon||0), pm=+(p.pay_months??3)||0, ytp=+(p.ytp||0);
    const bp=(+(p.rate_shift_bp||0))/1e4, cs=(+(p.credit_spread??0.03))+(+(p.spread_shift_bp||0))/1e4;
    const S0=(+(p.spot??10000))*(1+(+(p.spot_chg||0)));
    const sig=Math.max(1e-6,(+(p.vol??0.3))+(+(p.vol_shift||0))), q=+(p.div_yield||0);
    const gov=p._gov||bootstrap(p.gov_ytm,+(p.coupon_freq||2));
    const Z=t=>at(gov.spot,t)+bp, DFrf=t=>Math.pow(1+Z(t),-t), DFrk=t=>Math.pow(1+Z(t)+cs,-t);
    const days=mat.n-val.n, T=days/365, sd=Math.max(1,Math.floor(+(p.step_days||7)));
    const N=Math.max(2,Math.min(Math.floor(+(p.max_steps||300)),Math.ceil(days/sd))), dt=T/N;
    const stp=d=>Math.max(0,Math.min(N,rnd(((d.n-val.n)/365)/dt)));
    const tt=[]; for(let i=0;i<=N;i++) tt.push(i*dt);
    const RS=rateSigmas(p,N,dt,T), SR=RS.sig;
    const stepOn=!!p.step_on, sStart=+(p.step_start??6), sRate=+(p.step_rate??0.08), sInc=+(p.step_inc??0.01), sCap=+(p.step_cap??0.20);
    const rateFrom=d0=>{ if(stepOn){ const ys=mb(iss,d0)/12; if(ys>=sStart-1e-9) return Math.min(sCap,sRate+Math.floor(ys-sStart+1e-9)*sInc); } return c; };
    const couponAt=new Array(N+1).fill(0), cpnStep=new Array(N+1).fill(false), flows=[]; let acc=0;
    const totalM=mb(iss,mat);
    if(pm>0){ let k=1, prev=iss;
      while(true){ const d=edate(iss,k*pm); if(d.n>mat.n) break; const amt=rateFrom(prev)*F*pm/12;
        if(d.n>val.n){ cpnStep[stp(d)]=true; if(amt>0){ couponAt[stp(d)]+=amt; flows.push([(d.n-val.n)/365,amt]); } if(prev.n<=val.n) acc=amt*(val.n-prev.n)/Math.max(1,d.n-prev.n); }
        prev=d; k++; } }
    const redemption = stepOn ? F : pm>0 ? Math.max(F,accreted(c,pm,ytp,rnd(totalM))) : F*Math.pow(1+ytp,(mat.n-iss.n)/365);
    flows.push([T,redemption]);
    const putAt=new Array(N+1).fill(null), sched=[];
    if(p.put_on){ const py=+(p.put_yield??ytp), every=Math.max(1,Math.floor(+(p.put_every||3))), d0=parse(p.put_start);
      for(let j=0;;j++){ const d=edate(d0,j*every); if(d.n>=mat.n) break;
        if(d.n>=val.n){ const amt=accreted(c,pm,py,rnd(mb(iss,d))); putAt[stp(d)]=amt; sched.push({type:'풋(조기상환)',date:iso(d),price:amt}); } } }
    const callAt=new Array(N+1).fill(null); let alpha=0;
    if(p.call_on){ alpha=Math.min(1,Math.max(0,+(p.call_ratio??0.3))); const cy=+(p.call_yield??ytp), par=!!p.call_par, cpnOnly=!!p.call_coupon_only;
      const a=parse(p.call_start), b0=parse(p.call_end), b=b0.n<mat.n?b0:mat;
      for(let i=0;i<=N;i++){ const d=fromN(val.n+rnd(tt[i]*365)); if(a.n<=d.n&&d.n<=b.n&&(!cpnOnly||cpnStep[i])) callAt[i]= par?F:accreted(c,pm,cy,mb(iss,d)); }
      sched.push({type:`콜(${par?'중도상환':'매도청구'} ${(alpha*100).toFixed(0)}%)`,date:`${iso(a)} ~ ${iso(b)}`,price:par?F:accreted(c,pm,cy,mb(iss,a))}); }
    const drf=[], drk=[]; for(let i=0;i<N;i++){ drf.push(DFrf(tt[i+1])/DFrf(tt[i])); drk.push(DFrk(tt[i+1])/DFrk(tt[i])); }
    const sb=flows.reduce((s,[t,cf])=>s+cf*DFrk(t),0);
    // BDT
    const sq=Math.sqrt(dt), U=new Float64Array(N); let Qa=[1];
    for(let i=0;i<N;i++){ const P=DFrk(tt[i+1]); let u=-Math.log(drk[i])/dt;
      for(let it=0;it<60;it++){ let fs=0,fp=0; for(let j=0;j<=i;j++){ const e=Math.exp(SR[i]*(2*j-i)*sq), df=Math.exp(-u*e*dt); fs+=Qa[j]*df; fp+=Qa[j]*df*(-e*dt); }
        const st=(fs-P)/fp; u-=st; if(Math.abs(st)<1e-14) break; }
      U[i]=u; const Qn=new Array(i+2).fill(0);
      for(let j=0;j<=i;j++){ const df=Math.exp(-u*Math.exp(SR[i]*(2*j-i)*sq)*dt); Qn[j]+=0.5*Qa[j]*df; Qn[j+1]+=0.5*Qa[j]*df; }
      Qa=Qn; }
    function rateTree(usePut,useCall){
      const V=new Float64Array(N+1).fill(redemption+couponAt[N]);
      for(let i=N-1;i>=0;i--) for(let j=0;j<=i;j++){ const r=U[i]*Math.exp(SR[i]*(2*j-i)*sq); let v=0.5*(V[j]+V[j+1])*Math.exp(-r*dt);
        if(useCall&&callAt[i]!=null&&v>callAt[i]) v=callAt[i];
        if(usePut&&putAt[i]!=null&&v<putAt[i]) v=putAt[i];
        V[j]=v+(i>0?couponAt[i]:0); }
      return V[0];
    }
    const hostNone=rateTree(false,false), hostPut=p.put_on?rateTree(true,false):hostNone;
    const hostPC= alpha>0 ? alpha*rateTree(true,true)+(1-alpha)*hostPut : hostPut;
    // 주가트리
    const u=Math.exp(sig*sq), d=1/u, pr=[]; for(let i=0;i<N;i++) pr.push((Math.exp(-q*dt)/drf[i]-d)/(u-d));
    const cp0=+(p.conv_price??10000), refix=!!p.refix_on, floor=(+(p.refix_floor??0.7))*cp0;
    const kRef= refix&&p.refix_start ? stp(parse(p.refix_start)) : 0;
    const ce=parse(p.conv_end||p.maturity), kCs=stp(parse(p.conv_start||p.issue)), kCe=stp(ce);
    const csOk=i=>kCs<=i&&i<=kCe&&ce.n>=val.n, ratio=+(p.bw_ratio??1);
    const cpAt=(i,S)=> (refix&&i>=kRef) ? Math.max(floor,Math.min(cp0,S)) : cp0;
    const sub=kind==='BW'&&p.bw_pay==='대용', cr=sub?ratio:1;
    const pw=new Float64Array(2*N+1); for(let k=-N;k<=N;k++) pw[k+N]=Math.pow(u,k);
    const sAt=(i,j)=>S0*pw[2*j-i+N];
    function tfTree(useCall){
      const E=new Float64Array(N+1), D=new Float64Array(N+1);
      for(let j=0;j<=N;j++){ const s=sAt(N,j), bond=redemption+couponAt[N], cv=F*cr/cpAt(N,s)*s;
        if(csOk(N)&&cv>bond){ E[j]=cv; D[j]=0; } else { E[j]=0; D[j]=bond; } }
      for(let i=N-1;i>=0;i--){ const p_=pr[i], a_=drf[i], b_=drk[i], conv=csOk(i), ca=useCall?callAt[i]:null, pa=putAt[i];
        for(let j=0;j<=i;j++){ let e=a_*(p_*E[j+1]+(1-p_)*E[j]), dd=b_*(p_*D[j+1]+(1-p_)*D[j]);
          const s=sAt(i,j), cv=conv?F*cr/cpAt(i,s)*s:0;
          if(ca!=null&&e+dd>ca){ if(conv&&cv>=ca){ e=cv; dd=0; } else { e=0; dd=ca; } }
          if(pa!=null&&pa>e+dd){ e=0; dd=pa; }
          if(conv&&cv>e+dd){ e=cv; dd=0; }
          E[j]=e; D[j]=dd+(i>0?couponAt[i]:0); } }
      return [E[0]+D[0],E[0]];
    }
    function warrantTree(){
      const V=new Float64Array(N+1);
      for(let j=0;j<=N;j++){ const s=sAt(N,j), X=cpAt(N,s); V[j]=csOk(N)?(F*ratio/X)*Math.max(s-X,0):0; }
      for(let i=N-1;i>=0;i--){ const p_=pr[i], a_=drf[i], ok=csOk(i);
        for(let j=0;j<=i;j++){ let v=a_*(p_*V[j+1]+(1-p_)*V[j]); if(ok){ const s=sAt(i,j), X=cpAt(i,s); v=Math.max(v,(F*ratio/X)*Math.max(s-X,0)); } V[j]=v; } }
      return V[0];
    }
    let total, option, eqPart=0;
    if(kind==='CB'||kind==='EB'||sub){ const [nc,enc]=tfTree(false);
      if(alpha>0){ const [wc,ewc]=tfTree(true); total=alpha*wc+(1-alpha)*nc; eqPart=alpha*ewc+(1-alpha)*enc; } else { total=nc; eqPart=enc; }
      option=total-hostPC; }
    else if(kind==='BW'){ option=warrantTree(); total=hostPC+option; }
    else { option=0; total=hostPC; }
    const parity= kind!=='SB' ? (F/cp0)*S0 : 0;
    const out={kind,unit:F,units,total,total_amt:total*units,sb,host_none:hostNone,host_put:hostPut,host_pc:hostPC,put_value:hostPut-hostNone,call_value:hostPC-hostPut,
      option,eq_part:eqPart,accrued:acc,clean:total-acc,parity,premium:parity>0?total/parity-1:null,redemption,
      z_total:zspread(total,flows,gov.spot,bp),z_host:zspread(hostPC,flows,gov.spot,bp),z_sb:zspread(sb,flows,gov.spot,bp),ytm:irr(total,flows),
      N,dt,T,days,sigma:sig,S0,cs,p0:pr[0],calib_err:Math.abs(hostNone-sb),rate_vol_mode:RS.mode,rate_sig_min:Math.min(...(SR.length>1?SR.slice(1):SR)),rate_sig_max:Math.max(...(SR.length>1?SR.slice(1):SR))};
    if(detail){ out.flows=flows.map(([t,cf])=>({t,cf})); out.sched=sched; out.curve={t:gov.t,spot:gov.spot,ytm:gov.ytm}; out.rate_sigs=SR.map((sg,i)=>({t:tt[i],sigma:sg})); }
    return out;
  }
  function oas(p,price,lo=-0.05,hi=0.6){
    const q={...p,spread_shift_bp:0}; if(!q._gov) q._gov=bootstrap(q.gov_ytm,+(q.coupon_freq||2));
    const f=s=>value({...q,credit_spread:s},false).total-price;
    let a=lo,b=hi,fa=f(a),fb=f(b); if(fa<0||fb>0) return null;
    for(let i=0;i<60;i++){ const m=b-fb*(b-a)/(fb-fa), fm=f(m);
      if(Math.abs(fm)<1e-7||Math.abs(b-a)<1e-10) return m;
      if(fm*fb<0){ a=b; fa=fb; } else fa/=2;
      b=m; fb=fm; }
    return b;
  }
  return {value,bootstrap,fill,accreted,oas,zspread,TEN:TEN2};
})();
if (typeof module!=='undefined') module.exports.CbJS=CbJS;

/* ── 재무계산기 (TI BA II Plus 계열과 같은 계산 규칙) ── */
const FinJS = (() => {
  const DAY=86400000;
  const pd=s=>{ if(!s) return null; const [y,m,d]=String(s).slice(0,10).split('-').map(Number); return {y,m,d,n:Date.UTC(y,m-1,d)/DAY}; };
  const mk=(y,m,d)=>({y,m,d,n:Date.UTC(y,m-1,d)/DAY});
  const iso=a=>`${a.y}-${String(a.m).padStart(2,'0')}-${String(a.d).padStart(2,'0')}`;
  const mdays=(y,m)=>new Date(Date.UTC(y,m,0)).getUTCDate();
  const fromN=n=>{ const t=new Date(n*DAY); return mk(t.getUTCFullYear(),t.getUTCMonth()+1,t.getUTCDate()); };
  const isEom=a=>a.d===mdays(a.y,a.m);
  const addM=(a,k,eom)=>{ const t=a.m-1+k, y=a.y+Math.floor(t/12), m=((t%12)+12)%12+1, L=mdays(y,m); return mk(y,m,eom?L:Math.min(a.d,L)); };
  const num=v=>(v===''||v==null||!isFinite(+v))?null:+v;
  function solve(f,lo,hi,guess){ // 뉴턴 + 이분법 보완
    let x=guess??(lo+hi)/2;
    for(let k=0;k<100;k++){ const fx=f(x); if(!isFinite(fx)) break; if(Math.abs(fx)<1e-12) return x; const h=Math.max(1e-7,Math.abs(x)*1e-7), d=(f(x+h)-f(x-h))/(2*h); if(!isFinite(d)||d===0) break; const nx=x-fx/d; if(!isFinite(nx)||nx<=lo||nx>=hi) break; if(Math.abs(nx-x)<1e-14*Math.max(1,Math.abs(x))) return nx; x=nx; }
    // 이분법: 부호가 바뀌는 구간을 찾음
    const M=400; let a=lo, fa=f(a);
    for(let i=1;i<=M;i++){ const b=lo+(hi-lo)*i/M, fb=f(b); if(isFinite(fa)&&isFinite(fb)&&fa*fb<=0){ let A=a,B=b,FA=fa; for(let j=0;j<200;j++){ const m=(A+B)/2, fm=f(m); if(FA*fm<=0) B=m; else { A=m; FA=fm; } if(B-A<1e-15) break; } return (A+B)/2; } a=b; fa=fb; }
    throw new Error('해를 찾지 못했습니다 (Error 5: 해 없음)');
  }
  // ── TVM: PV·(1+i)^N + PMT·(1+i·S)·((1+i)^N − 1)/i + FV = 0
  function rate(t){ const PY=+t.PY||1, CY=+t.CY||PY; return Math.pow(1+(+t.IY)/100/CY, CY/PY)-1; }
  function tvmF(N,i,PV,PMT,FV,S){ if(Math.abs(i)<1e-13) return PV+PMT*N+FV; const g=Math.pow(1+i,N); return PV*g+PMT*(1+i*S)*(g-1)/i+FV; }
  function tvm(t,key){
    const PY=+t.PY||1, CY=+t.CY||PY, S=t.BGN?1:0, N=+t.N, PV=+t.PV, PMT=+t.PMT, FV=+t.FV;
    if(key==='IY'){
      const f=i=>tvmF(N,i,PV,PMT,FV,S);
      if(!(N>0)) throw new Error('N이 0보다 커야 합니다.');
      const i=solve(f,-0.9999,10,0.01);
      return CY*(Math.pow(1+i,PY/CY)-1)*100;
    }
    const i=rate(t);
    if(key==='PV'){ if(Math.abs(i)<1e-13) return -(PMT*N+FV); const g=Math.pow(1+i,N); return -(PMT*(1+i*S)*(g-1)/i+FV)/g; }
    if(key==='FV'){ if(Math.abs(i)<1e-13) return -(PV+PMT*N); const g=Math.pow(1+i,N); return -(PV*g+PMT*(1+i*S)*(g-1)/i); }
    if(key==='PMT'){ if(Math.abs(i)<1e-13) return -(PV+FV)/N; const g=Math.pow(1+i,N); return -(PV*g+FV)*i/((1+i*S)*(g-1)); }
    if(key==='N'){ if(Math.abs(i)<1e-13) return -(PV+FV)/PMT; const a=PMT*(1+i*S); const v=(a-FV*i)/(a+PV*i); if(!(v>0)) throw new Error('N을 계산할 수 없습니다 (Error 5).'); return Math.log(v)/Math.log(1+i); }
    throw new Error('알 수 없는 항목');
  }
  // ── 상환표 (AMORT): P1~P2 회차의 원금·이자 합계와 P2 후 잔액
  function amortRows(t){ const i=rate(t), N=Math.ceil(+t.N-1e-9), S=!!t.BGN, PMT=+t.PMT; let bal=+t.PV; const rows=[];
    for(let k=1;k<=Math.min(N,2000);k++){ let a; if(S){ a=k===1?0:bal*i; bal=bal+a; } else { a=bal*i; bal=bal+a; } bal+=PMT; rows.push({k,int:-a,prn:PMT+a,bal}); }
    return rows; }
  function amort(t,P1,P2){ const rows=amortRows(t); P1=Math.max(1,Math.round(+P1||1)); P2=Math.max(P1,Math.round(+P2||P1)); const sel=rows.filter(r=>r.k>=P1&&r.k<=P2);
    if(!sel.length) throw new Error('P1·P2가 회차 범위를 벗어났습니다.');
    return {BAL:sel[sel.length-1].bal, PRN:sel.reduce((s,r)=>s+r.prn,0), INT:sel.reduce((s,r)=>s+r.int,0), rows}; }
  // ── 현금흐름 (CF): flows=[{c,f}], 0번이 CF0
  function expand(flows){ const v=[]; flows.forEach((x,j)=>{ const n=j===0?1:Math.max(0,Math.round(+x.f||1)); for(let k=0;k<n;k++) v.push(+x.c||0); }); return v; }
  function npvOf(v,r){ let s=0; for(let t=0;t<v.length;t++) s+=v[t]/Math.pow(1+r,t); return s; }
  function cf(flows,I,RI){
    const v=expand(flows), r=(+I||0)/100, n=v.length-1, out={NPV:npvOf(v,r)};
    out.NFV=out.NPV*Math.pow(1+r,n);
    const pb=disc=>{ let cum=0; for(let t=0;t<v.length;t++){ const x=disc?v[t]/Math.pow(1+r,t):v[t]; const prev=cum; cum+=x; if(t>0&&prev<0&&cum>=0) return (t-1)+(-prev)/x; } return null; };
    out.PB=pb(false); out.DPB=pb(true);
    const pos=v.some(x=>x>0), neg=v.some(x=>x<0);
    out.IRR = pos&&neg ? (()=>{ try{ return solve(x=>npvOf(v,x),-0.9999,100,0.1)*100; }catch(e){ return null; } })() : null;
    if(pos&&neg&&n>0){ const ri=(RI==null||RI==='')?r:(+RI)/100; let fvp=0,pvn=0; v.forEach((x,t)=>{ if(x>0) fvp+=x*Math.pow(1+ri,n-t); else pvn+=x/Math.pow(1+r,t); }); out.MIRR=(Math.pow(fvp/(-pvn),1/n)-1)*100; } else out.MIRR=null;
    out.n=n; out.flows=v; return out;
  }
  // ── 채권 (BOND): 가격·수익률·경과이자·수정듀레이션 (액면 100 기준)
  const d360=(a,b)=>{ let d1=a.d, d2=b.d; if(d1===31) d1=30; if(d2===31&&d1>=30) d2=30; return (b.y-a.y)*360+(b.m-a.m)*30+(d2-d1); };
  function bondSetup(b){
    const sdt=pd(b.SDT), rdt=pd(b.RDT); if(!sdt||!rdt) throw new Error('결제일과 상환일을 입력하세요.'); if(rdt.n<=sdt.n) throw new Error('상환일이 결제일보다 뒤여야 합니다.');
    const f=+b.FREQ===1?1:2, step=12/f, eom=isEom(rdt); let k=0, nxt=rdt, prv=addM(rdt,-step,eom);
    while(prv.n>sdt.n){ k++; nxt=prv; prv=addM(rdt,-step*(k+1),eom); }
    const N=k+1, act=b.DAY!=='360';
    const E=act?nxt.n-prv.n:360/f, A=act?sdt.n-prv.n:d360(prv,sdt), DSC=act?nxt.n-sdt.n:E-A;
    return {f,N,E,A,DSC,prv,nxt,sdt,rdt};
  }
  function bondPrice(b,Y,st){ const s=st||bondSetup(b), C=+b.CPN||0, RV=+b.RV||100, f=s.f, c=C/f, y=Y/100/f;
    if(s.N===1) return (RV+c)/(1+(s.DSC/s.E)*y) - (s.A/s.E)*c;
    let p=RV/Math.pow(1+y,s.N-1+s.DSC/s.E); for(let k=1;k<=s.N;k++) p+=c/Math.pow(1+y,k-1+s.DSC/s.E); return p-(s.A/s.E)*c; }
  function bond(b,key){
    const s=bondSetup(b), C=+b.CPN||0, AI=(C/s.f)*(s.A/s.E); let YLD=num(b.YLD), PRI=num(b.PRI);
    if(key==='PRI'){ if(YLD==null) throw new Error('YLD(수익률)를 입력하세요.'); PRI=bondPrice(b,YLD,s); }
    else if(key==='YLD'){ if(PRI==null) throw new Error('PRI(가격)를 입력하세요.'); YLD=solve(y=>bondPrice(b,y,s)-PRI,-50,200,C||5); }
    let DUR=null; if(YLD!=null){ const h=1e-4, dirty=y=>bondPrice(b,y,s)+AI, P0=dirty(YLD); DUR=-(dirty(YLD+h)-dirty(YLD-h))/(2*h)/P0*100; }
    return {PRI,YLD,AI,DUR,prev:iso(s.prv),next:iso(s.nxt),N:s.N,E:s.E,A:s.A,DSC:s.DSC};
  }
  // ── 감가상각 (DEPR): SL·SYD·DB·DBX, M01 = 시작 월(1=1월 1일, 3.5=3월 중순)
  function deprSchedule(d){
    const L=+d.LIF, C=+d.CST, SV=+d.SAL||0, M=+d.M01||1, R=(+d.DB||200)/100, meth=d.METH||'SL';
    if(!(L>0)) throw new Error('LIF(내용연수)를 입력하세요.'); if(!(C>0)) throw new Error('CST(취득원가)를 입력하세요.');
    const fr=(13-M)/12, base=C-SV, years=Math.ceil(L+(fr<1?1:0)-1e-9), out=[]; let rbv=C;
    const syd=k=>{ const S=L*(L+1)/2; return k>=1&&k<=Math.ceil(L)?base*Math.max(0,L-k+1)/S:0; };
    for(let y=1;y<=years+1&&rbv>SV+1e-9;y++){
      let dep;
      if(meth==='SL'){ dep=base/L*(y===1?fr:1); }
      else if(meth==='SYD'){ dep=y===1?fr*syd(1):(1-fr)*syd(y-1)+fr*syd(y); }
      else { const ratio=R/L; dep=rbv*ratio*(y===1?fr:1);
        if(meth==='DBX'){ const usedYears=(y===1?0:fr+(y-2)), remain=L-usedYears, sl=remain>0?(rbv-SV)/remain*(y===1?fr:1):rbv-SV; if(sl>dep) dep=sl; } }
      dep=Math.min(dep,rbv-SV); if(dep<0) dep=0; rbv-=dep; out.push({YR:y,DEP:dep,RBV:rbv,RDV:rbv-SV});
      if(y>=years+1) break;
    }
    return out;
  }
  function depr(d){ const rows=deprSchedule(d), y=Math.max(1,Math.round(+d.YR||1)), r=rows[y-1]||{YR:y,DEP:0,RBV:rows.length?rows[rows.length-1].RBV:+d.CST,RDV:rows.length?rows[rows.length-1].RDV:(+d.CST-(+d.SAL||0))}; return {...r,rows}; }
  // ── 날짜 (DATE)
  function date(x,key){ const a=pd(x.DT1), b=pd(x.DT2), D=num(x.DBD), act=x.DAY!=='360';
    if(key==='DBD'){ if(!a||!b) throw new Error('DT1·DT2를 입력하세요.'); return act?b.n-a.n:d360(a,b); }
    if(!act) throw new Error('30/360 기준에서는 날짜를 거꾸로 계산하지 않습니다. ACT로 바꾸세요.');
    if(key==='DT2'){ if(!a||D==null) throw new Error('DT1·DBD를 입력하세요.'); return iso(fromN(a.n+Math.round(D))); }
    if(key==='DT1'){ if(!b||D==null) throw new Error('DT2·DBD를 입력하세요.'); return iso(fromN(b.n-Math.round(D))); } }
  const weekday=s=>{ const a=pd(s); return a?['일','월','화','수','목','금','토'][new Date(a.n*DAY).getUTCDay()]:''; };
  // ── 이자율 변환 (ICONV)
  function iconv(x,key){ const C=+x.CY||1; if(key==='EFF') return 100*(Math.pow(1+(+x.NOM)/100/C,C)-1); return 100*C*(Math.pow(1+(+x.EFF)/100,1/C)-1); }
  // ── 이익률 (PROFIT): MAR = (SEL−CST)/SEL, MU = (SEL−CST)/CST
  function profit(x,key){ const C=num(x.CST), S=num(x.SEL), M=num(x.MAR), U=num(x.MU);
    if(key==='MAR') return (S-C)/S*100; if(key==='MU') return (S-C)/C*100;
    if(key==='SEL') return M!=null&&x._via!=='MU'?C/(1-M/100):C*(1+U/100);
    if(key==='CST') return M!=null&&x._via!=='MU'?S*(1-M/100):S/(1+U/100); }
  // ── 손익분기 (BRKEVN): PFT = P·Q − (FC + VC·Q)
  function brkevn(x,key){ const FC=+x.FC||0, VC=+x.VC||0, P=+x.P||0, PFT=+x.PFT||0, Q=+x.Q||0;
    if(key==='PFT') return P*Q-FC-VC*Q; if(key==='Q') return (PFT+FC)/(P-VC); if(key==='P') return (PFT+FC)/Q+VC; if(key==='FC') return P*Q-VC*Q-PFT; if(key==='VC') return (P*Q-FC-PFT)/Q; }
  // ── 증감률 (Δ%): NEW = OLD·(1 + %CH/100)^#PD
  function pct(x,key){ const O=+x.OLD, N=+x.NEW, C=+x.CH, P=+x.PD||1;
    if(key==='CH') return (Math.pow(N/O,1/P)-1)*100; if(key==='NEW') return O*Math.pow(1+C/100,P); if(key==='OLD') return N/Math.pow(1+C/100,P); if(key==='PD') return Math.log(N/O)/Math.log(1+C/100); }
  // ── 통계 (STAT): 1-V · LIN · Ln · EXP · PWR
  function stat(pts,meth){
    meth=meth||'LIN'; const P=pts.filter(p=>num(p.x)!=null&&(meth==='1-V'||num(p.y)!=null)).map(p=>({x:+p.x,y:num(p.y)??1}));
    const n=P.length, o={n,meth}; if(!n) return o;
    const tx=meth==='Ln'||meth==='PWR'?(x=>Math.log(x)):(x=>x), ty=meth==='EXP'||meth==='PWR'?(y=>Math.log(y)):(y=>y);
    const sum=f=>P.reduce((s,p)=>s+f(p),0);
    o.Sx_=sum(p=>p.x); o.Sx2=sum(p=>p.x*p.x); o.xbar=o.Sx_/n; o.Sx=n>1?Math.sqrt((o.Sx2-n*o.xbar*o.xbar)/(n-1)):null; o.sx=Math.sqrt(Math.max(0,o.Sx2/n-o.xbar*o.xbar));
    if(meth==='1-V') return o;
    o.Sy_=sum(p=>p.y); o.Sy2=sum(p=>p.y*p.y); o.Sxy=sum(p=>p.x*p.y); o.ybar=o.Sy_/n; o.Sy=n>1?Math.sqrt((o.Sy2-n*o.ybar*o.ybar)/(n-1)):null; o.sy=Math.sqrt(Math.max(0,o.Sy2/n-o.ybar*o.ybar));
    const X=P.map(p=>tx(p.x)), Y=P.map(p=>ty(p.y)); if(X.concat(Y).some(v=>!isFinite(v))) throw new Error('Ln·EXP·PWR 회귀는 양수 값만 쓸 수 있습니다.');
    const mx=X.reduce((a,b)=>a+b,0)/n, my=Y.reduce((a,b)=>a+b,0)/n; let sxx=0,syy=0,sxy=0; for(let k=0;k<n;k++){ sxx+=(X[k]-mx)**2; syy+=(Y[k]-my)**2; sxy+=(X[k]-mx)*(Y[k]-my); }
    const b=sxy/sxx, a0=my-b*mx; o.b=b; o.a=(meth==='EXP'||meth==='PWR')?Math.exp(a0):a0; o.r=sxy/Math.sqrt(sxx*syy);
    o.predY=x=>meth==='LIN'?o.a+o.b*x:meth==='Ln'?o.a+o.b*Math.log(x):meth==='EXP'?o.a*Math.exp(o.b*x):o.a*Math.pow(x,o.b);
    o.predX=y=>meth==='LIN'?(y-o.a)/o.b:meth==='Ln'?Math.exp((y-o.a)/o.b):meth==='EXP'?Math.log(y/o.a)/o.b:Math.pow(y/o.a,1/o.b);
    return o;
  }
  return {tvm,rate,amort,amortRows,cf,bond,bondSetup,deprSchedule,depr,date,weekday,iconv,profit,brkevn,pct,stat,solve};
})();
if (typeof module!=='undefined') module.exports.FinJS=FinJS;

/* ───────── 공통 금리커브 (curve_engine.py 와 같은 계산) ───────── */
const CurveJS = (() => {
  const STEP = 0.25, FWD_START = [0, 0.25, 0.5, 0.75, 1, 2, 3, 5, 7, 10], FWD_TENOR = [0.25, 0.5, 0.75, 1, 2, 3, 5];
  const KIS_TENORS = [0.25, 0.5, 0.75, 1, 1.5, 2, 3, 5], KTB_LONG = [10, 20, 30];
  const GRADES = ['AAA','AA+','AA','AA-','A+','A','A-','BBB+','BBB','BBB-'];
  const interp = (x, xs, ys) => {
    if (x <= xs[0]) return ys[0];
    if (x >= xs[xs.length-1]) return ys[ys.length-1];
    for (let i = 0; i < xs.length-1; i++) if (xs[i] <= x && x <= xs[i+1]) return ys[i] + (ys[i+1]-ys[i])*(x-xs[i])/(xs[i+1]-xs[i]);
    return ys[ys.length-1];
  };
  function points(tenors, rates){
    const pts = tenors.map((t,i)=>[+t, rates[i]]).filter(p=>p[1]!==null&&p[1]!==''&&p[1]!==undefined&&isFinite(+p[1])).map(p=>[p[0], +p[1]/100]).sort((a,b)=>a[0]-b[0]);
    if (!pts.length) throw new Error('금리를 하나 이상 입력하세요.');
    return [pts.map(p=>p[0]), pts.map(p=>p[1])];
  }
  function build(tenors, rates, freq=2, maxT=null){
    const [xs, ys] = points(tenors, rates);
    maxT = maxT || xs[xs.length-1];
    const n = Math.max(1, Math.round(Math.ceil(maxT/STEP - 1e-9)));
    const grid = Array.from({length:n}, (_,k)=>+((k+1)*STEP).toFixed(10));
    const par = grid.map(t=>interp(t, xs, ys));
    const node = []; let s = 0;
    grid.forEach((t,i)=>{
      const y = par[i], cf = Math.abs(t*freq - Math.round(t*freq)) < 1e-9;
      let df = null;
      if (t <= 1 + 1e-12) df = Math.pow(1+y, -t);
      else if (cf) { const c = y/freq; df = (1 - c*s)/(1 + c); }
      if (df !== null && cf) s += df;
      node.push(df === null ? null : Math.pow(df, -1/t) - 1);
    });
    const zero = grid.map((t,i)=>{
      if (node[i] !== null) return node[i];
      let a = i-1; while (node[a] === null) a--;
      let b = i+1; while (b < n && node[b] === null) b++;
      if (b >= n) return node[a];
      return node[a] + (t-grid[a])*(node[b]-node[a])/(grid[b]-grid[a]);
    });
    const df = grid.map((t,i)=>Math.pow(1+zero[i], -t));
    const fwd = grid.map((t,i)=> i===0 ? zero[0] : Math.pow(df[i-1]/df[i], 1/(t-grid[i-1])) - 1);
    return { t:grid, par, zero, zero_cc:zero.map(z=>Math.log(1+z)), df, fwd, freq, src_t:xs, src_r:ys };
  }
  const zeroAt = (c,t) => interp(t, c.t, c.zero);
  const dfAt = (c,t) => t <= 0 ? 1 : Math.pow(1+zeroAt(c,t), -t);
  const fwdAt = (c,t1,t2) => t2 <= t1 ? null : Math.pow(dfAt(c,t1)/dfAt(c,t2), 1/(t2-t1)) - 1;
  const parAt = (c,t) => interp(t, c.t, c.par);
  const fwdMatrix = c => { const T = c.t[c.t.length-1]; return FWD_START.map(s=>FWD_TENOR.map(m=> s+m <= T+1e-9 ? fwdAt(c,s,s+m) : null)); };
  function ktbPoints(ecos, kisGov){
    const pts = {};
    if (kisGov) { KIS_TENORS.forEach((t,i)=>{ if (kisGov[i]!=null) pts[t]=kisGov[i]; }); KTB_LONG.forEach(t=>{ if (ecos[t]!=null) pts[t]=ecos[t]; }); }
    else Object.keys(ecos).forEach(t=>{ if (ecos[t]!=null) pts[+t]=ecos[t]; });
    const ts = Object.keys(pts).map(Number).sort((a,b)=>a-b);
    return [ts, ts.map(t=>pts[t])];
  }
  function corpPoints(gv, kisGov, kt, kr){
    const sp5 = gv[gv.length-1] - kisGov[kisGov.length-1];
    const ts = KIS_TENORS.slice(), rs = gv.slice();
    kt.forEach((t,i)=>{ if (t > 5) { ts.push(t); rs.push(kr[i] + sp5); } });
    return [ts, rs];
  }
  function value(p){
    const c = build(p.tenors, p.rates, +(p.freq||2), p.max_t||null);
    c.fwd_matrix = fwdMatrix(c); c.fwd_start = FWD_START; c.fwd_tenor = FWD_TENOR;
    return c;
  }
  return { build, value, zeroAt, dfAt, fwdAt, parAt, fwdMatrix, ktbPoints, corpPoints, interp, GRADES, KIS_TENORS, FWD_START, FWD_TENOR };
})();

module.exports={SwapJS,OptionJS,PrsJS,CbJS,FinJS,CurveJS};

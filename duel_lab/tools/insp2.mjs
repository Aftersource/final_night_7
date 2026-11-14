import { NodeIO } from '@gltf-transform/core';
const io = new NodeIO(); const doc = await io.read(process.argv[2]); const r = doc.getRoot();
const sc=r.listScenes()[0]; const walk=(n,d)=>{ if(d<4) console.log(' '.repeat(d*2)+n.getName(), n.getTranslation().map(x=>+x.toFixed(2)), n.getRotation().map(x=>+x.toFixed(2)), n.getScale().map(x=>+x.toFixed(2))); n.listChildren().forEach(c=>walk(c,d+1)); };
sc.listChildren().forEach(n=>walk(n,0));
const a=r.listAnimations()[0]; for (const ch of a.listChannels()) { const n=ch.getTargetNode().getName(), p=ch.getTargetPath(); if(n==='pelvis'||!['translation'].includes(p)&&false){ const s=ch.getSampler(); const inp=s.getInput().getArray(), o=s.getOutput(); if(p==='translation'){ let mn=[1e9,1e9,1e9],mx=[-1e9,-1e9,-1e9]; for(let i=0;i<o.getCount();i++){const e=o.getElement(i,[]); e.forEach((v,k)=>{mn[k]=Math.min(mn[k],v);mx[k]=Math.max(mx[k],v)})} console.log(n,p,'dur',inp[inp.length-1].toFixed(2),'min',mn.map(x=>+x.toFixed(2)),'max',mx.map(x=>+x.toFixed(2))); } } }
const tr=a.listChannels().filter(c=>c.getTargetPath()==='translation').length; console.log('translation channels',tr,'of',a.listChannels().length);

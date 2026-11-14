import { NodeIO } from '@gltf-transform/core';
import { ALL_EXTENSIONS } from '@gltf-transform/extensions';
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS);
const doc = await io.read(process.argv[2]); const r = doc.getRoot();
console.log('meshes', r.listMeshes().length, 'tex', r.listTextures().map(t=>t.getName()+':'+t.getSize()), 'skins', r.listSkins().length, 'joints', r.listSkins()[0]?.listJoints().length);
console.log('anims', r.listAnimations().map(a=>a.getName()+':'+a.listChannels().length));
console.log('joints', r.listSkins()[0]?.listJoints().map(j=>j.getName()).join(' '));
let v=0; for (const m of r.listMeshes()) for (const p of m.listPrimitives()) v+=p.getAttribute('POSITION').getCount(); console.log('verts', v);
